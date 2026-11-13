//! Template rendering (minijinja) and the data exposed to themes.

use std::path::Path;

use minijinja::value::Serde;
use minijinja::{AutoEscape, Environment, TemplateSource, UndefinedBehavior};
use serde::Serialize;

use crate::config::{Config, GiscusConfig};
use crate::error::{Error, Result};
use crate::model::{Article, ArticleSummary};
use crate::theme::Theme;

/// Maximum template rendering recursion (guards against malicious themes).
const MAX_TEMPLATE_DEPTH: usize = 64;

/// Engine that renders theme templates.
#[derive(Debug, Clone)]
pub struct TemplateEngine {
    env: Environment<'static>,
    theme: Theme,
}

impl TemplateEngine {
    /// Create an engine for the given theme.
    pub fn new(theme: Theme) -> Result<Self> {
        let mut env = Environment::new();
        let loader_theme = theme.clone();
        env.set_loader(move |name| match loader_theme.read_template_opt(name) {
            Ok(Some(source)) => Ok(Some(TemplateSource::new(source))),
            Ok(None) => Ok(None),
            Err(err) => Err(minijinja::Error::new(
                minijinja::ErrorKind::InvalidOperation,
                err.to_string(),
            )),
        });
        env.set_auto_escape_callback(|name| {
            let extension = Path::new(name)
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or_default();
            if extension.eq_ignore_ascii_case("html")
                || extension.eq_ignore_ascii_case("htm")
                || extension.eq_ignore_ascii_case("xml")
            {
                AutoEscape::Html
            } else {
                AutoEscape::None
            }
        });
        // Use conventional HTML escaping (`/` and `'` are not escaped). The
        // built-in minijinja escaper also escapes `/` as `&#x2f;`, which would
        // make every URL in generated HTML unreadable.
        env.set_formatter(|out, state, value| {
            if *state.auto_escape() == AutoEscape::Html && !value.is_safe() {
                let text = value.to_string();
                let escaped = escape_html(&text);
                out.write_str(&escaped).map_err(|err| {
                    minijinja::Error::new(
                        minijinja::ErrorKind::InvalidOperation,
                        format!("failed to write template output: {err}"),
                    )
                })?;
                Ok(())
            } else {
                minijinja::escape_formatter(out, state, value)
            }
        });
        // A typo in a template should fail the build instead of silently
        // rendering an empty string.
        env.set_undefined_behavior(UndefinedBehavior::Strict);
        env.set_recursion_limit(MAX_TEMPLATE_DEPTH);
        Ok(Self { env, theme })
    }

    /// The theme this engine renders.
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Render a template by name with the given context.
    pub fn render<T: Serialize>(&self, template: &str, context: &T) -> Result<String> {
        let value = minijinja::Value::from(Serde(context));
        self.env
            .get_template(template)
            .and_then(|template| template.render(value))
            .map_err(|err| {
                let mut message = err.to_string();
                if let Some(detail) = err.detail() {
                    message.push_str(": ");
                    message.push_str(detail);
                }
                if let Some(path) = err.name() {
                    if path != template {
                        message.push_str(" (included from ");
                        message.push_str(template);
                        message.push(')');
                    }
                }
                Error::Template(message)
            })
    }
}

/// Escape text for HTML text and attribute contexts.
fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Information about the generator itself, available to every template.
#[derive(Debug, Clone, Serialize)]
pub struct BlogenInfo {
    /// Crate version.
    pub version: String,
    /// UTC build timestamp in RFC 3339 form.
    pub generated_at: String,
}

/// Site configuration exposed to templates.
#[derive(Debug, Clone, Serialize)]
pub struct SiteView {
    /// Site name.
    pub name: String,
    /// Canonical site URL.
    pub url: String,
    /// Site description.
    pub description: String,
    /// Site language tag.
    pub language: String,
    /// Default author.
    pub default_author: String,
    /// Name used in the footer copyright.
    pub web_master: String,
    /// Google Analytics id, if configured.
    pub google_analytics_id: Option<String>,
    /// Normalized base path (`/` or `/sub/`).
    pub base_path: String,
    /// URL prefix of theme assets (`/dist`).
    pub assets_url: String,
    /// URL prefix of post images (`/images`).
    pub images_url: String,
}

impl SiteView {
    /// Build the view from the configuration.
    pub fn from_config(config: &Config) -> Self {
        Self {
            name: config.site.name.clone(),
            url: config.site.url.trim_end_matches('/').to_string(),
            description: config.site_description().to_string(),
            language: config.site_language().to_string(),
            default_author: config.default_author().to_string(),
            web_master: config.web_master().to_string(),
            google_analytics_id: config
                .site
                .google_analytics_id
                .clone()
                .filter(|id| !id.is_empty()),
            base_path: config.base_path(),
            assets_url: config.assets_url(),
            images_url: config.images_url(),
        }
    }
}

/// Theme metadata exposed to templates.
#[derive(Debug, Clone, Serialize)]
pub struct ThemeView {
    /// Theme name.
    pub name: String,
    /// Theme version.
    pub version: String,
}

/// giscus view used by the comments partial.
#[derive(Debug, Clone, Serialize)]
pub struct GiscusView {
    /// `owner/repo` of the discussions repository.
    pub repo: String,
    /// Repository GraphQL id.
    pub repo_id: String,
    /// Category name.
    pub category: Option<String>,
    /// Category id.
    pub category_id: Option<String>,
    /// Discussion mapping.
    pub mapping: String,
    /// Reactions metadata flag.
    pub reactions_enabled: String,
    /// Emit metadata flag.
    pub emit_metadata: String,
    /// Input position.
    pub input_position: String,
    /// giscus theme.
    pub theme: String,
    /// UI language.
    pub lang: String,
    /// Lazy loading flag.
    pub lazy_loading: bool,
}

impl From<&GiscusConfig> for GiscusView {
    fn from(giscus: &GiscusConfig) -> Self {
        Self {
            repo: giscus.repo.clone(),
            repo_id: giscus.repo_id.clone(),
            category: giscus.category.clone(),
            category_id: giscus.category_id.clone(),
            mapping: giscus.mapping.clone(),
            reactions_enabled: giscus.reactions_enabled.clone(),
            emit_metadata: giscus.emit_metadata.clone(),
            input_position: giscus.input_position.clone(),
            theme: giscus.theme.clone(),
            lang: giscus.lang.clone(),
            lazy_loading: giscus.lazy_loading,
        }
    }
}

/// Comment configuration exposed to templates.
#[derive(Debug, Clone, Serialize)]
pub struct CommentsView {
    /// giscus configuration, if enabled.
    pub giscus: Option<GiscusView>,
}

/// The page currently being rendered.
#[derive(Debug, Clone, Serialize)]
pub struct PageView {
    /// `article`, `index` or `profile`.
    pub kind: String,
    /// Full text of the HTML `<title>` element.
    pub title_string: String,
    /// Page title heading.
    pub title: String,
    /// URL path of the page from the site root.
    pub url_path: String,
    /// Article date (`YYYY.MM.DD`).
    pub date: Option<String>,
    /// Article author.
    pub author: Option<String>,
    /// GitHub profile URL for GitHub-handle authors.
    pub author_url: Option<String>,
    /// Article tags.
    pub tags: Vec<String>,
    /// Whether the article is hidden.
    pub hidden: bool,
    /// Rendered article body HTML (empty for index/profile pages).
    pub html: String,
}

impl PageView {
    /// Build the page view for an article.
    pub fn article(article: &Article, site_name: &str) -> Self {
        Self {
            kind: "article".to_string(),
            title_string: format!("{} | {site_name}", article.summary.title),
            title: article.summary.title.clone(),
            url_path: article.summary.url_path.clone(),
            date: article.summary.date.clone(),
            author: Some(article.summary.author.clone()),
            author_url: Some(article.summary.author_url.clone()),
            tags: article.summary.tags.clone(),
            hidden: article.hidden,
            html: article.html.clone(),
        }
    }

    /// Build the page view for the index page.
    pub fn index(site_name: &str) -> Self {
        Self {
            kind: "index".to_string(),
            title_string: site_name.to_string(),
            title: site_name.to_string(),
            url_path: "/".to_string(),
            date: None,
            author: None,
            author_url: None,
            tags: Vec::new(),
            hidden: false,
            html: String::new(),
        }
    }

    /// Build the page view for the profile README.
    pub fn profile(site_name: &str) -> Self {
        Self {
            kind: "profile".to_string(),
            title_string: site_name.to_string(),
            title: site_name.to_string(),
            url_path: "/".to_string(),
            date: None,
            author: None,
            author_url: None,
            tags: Vec::new(),
            hidden: false,
            html: String::new(),
        }
    }
}

/// Full context handed to article/index templates.
#[derive(Debug, Clone, Serialize)]
pub struct RenderContext {
    /// Generator information.
    pub blogen: BlogenInfo,
    /// Site configuration.
    pub site: SiteView,
    /// Theme information.
    pub theme: ThemeView,
    /// Comment configuration (null when no comments are configured).
    pub comments: Option<CommentsView>,
    /// Current page.
    pub page: PageView,
    /// Article summaries (index/profile pages).
    pub articles: Vec<ArticleSummary>,
}

impl RenderContext {
    /// Build a context.
    pub fn new(
        config: &Config,
        theme: &Theme,
        generated_at: &str,
        page: PageView,
        articles: Vec<ArticleSummary>,
    ) -> Self {
        Self {
            blogen: BlogenInfo {
                version: env!("CARGO_PKG_VERSION").to_string(),
                generated_at: generated_at.to_string(),
            },
            site: SiteView::from_config(config),
            theme: ThemeView {
                name: theme.manifest().name.clone(),
                version: theme.manifest().version.clone(),
            },
            comments: config.comments.as_ref().map(|comments| CommentsView {
                giscus: comments.giscus.as_ref().map(GiscusView::from),
            }),
            page,
            articles,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{SiteConfig, ThemeConfig};
    use std::path::PathBuf;

    fn engine(theme: Theme) -> TemplateEngine {
        TemplateEngine::new(theme).unwrap()
    }

    fn default_theme() -> Theme {
        Theme::load_bundled("default").unwrap()
    }

    fn config() -> Config {
        let mut config = Config {
            site: SiteConfig {
                name: "Test <Site>".into(),
                url: "https://example.com".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        config.theme = ThemeConfig {
            name: "default".into(),
            path: None,
            assets: "dist".into(),
        };
        config
    }

    #[test]
    fn renders_base_layout_with_escaping() {
        let config = config();
        let theme = default_theme();
        let engine = engine(theme.clone());
        let context = RenderContext::new(
            &config,
            &theme,
            "2026-01-01T00:00:00Z",
            PageView::index("Test <Site>"),
            vec![],
        );
        let html = engine.render("index.html", &context).unwrap();
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("Test &lt;Site&gt;"));
        assert!(html.contains("/dist/main.css"));
    }

    #[test]
    fn renders_article_with_unescaped_body() {
        let config = config();
        let theme = default_theme();
        let engine = engine(theme.clone());
        let article = Article {
            summary: ArticleSummary {
                title: "Title".into(),
                date: Some("2024.01.01".into()),
                author: "SF-Zhou".into(),
                tags: vec!["Rust".into()],
                filename: "post".into(),
                author_url: "https://github.com/SF-Zhou".into(),
                url_path: "/post.html".into(),
            },
            dir: String::new(),
            source: PathBuf::from("post.md"),
            hidden: false,
            markdown: String::new(),
            html: "<p>body &amp; content</p>".into(),
            mermaid_diagrams: 0,
        };
        let context = RenderContext::new(
            &config,
            &theme,
            "2026-01-01T00:00:00Z",
            PageView::article(&article, &config.site.name),
            vec![],
        );
        let html = engine
            .render(theme.article_template_name(), &context)
            .unwrap();
        assert!(html.contains("<p>body &amp; content</p>"));
        assert!(html.contains("Title | Test &lt;Site&gt;"));
        assert!(!html.contains("{{"));
    }

    #[test]
    fn strict_undefined_behavior() {
        let theme = default_theme();
        let engine = engine(theme.clone());
        // `missing_field` is not part of the context.
        let context = serde_json::json!({ "site": { "name": "x" } });
        let name = theme.index_template_name().to_string();
        let result = engine.render(&name, &context);
        assert!(result.is_err());
    }
}
