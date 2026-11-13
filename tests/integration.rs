//! End-to-end integration tests: init a site, build it and inspect the output.

use std::path::{Path, PathBuf};

use blogen::config::LoadedConfig;
use blogen::init::{init, InitOptions};
use blogen::site;

fn temp_site() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("tempdir");
    let dir = temp.path().join("site");
    init(&InitOptions {
        dir: dir.clone(),
        name: Some("Integration Test Blog".into()),
        url: Some("https://example.com/blog".into()),
        force: false,
        example: true,
    })
    .expect("init");
    (temp, dir)
}

fn build(dir: &Path) -> site::BuildReport {
    let loaded = LoadedConfig::from_file(dir.join("blogen.toml")).expect("load config");
    site::build(&loaded).expect("build site")
}

#[test]
fn init_build_and_outputs() {
    let (_temp, dir) = temp_site();
    let report = build(&dir);
    assert_eq!(report.visible_articles, 1);
    assert_eq!(report.hidden_articles, 0);
    assert_eq!(report.mermaid_diagrams, 1);

    let article = dir.join("public/hello-world.html");
    assert!(article.is_file(), "article page should be generated");
    let html = std::fs::read_to_string(&article).unwrap();
    assert!(html.contains("<title>Hello, blogen! | Integration Test Blog</title>"));
    assert!(html.contains("class=\"katex\""), "math should be rendered");
    assert!(
        html.contains("class=\"language-rust\""),
        "code should be highlighted"
    );
    assert!(
        html.contains("class=\"token keyword\""),
        "Prism tokens expected"
    );
    assert!(
        html.contains("class=\"table-of-contents\""),
        "toc not rendered"
    );
    assert!(html.contains("<table>"), "table not rendered");
    assert!(
        html.contains("<figure class=\"mermaid\" data-mermaid-digest="),
        "static mermaid figure missing"
    );
    assert!(
        html.contains("class=\"mermaid-light\"><svg"),
        "light mermaid svg missing"
    );
    assert!(
        html.contains("class=\"mermaid-dark\"><svg"),
        "dark mermaid svg missing"
    );
    assert!(
        !html.contains("mermaid-source"),
        "client-side mermaid source left over"
    );
    assert!(html.contains("/dist/main.css"));

    let index = std::fs::read_to_string(dir.join("public/index.html")).unwrap();
    assert!(index.contains("Integration Test Blog"));
    assert!(index.contains("/hello-world.html"));
    assert!(index.contains("Hello"));

    let index_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("public/index.json")).unwrap())
            .unwrap();
    let articles = index_json.as_array().expect("array");
    assert_eq!(articles.len(), 1);
    assert_eq!(articles[0]["title"], "Hello, blogen!");
    assert_eq!(articles[0]["date"], "2026.01.01");
    assert_eq!(articles[0]["url_path"], "/hello-world.html");

    let rss = std::fs::read_to_string(dir.join("public/rss.xml")).unwrap();
    assert!(rss.contains("<rss version=\"2.0\""));
    assert!(rss.contains("<title>Hello, blogen!</title>"));

    // Theme assets are copied to the configured assets directory.
    assert!(dir.join("public/dist/main.css").is_file());
    assert!(dir.join("public/dist/main.js").is_file());
    assert!(dir.join("public/dist/favicon.ico").is_file());
    assert!(dir.join("public/dist/katex.min.css").is_file());
    assert!(dir.join("public/dist/fonts").is_dir());

    // No profile was configured.
    assert!(!dir.join("profile").exists());

    // Manifest exists and the build is repeatable without rewriting pages.
    assert!(dir.join("public/.blogen-manifest.json").is_file());
    let second = build(&dir);
    assert_eq!(second.output.removed, 0);
}

#[test]
fn hidden_articles_and_display_name_authors() {
    let (_temp, dir) = temp_site();
    let posts = dir.join("posts");
    // Visible article with a display-name author.
    std::fs::write(
        posts.join("[2025.02.03 Rust,RDMA DeepSeek V4.1 Flash]visible.md"),
        "# Visible\n\nBody.\n",
    )
    .unwrap();
    // Hidden article in a nested directory.
    std::fs::create_dir_all(posts.join("hidden")).unwrap();
    std::fs::write(
        posts.join("hidden/[2024.01.01 Hidden]secret.md"),
        "# Secret\n\nHidden body.\n",
    )
    .unwrap();

    let report = build(&dir);
    assert_eq!(report.visible_articles, 2); // example + visible
    assert_eq!(report.hidden_articles, 1);

    // Hidden pages are generated with the `.htm` suffix and no comments.
    let hidden = std::fs::read_to_string(dir.join("public/hidden/secret.htm")).unwrap();
    assert!(hidden.contains("<title>Secret | Integration Test Blog</title>"));
    assert!(!hidden.contains("giscus"));

    // Display names do not get a GitHub profile link but are rendered.
    let visible = std::fs::read_to_string(dir.join("public/visible.html")).unwrap();
    assert!(visible.contains("DeepSeek V4.1 Flash"));
    assert!(!visible.contains("https://github.com/DeepSeek V4.1 Flash"));

    let index_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("public/index.json")).unwrap())
            .unwrap();
    let articles = index_json.as_array().unwrap();
    assert_eq!(articles.len(), 2);
    assert!(!articles.iter().any(|a| a["title"] == "Secret"));

    let rss = std::fs::read_to_string(dir.join("public/rss.xml")).unwrap();
    assert!(!rss.contains("<title>Secret</title>"));
}

#[test]
fn profile_generation_when_configured() {
    let (_temp, dir) = temp_site();
    let config_path = dir.join("blogen.toml");
    let mut config = std::fs::read_to_string(&config_path).unwrap();
    config.push_str("\n[profile]\npath = \"profile\"\nlimit = 5\n");
    std::fs::write(&config_path, config).unwrap();

    build(&dir);
    let readme = std::fs::read_to_string(dir.join("profile/README.md")).unwrap();
    assert!(readme.contains("Hello, blogen!"));
    assert!(readme.contains("https://example.com/blog/hello-world.html"));
}

#[test]
fn giscus_comments_are_rendered_for_visible_articles() {
    let (_temp, dir) = temp_site();
    let config_path = dir.join("blogen.toml");
    let mut config = std::fs::read_to_string(&config_path).unwrap();
    config.push_str(
        "\n[comments.giscus]\nrepo = \"owner/repo\"\nrepo_id = \"R_123\"\ncategory = \"General\"\n",
    );
    std::fs::write(&config_path, config).unwrap();

    build(&dir);
    let article = std::fs::read_to_string(dir.join("public/hello-world.html")).unwrap();
    assert!(article.contains("https://giscus.app/client.js"));
    assert!(article.contains("data-repo=\"owner/repo\""));
    assert!(article.contains("data-repo-id=\"R_123\""));
    assert!(article.contains("data-category=\"General\""));

    let index = std::fs::read_to_string(dir.join("public/index.html")).unwrap();
    assert!(!index.contains("giscus.app"));
}

#[test]
fn invalid_config_is_reported() {
    let (_temp, dir) = temp_site();
    let config_path = dir.join("blogen.toml");
    let config = std::fs::read_to_string(&config_path).unwrap();
    std::fs::write(
        &config_path,
        config.replace("url = \"https://example.com/blog\"", "url = \"not a url\""),
    )
    .unwrap();
    let error = LoadedConfig::from_file(&config_path).unwrap_err();
    assert!(error.to_string().contains("site.url"));
}

#[test]
fn stale_outputs_are_cleaned() {
    let (_temp, dir) = temp_site();
    build(&dir);
    let extra_article = dir.join("public/hello-world.html");
    assert!(extra_article.is_file());

    // Remove the source article: the generated page must disappear.
    std::fs::remove_file(dir.join("posts/[2026.01.01 Hello]hello-world.md")).unwrap();
    build(&dir);
    assert!(!extra_article.exists());
}
