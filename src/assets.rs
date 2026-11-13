//! Asset handling: theme assets, post images and remote image downloading.

use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use regex::Regex;

use crate::error::{Error, IoContext, Result};
use crate::output::Output;
use crate::theme::Theme;

/// User agent sent when downloading remote images.
const USER_AGENT: &str = concat!("blogen/", env!("CARGO_PKG_VERSION"));

/// Copy all theme assets into `<output>/<assets_subdir>/`.
///
/// Returns the number of asset files copied.
pub fn copy_theme_assets(output: &mut Output, theme: &Theme, assets_subdir: &str) -> Result<usize> {
    let assets_subdir = assets_subdir.trim_matches('/');
    let assets = theme.assets()?;
    let count = assets.len();
    for asset in assets {
        let relative = format!("{assets_subdir}/{}", asset.relative_path);
        output.write(&relative, &asset.contents)?;
    }
    Ok(count)
}

/// Copy `<posts_dir>/<images_dir>` to `<output>/<images_dir>`.
///
/// Returns the number of copied files (0 when the directory does not exist).
pub fn copy_post_images(output: &mut Output, posts_dir: &Path, images_dir: &str) -> Result<usize> {
    let images_dir = images_dir.trim_matches('/');
    let source_root = posts_dir.join(images_dir);
    if !source_root.is_dir() {
        return Ok(0);
    }
    let mut count = 0;
    for entry in walkdir::WalkDir::new(&source_root).sort_by_file_name() {
        let entry = entry.map_err(|err| {
            Error::io(
                format!("walking images directory `{}`", source_root.display()),
                std::io::Error::other(err),
            )
        })?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(&source_root)
            .expect("walkdir entry inside images root")
            .to_string_lossy()
            .replace('\\', "/");
        let contents = std::fs::read(entry.path())
            .with_context(|| format!("reading image `{}`", entry.path().display()))?;
        output.write(&format!("{images_dir}/{relative}"), &contents)?;
        count += 1;
    }
    Ok(count)
}

/// Download remote images referenced from an article and rewrite the URLs to
/// relative local paths.
///
/// * `article_relative_path` – article path relative to the posts root, used to
///   compute the correct number of `../` segments.
/// * `posts_images_dir` – absolute path of `<posts>/<images_dir>`.
/// * `images_url_dir` – configured image directory name (usually `images`).
///
/// When `enabled` is `false` the markdown is returned unchanged.
pub fn resolve_remote_images(
    markdown: &str,
    article_relative_path: &Path,
    posts_images_dir: &Path,
    images_url_dir: &str,
    enabled: bool,
) -> Result<String> {
    if !enabled {
        return Ok(markdown.to_string());
    }
    let urls: Vec<String> = remote_image_regex()
        .captures_iter(markdown)
        .filter_map(|captures| captures.get(1).map(|m| m.as_str().to_string()))
        .collect();
    if urls.is_empty() {
        return Ok(markdown.to_string());
    }

    // The original implementation replaces the URL text everywhere it occurs.
    let mut replaced = markdown.to_string();
    let mut seen = std::collections::BTreeSet::new();
    let depth = article_relative_path.components().count().saturating_sub(1);
    let prefix = format!("{}{images_url_dir}/", "../".repeat(depth));

    for url in urls {
        if !seen.insert(url.clone()) {
            continue;
        }
        let file_name = download_image(&url, posts_images_dir)?;
        let replacement = format!("{prefix}{file_name}");
        replaced = replaced.replace(&url, &replacement);
    }
    Ok(replaced)
}

/// Match the remote image URLs that the original pipeline downloads.
fn remote_image_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        // Equivalent to the original `!\[.*\]\((https?://[^)]+)\)` per line,
        // but keeps the capture to the URL itself.
        Regex::new(r"!\[[^\]\n]*\]\((https?://[^)\s]+)\)").expect("valid regex")
    })
}

/// Download one image and store it content-addressed in `images_dir`.
///
/// Returns the file name (e.g. `d41d8cd98f00b204e9800998ecf8427e.png`).
fn download_image(url: &str, images_dir: &Path) -> Result<String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|err| Error::RemoteImage {
            url: url.to_string(),
            message: err.to_string(),
        })?;
    let response = client.get(url).send().map_err(|err| Error::RemoteImage {
        url: url.to_string(),
        message: err.to_string(),
    })?;
    if !response.status().is_success() {
        return Err(Error::RemoteImage {
            url: url.to_string(),
            message: format!("HTTP status {}", response.status()),
        });
    }
    let bytes = response.bytes().map_err(|err| Error::RemoteImage {
        url: url.to_string(),
        message: err.to_string(),
    })?;

    let extension = image_extension(&bytes);
    let hash = crate::hashing::md5_hex(&bytes);
    let file_name = format!("{hash}.{extension}");
    let path = images_dir.join(&file_name);
    if !path.is_file() {
        std::fs::create_dir_all(images_dir)
            .with_context(|| format!("creating images directory `{}`", images_dir.display()))?;
        std::fs::write(&path, &bytes)
            .with_context(|| format!("writing downloaded image `{}`", path.display()))?;
    }
    Ok(file_name)
}

/// Determine the file extension for downloaded image bytes.
///
/// Mirrors the original pipeline: detected type, `xml` mapped to `svg`, and
/// `svg` as the fallback when the type cannot be detected.
fn image_extension(bytes: &[u8]) -> String {
    match infer::get(bytes) {
        Some(kind) => {
            let extension = kind.extension();
            if extension == "xml" {
                "svg".to_string()
            } else {
                extension.to_string()
            }
        }
        None => "svg".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_extension_detection() {
        // PNG magic bytes.
        let png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        assert_eq!(image_extension(&png), "png");
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"></svg>"#;
        assert_eq!(image_extension(svg), "svg");
        assert_eq!(image_extension(b"garbage"), "svg");
    }

    #[test]
    fn remote_regex_matches_original_pattern() {
        let markdown = "![alt](https://example.com/a.png)\ntext\n![x](http://e.com/b?q=1)";
        let urls: Vec<&str> = remote_image_regex()
            .captures_iter(markdown)
            .filter_map(|captures| captures.get(1).map(|m| m.as_str()))
            .collect();
        assert_eq!(
            urls,
            vec!["https://example.com/a.png", "http://e.com/b?q=1"]
        );
        assert!(remote_image_regex()
            .find("![alt](../images/local.png)")
            .is_none());
    }

    #[test]
    fn copies_post_images() {
        let temp = tempfile::tempdir().unwrap();
        let posts = temp.path().join("posts");
        std::fs::create_dir_all(posts.join("images/sub")).unwrap();
        std::fs::write(posts.join("images/a.png"), "a").unwrap();
        std::fs::write(posts.join("images/sub/b.png"), "b").unwrap();
        let mut output = Output::new(temp.path().join("out"), true).unwrap();
        let count = copy_post_images(&mut output, &posts, "images").unwrap();
        assert_eq!(count, 2);
        output.finish().unwrap();
        assert!(temp.path().join("out/images/a.png").is_file());
        assert!(temp.path().join("out/images/sub/b.png").is_file());
    }
}
