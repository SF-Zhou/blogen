//! Article discovery and parsing of the filename/markdown conventions.
//!
//! The blog stores metadata in the file name:
//!
//! ```text
//! posts/rdma/[2026.11.12 Rust,RDMA,Memory,RPC DeepSeek V4.1 Flash]ruapc_safe_zero_copy.md
//! ```
//!
//! * `[` … `]` – metadata block (optional)
//! * first whitespace separated token – date, `YYYY.MM.DD`
//! * second token – comma separated tags (a single token in the filename)
//! * remaining tokens – author display name (may contain spaces)
//! * text after `]` – output filename
//!
//! The article title is the first line when it starts with `# `.

use std::path::{Path, PathBuf};

use serde::Serialize;
use walkdir::WalkDir;

use crate::error::{Error, IoContext, Result};

/// File name metadata extracted from the `[date tags author]` prefix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ParsedFileName {
    /// Date as written in the filename (`YYYY.MM.DD`), if present.
    pub date: Option<String>,
    /// Tags, in filename order.
    pub tags: Vec<String>,
    /// Author, if present in the filename.
    pub author: Option<String>,
    /// File name without the metadata block and extension.
    pub filename: String,
}

/// Parse the base name (without extension) of an article file.
///
/// Mirrors the original `analyze_article`:
/// `/^\[([^\]]+)\]/` matched against the file name, then split on whitespace.
pub fn parse_file_name(base_name: &str) -> ParsedFileName {
    let Some(rest) = base_name.strip_prefix('[') else {
        return ParsedFileName {
            date: None,
            tags: Vec::new(),
            author: None,
            filename: base_name.to_string(),
        };
    };
    let Some(end) = rest.find(']') else {
        return ParsedFileName {
            date: None,
            tags: Vec::new(),
            author: None,
            filename: base_name.to_string(),
        };
    };
    let info: Vec<&str> = rest[..end].split_whitespace().collect();
    let filename = rest[end + 1..].to_string();
    let date = info.first().map(|s| (*s).to_string());
    let tags = info
        .get(1)
        .filter(|tags| !tags.is_empty())
        .map(|tags| tags.split(',').map(str::to_string).collect())
        .unwrap_or_default();
    let author = if info.len() > 2 {
        Some(info[2..].join(" "))
    } else {
        None
    };
    ParsedFileName {
        date,
        tags,
        author,
        filename,
    }
}

/// The markdown after the title heading has been removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedMarkdown {
    /// Article title: the first `# ` heading or the file name.
    pub title: String,
    /// Remaining markdown body.
    pub markdown: String,
}

/// Split the `# Title` first line from the markdown body.
pub fn split_title(markdown: &str, fallback_title: &str) -> ParsedMarkdown {
    if let Some(rest) = markdown.strip_prefix("# ") {
        // The original implementation treats the whole first line as the
        // title, including a possible trailing `\r`.
        let (first_line, remainder) = match rest.find('\n') {
            Some(index) => (&rest[..index], &rest[index + 1..]),
            None => (rest, ""),
        };
        ParsedMarkdown {
            title: first_line.trim_end_matches('\r').to_string(),
            markdown: remainder.to_string(),
        }
    } else {
        ParsedMarkdown {
            title: fallback_title.to_string(),
            markdown: markdown.to_string(),
        }
    }
}

/// GitHub profile URL for an author name.
///
/// Only GitHub-handle-looking names (`[A-Za-z0-9-]+`) get a profile link; a
/// display name such as `DeepSeek V4.1 Flash` is rendered without one.
pub fn author_url(author: &str) -> String {
    let is_handle = !author.is_empty()
        && author
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-');
    if is_handle {
        format!("https://github.com/{author}")
    } else {
        String::new()
    }
}

/// Result of walking the posts directory.
#[derive(Debug, Clone, Default)]
pub struct Discovery {
    /// Directories relative to the posts root, excluding the root itself.
    pub dirs: Vec<PathBuf>,
    /// Article files relative to the posts root.
    pub files: Vec<PathBuf>,
}

/// Recursively discover directories and article files under `posts_dir`.
///
/// The result is sorted so builds are deterministic. Symbolic links are
/// followed for files but directory symlinks are not walked (matching the
/// original `lstat` based traversal closely enough while avoiding cycles).
pub fn discover(posts_dir: &Path, article_format: &str, skip_dir: &str) -> Result<Discovery> {
    let extension = format!(".{}", article_format.trim_start_matches('.'));
    let mut discovery = Discovery::default();

    let walker = WalkDir::new(posts_dir)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| {
            // Skip the image assets directory: it contains no articles and can
            // be huge.
            !(entry.file_type().is_dir() && entry.file_name() == skip_dir)
        });
    for entry in walker {
        let entry = entry.map_err(|err| {
            Error::io(
                format!("walking posts directory `{}`", posts_dir.display()),
                std::io::Error::other(err),
            )
        })?;
        if entry.depth() == 0 {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(posts_dir)
            .expect("walkdir paths are inside the root")
            .to_path_buf();
        if entry.file_type().is_dir() {
            discovery.dirs.push(relative);
        } else if entry.file_type().is_file() {
            let name = entry.file_name().to_string_lossy();
            if name.ends_with(&extension) {
                discovery.files.push(relative);
            }
        }
    }
    Ok(discovery)
}

/// Read a source markdown file into a string.
pub fn read_source(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("reading article `{}`", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_metadata() {
        let parsed = parse_file_name("[2026.11.12 Rust,RDMA,Memory,RPC DeepSeek V4.1 Flash]ruapc");
        assert_eq!(parsed.date.as_deref(), Some("2026.11.12"));
        assert_eq!(parsed.tags, vec!["Rust", "RDMA", "Memory", "RPC"]);
        assert_eq!(parsed.author.as_deref(), Some("DeepSeek V4.1 Flash"));
        assert_eq!(parsed.filename, "ruapc");
    }

    #[test]
    fn parses_metadata_without_author() {
        let parsed = parse_file_name("[2025.11.19 RDMA,Memory]post");
        assert_eq!(parsed.date.as_deref(), Some("2025.11.19"));
        assert_eq!(parsed.author, None);
        assert_eq!(parsed.filename, "post");
    }

    #[test]
    fn parses_metadata_with_single_tag() {
        let parsed = parse_file_name("[2024.06.03 Hidden]post");
        assert_eq!(parsed.tags, vec!["Hidden"]);
        assert_eq!(parsed.filename, "post");
    }

    #[test]
    fn missing_metadata_keeps_name() {
        let parsed = parse_file_name("plain_name");
        assert_eq!(parsed.date, None);
        assert_eq!(parsed.tags, Vec::<String>::new());
        assert_eq!(parsed.author, None);
        assert_eq!(parsed.filename, "plain_name");
    }

    #[test]
    fn malformed_metadata_is_not_metadata() {
        assert_eq!(parse_file_name("[unclosed").filename, "[unclosed");
    }

    #[test]
    fn splits_title() {
        let parsed = split_title("# Hello\n\nbody\n", "fallback");
        assert_eq!(parsed.title, "Hello");
        assert_eq!(parsed.markdown, "\nbody\n");
    }

    #[test]
    fn splits_title_crlf() {
        let parsed = split_title("# Hello\r\nbody", "fallback");
        assert_eq!(parsed.title, "Hello");
        assert_eq!(parsed.markdown, "body");
    }

    #[test]
    fn keeps_body_when_no_title() {
        let parsed = split_title("body only", "fallback");
        assert_eq!(parsed.title, "fallback");
        assert_eq!(parsed.markdown, "body only");
    }

    #[test]
    fn author_urls() {
        assert_eq!(author_url("SF-Zhou"), "https://github.com/SF-Zhou");
        assert_eq!(author_url("DeepSeek V4.1 Flash"), "");
        assert_eq!(author_url(""), "");
    }

    #[test]
    fn discovers_articles_and_dirs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a/b")).unwrap();
        std::fs::create_dir_all(dir.path().join("images")).unwrap();
        std::fs::write(dir.path().join("a/x.md"), "# x").unwrap();
        std::fs::write(dir.path().join("a/b/y.md"), "# y").unwrap();
        std::fs::write(dir.path().join("a/ignore.txt"), "nope").unwrap();
        std::fs::write(dir.path().join("images/pic.png"), "nope").unwrap();
        let discovery = discover(dir.path(), "md", "images").unwrap();
        assert_eq!(
            discovery.dirs,
            vec![PathBuf::from("a"), PathBuf::from("a/b")]
        );
        assert_eq!(
            discovery.files,
            vec![PathBuf::from("a/b/y.md"), PathBuf::from("a/x.md")]
        );
    }
}
