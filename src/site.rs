//! Build orchestration: discover, render and write a complete site.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::{DateTime, SecondsFormat, Utc};
use rayon::prelude::*;

use crate::article::{self, author_url, parse_file_name, split_title};
use crate::assets;
use crate::config::{Config, LoadedConfig};
use crate::error::{Error, IoContext, Result};
use crate::feed;
use crate::markdown::{MarkdownOptions, MarkdownRenderer};
use crate::model::{Article, ArticleSummary};
use crate::output::{write_file_if_changed, Output, OutputSummary};
use crate::templates::{PageView, RenderContext, TemplateEngine};
use crate::theme::Theme;

/// Report produced by a successful build.
#[derive(Debug, Clone)]
pub struct BuildReport {
    /// Build timestamp used in templates and the RSS feed.
    pub generated_at: DateTime<Utc>,
    /// Visible articles generated.
    pub visible_articles: usize,
    /// Hidden articles generated (`.htm` pages excluded from indexes).
    pub hidden_articles: usize,
    /// Mermaid diagrams found across all articles.
    pub mermaid_diagrams: usize,
    /// Output file statistics.
    pub output: OutputSummary,
    /// Non-fatal problems encountered during the build.
    pub warnings: Vec<String>,
    /// Wall-clock build duration.
    pub duration: Duration,
}

/// Build the whole site described by a loaded configuration.
pub fn build(loaded: &LoadedConfig) -> Result<BuildReport> {
    let started = Instant::now();
    let config = &loaded.config;
    let generated_at = Utc::now();
    let generated_at_string = generated_at.to_rfc3339_opts(SecondsFormat::Secs, true);

    let theme = Theme::load(&config.theme, &loaded.root)?;
    if config.profile.is_some() && theme.profile_template_name().is_none() {
        return Err(Error::Theme(format!(
            "`[profile]` is configured but theme `{}` does not provide a profile template \
             (`[templates].profile` in its theme.toml)",
            theme.manifest().name
        )));
    }
    let templates = TemplateEngine::new(theme.clone())?;

    let posts_dir = loaded.posts_dir();
    if !posts_dir.is_dir() {
        return Err(Error::validation(format!(
            "posts directory `{}` does not exist (config: `[build].posts`)",
            posts_dir.display()
        )));
    }
    let discovery = article::discover(
        &posts_dir,
        &config.build.article_format,
        &config.build.images,
    )?;
    tracing::info!(
        files = discovery.files.len(),
        dirs = discovery.dirs.len(),
        "discovered articles"
    );

    let renderer = MarkdownRenderer::new(MarkdownOptions {
        toc: config.markdown.toc,
        math: config.markdown.math,
        highlight: config.markdown.highlight,
        mermaid: config.markdown.mermaid,
    });
    let images_dir = loaded.posts_images_dir();
    let warnings = std::sync::Mutex::new(Vec::<String>::new());

    let render_one = |relative: &PathBuf| -> Result<Article> {
        render_article(
            relative,
            config,
            &posts_dir,
            &images_dir,
            &renderer,
            &warnings,
        )
    };
    let articles: Vec<Article> = run_parallel(config.build.parallel, || {
        discovery
            .files
            .par_iter()
            .map(render_one)
            .collect::<Result<Vec<_>>>()
    })?;

    let mut visible: Vec<Article> = articles.iter().filter(|a| !a.hidden).cloned().collect();
    sort_articles(&mut visible);
    let hidden_count = articles.len() - visible.len();
    let summaries: Vec<ArticleSummary> = visible.iter().map(|a| a.summary.clone()).collect();
    let mermaid_diagrams: usize = articles.iter().map(|a| a.mermaid_diagrams).sum();
    let warnings = warnings
        .into_inner()
        .map_err(|_| Error::validation("internal warning lock was poisoned"))?;

    // Detect output path collisions early: silently overwriting articles is a
    // data-loss bug we do not want to inherit from the old pipeline.
    let mut seen_paths: BTreeMap<String, &Path> = BTreeMap::new();
    for article in &articles {
        let relative = article_output_path(article);
        if let Some(previous) = seen_paths.insert(relative.clone(), article.source.as_path()) {
            return Err(Error::validation(format!(
                "two articles would be written to `{relative}`: `{}` and `{}`",
                previous.display(),
                article.source.display()
            )));
        }
    }

    let mut output = Output::new(loaded.output_dir(), config.build.clean)?;

    // Article pages.
    for article in &articles {
        let relative = article_output_path(article);
        let context = RenderContext::new(
            config,
            &theme,
            &generated_at_string,
            PageView::article(article, &config.site.name),
            Vec::new(),
        );
        let html = templates.render(theme.article_template_name(), &context)?;
        output.write_str(&relative, &html)?;
    }
    tracing::info!(
        visible = summaries.len(),
        hidden = hidden_count,
        "rendered article pages"
    );

    // Index page.
    let index_context = RenderContext::new(
        config,
        &theme,
        &generated_at_string,
        PageView::index(&config.site.name),
        summaries.clone(),
    );
    let index_html = templates.render(theme.index_template_name(), &index_context)?;
    output.write_str("index.html", &index_html)?;

    // Machine readable article index.
    let index_json = serde_json::to_string_pretty(&summaries).map_err(|err| Error::Io {
        context: "serializing index.json".to_string(),
        source: std::io::Error::other(err),
    })?;
    output.write_str("index.json", &index_json)?;

    // RSS feed.
    if config.rss.enabled {
        let rss = feed::render(config, &summaries, generated_at);
        output.write_str(&config.rss.file, &rss)?;
    }

    // Profile README (outside the output directory, like the old pipeline).
    if let (Some(profile), Some(profile_template)) =
        (&config.profile, theme.profile_template_name())
    {
        let profile_dir = loaded
            .profile_dir()
            .expect("profile directory is configured when [profile] exists");
        let limit = profile.limit;
        let context = RenderContext::new(
            config,
            &theme,
            &generated_at_string,
            PageView::profile(&config.site.name),
            summaries.iter().take(limit).cloned().collect(),
        );
        let readme = templates.render(profile_template, &context)?;
        std::fs::create_dir_all(&profile_dir)
            .with_context(|| format!("creating profile directory `{}`", profile_dir.display()))?;
        let readme_path = profile_dir.join("README.md");
        if write_file_if_changed(&readme_path, readme.as_bytes())? {
            tracing::info!(path = %readme_path.display(), "wrote profile README");
        }
    }

    // Static assets.
    let asset_count = assets::copy_theme_assets(&mut output, &theme, &config.theme.assets)?;
    let image_count = assets::copy_post_images(&mut output, &posts_dir, &config.build.images)?;
    tracing::info!(
        assets = asset_count,
        images = image_count,
        "copied static assets"
    );

    let output_summary = output.finish()?;
    let report = BuildReport {
        generated_at,
        visible_articles: summaries.len(),
        hidden_articles: hidden_count,
        mermaid_diagrams,
        output: output_summary,
        warnings,
        duration: started.elapsed(),
    };
    tracing::info!(
        written = report.output.written,
        unchanged = report.output.unchanged,
        removed = report.output.removed,
        elapsed_ms = report.duration.as_millis(),
        "build finished"
    );
    Ok(report)
}

/// Run `f` on a dedicated rayon pool when a thread count is configured.
fn run_parallel<T, F>(parallel: usize, f: F) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send,
    T: Send,
{
    if parallel == 0 {
        return f();
    }
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(parallel)
        .build()
        .map_err(|err| Error::Io {
            context: "creating the worker thread pool".to_string(),
            source: std::io::Error::other(err),
        })?;
    pool.install(f)
}

/// Render one article from disk.
fn render_article(
    relative_path: &Path,
    config: &Config,
    posts_dir: &Path,
    images_dir: &Path,
    renderer: &MarkdownRenderer,
    warnings: &std::sync::Mutex<Vec<String>>,
) -> Result<Article> {
    let source_path = posts_dir.join(relative_path);
    let source = article::read_source(&source_path)?;
    let base_name = relative_path
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_default();
    let parsed_name = parse_file_name(&base_name);
    let default_author = config.default_author();
    let author = parsed_name
        .author
        .clone()
        .unwrap_or_else(|| default_author.to_string());
    let parsed = split_title(&source, &parsed_name.filename);

    let markdown = assets::resolve_remote_images(
        &parsed.markdown,
        relative_path,
        images_dir,
        &config.build.images,
        config.build.download_remote_images,
    )?;
    let rendered_markdown = renderer.render(&markdown)?;

    if parsed_name.date.is_none() {
        if let Ok(mut warnings) = warnings.lock() {
            warnings.push(format!(
                "`{}` has no `[date ...]` metadata; RSS uses the build date",
                source_path.display()
            ));
        }
    }

    let dir = relative_path
        .parent()
        .map(|parent| parent.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let hidden = parsed_name.tags.iter().any(|tag| tag == "Hidden");
    let extension = if hidden { "htm" } else { "html" };
    let url_path = if dir.is_empty() {
        format!("/{}.{extension}", parsed_name.filename)
    } else {
        format!("/{dir}/{}.{extension}", parsed_name.filename)
    };
    let summary = ArticleSummary {
        title: parsed.title,
        date: parsed_name.date,
        author: author.clone(),
        tags: parsed_name.tags,
        filename: parsed_name.filename,
        author_url: author_url(&author),
        url_path,
    };
    Ok(Article {
        summary,
        dir,
        source: source_path,
        hidden,
        markdown,
        html: rendered_markdown.html,
        mermaid_diagrams: rendered_markdown.mermaid_diagrams,
    })
}

/// Output path (relative to the output root) of an article page.
fn article_output_path(article: &Article) -> String {
    let file_name = article.output_file_name();
    if article.dir.is_empty() {
        file_name
    } else {
        format!("{}/{file_name}", article.dir)
    }
}

/// Sort articles newest first, then by title.
pub fn sort_articles(articles: &mut [Article]) {
    articles.sort_by(|left, right| {
        let by_date = match (&left.summary.date, &right.summary.date) {
            (Some(left_date), Some(right_date)) => right_date.cmp(left_date),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        };
        by_date.then_with(|| left.summary.title.cmp(&right.summary.title))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn article(date: Option<&str>, title: &str) -> Article {
        Article {
            summary: ArticleSummary {
                title: title.to_string(),
                date: date.map(str::to_string),
                author: "a".into(),
                tags: vec![],
                filename: title.to_lowercase(),
                author_url: String::new(),
                url_path: format!("/{}.html", title.to_lowercase()),
            },
            dir: String::new(),
            source: PathBuf::from("x.md"),
            hidden: false,
            markdown: String::new(),
            html: String::new(),
            mermaid_diagrams: 0,
        }
    }

    #[test]
    fn sorts_by_date_desc_then_title() {
        let mut articles = vec![
            article(Some("2024.01.01"), "b"),
            article(Some("2025.01.01"), "c"),
            article(Some("2024.01.01"), "a"),
            article(None, "z"),
        ];
        sort_articles(&mut articles);
        let titles: Vec<&str> = articles.iter().map(|a| a.summary.title.as_str()).collect();
        assert_eq!(titles, vec!["c", "a", "b", "z"]);
    }

    #[test]
    fn article_output_paths() {
        let mut a = article(Some("2024.01.01"), "A");
        assert_eq!(article_output_path(&a), "a.html");
        a.dir = "nested/dir".into();
        assert_eq!(article_output_path(&a), "nested/dir/a.html");
        a.hidden = true;
        assert_eq!(article_output_path(&a), "nested/dir/a.htm");
    }
}
