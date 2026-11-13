//! Site model shared by the build pipeline and templates.

use std::path::PathBuf;

use serde::Serialize;

/// Summary of one article, as exposed to templates (`index.json`, card lists)
/// and RSS. Field order matches the JSON output of the original generator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArticleSummary {
    /// Article title (the first `# heading` line).
    pub title: String,
    /// Date in `YYYY.MM.DD` form, taken from the filename metadata. Omitted
    /// from `index.json` when the article has no date (matching the original
    /// JavaScript `undefined` behaviour).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    /// Author name; falls back to `site.default_author`.
    pub author: String,
    /// Tags from the filename metadata, in order.
    pub tags: Vec<String>,
    /// Output filename without extension (metadata stripped).
    pub filename: String,
    /// GitHub profile URL for GitHub-handle authors, empty otherwise.
    pub author_url: String,
    /// URL path of the generated page, e.g. `/rdma/post.html`.
    pub url_path: String,
}

/// One source article with everything needed to render it.
#[derive(Debug, Clone)]
pub struct Article {
    /// Public summary information.
    pub summary: ArticleSummary,
    /// Directory of the article relative to the posts root (`""` for top level).
    pub dir: String,
    /// Path of the source markdown file.
    pub source: PathBuf,
    /// `true` when the article carries the `Hidden` tag.
    pub hidden: bool,
    /// Article body markdown (title line removed).
    pub markdown: String,
    /// Rendered HTML of the article body.
    pub html: String,
    /// Number of Mermaid diagrams in the article body.
    pub mermaid_diagrams: usize,
}

impl Article {
    /// File name of the generated page (`name.html` or `name.htm` for hidden).
    pub fn output_file_name(&self) -> String {
        if self.hidden {
            format!("{}.htm", self.summary.filename)
        } else {
            format!("{}.html", self.summary.filename)
        }
    }
}

/// Headings collected while rendering one markdown document, used for the
/// table of contents and exposed to templates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    /// Heading level (1-6).
    pub level: u8,
    /// Slug used as the HTML `id` (not percent encoded).
    pub id: String,
    /// Plain text of the heading.
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary() -> ArticleSummary {
        ArticleSummary {
            title: "Title".into(),
            date: Some("2024.01.02".into()),
            author: "Author".into(),
            tags: vec![],
            filename: "post".into(),
            author_url: String::new(),
            url_path: "/post.html".into(),
        }
    }

    #[test]
    fn visible_output_name() {
        let article = Article {
            summary: summary(),
            dir: String::new(),
            source: PathBuf::from("post.md"),
            hidden: false,
            markdown: String::new(),
            html: String::new(),
            mermaid_diagrams: 0,
        };
        assert_eq!(article.output_file_name(), "post.html");
    }

    #[test]
    fn hidden_output_name() {
        let article = Article {
            summary: summary(),
            dir: String::new(),
            source: PathBuf::from("post.md"),
            hidden: true,
            markdown: String::new(),
            html: String::new(),
            mermaid_diagrams: 0,
        };
        assert_eq!(article.output_file_name(), "post.htm");
    }
}
