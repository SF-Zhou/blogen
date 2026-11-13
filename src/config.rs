//! TOML configuration: data model, loading, validation and path resolution.
//!
//! See `docs/config-reference.md` for the user-facing documentation.

use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use crate::error::{Error, IoContext, Result};

/// Name of the default configuration file used when `--config` is not given.
pub const DEFAULT_CONFIG_FILE: &str = "blogen.toml";

/// Top level configuration, mirroring `blogen.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Site identity and metadata (required).
    pub site: SiteConfig,
    /// Input/output paths and build behaviour.
    #[serde(default)]
    pub build: BuildConfig,
    /// Theme selection.
    #[serde(default)]
    pub theme: ThemeConfig,
    /// Markdown feature switches.
    #[serde(default)]
    pub markdown: MarkdownConfig,
    /// Optional profile README generation.
    #[serde(default)]
    pub profile: Option<ProfileConfig>,
    /// Optional comment system integration.
    #[serde(default)]
    pub comments: Option<CommentsConfig>,
    /// RSS feed generation.
    #[serde(default)]
    pub rss: RssConfig,
    /// Settings for `blogen serve`.
    #[serde(default)]
    pub serve: ServeConfig,
}

/// `[site]` – everything user/site specific.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SiteConfig {
    /// Site name, used in titles and feeds.
    pub name: String,
    /// Canonical site URL without trailing slash, e.g. `https://example.com`.
    pub url: String,
    /// Short description used by the RSS feed.
    #[serde(default)]
    pub description: Option<String>,
    /// BCP 47 language tag, e.g. `zh-CN`.
    #[serde(default)]
    pub language: Option<String>,
    /// Default author for articles without an author in their filename.
    #[serde(default)]
    pub default_author: Option<String>,
    /// Name used in the footer copyright.
    #[serde(default)]
    pub web_master: Option<String>,
    /// Google Analytics measurement id; the tracking snippet is omitted when empty.
    #[serde(default)]
    pub google_analytics_id: Option<String>,
    /// URL path prefix the site is served from. Defaults to `/`.
    #[serde(default = "default_base_path")]
    pub base_path: String,
}

/// `[build]` – where content comes from and goes to.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildConfig {
    /// Markdown content directory.
    #[serde(default = "default_posts")]
    pub posts: PathBuf,
    /// Generated site directory.
    #[serde(default = "default_output")]
    pub output: PathBuf,
    /// Markdown file extension (without dot).
    #[serde(default = "default_article_format")]
    pub article_format: String,
    /// Image directory inside the posts directory; copied to the output root.
    #[serde(default = "default_images")]
    pub images: String,
    /// Download remote (`http(s)`) images into the images directory.
    #[serde(default = "default_true")]
    pub download_remote_images: bool,
    /// Remove previously generated files that are no longer produced.
    #[serde(default = "default_true")]
    pub clean: bool,
    /// Worker threads; 0 means "detect from the machine".
    #[serde(default)]
    pub parallel: usize,
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            posts: default_posts(),
            output: default_output(),
            article_format: default_article_format(),
            images: default_images(),
            download_remote_images: true,
            clean: true,
            parallel: 0,
        }
    }
}

/// `[theme]` – which theme to use and where its assets go.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeConfig {
    /// Bundled theme name. Ignored when `path` is set.
    #[serde(default = "default_theme")]
    pub name: String,
    /// External theme directory (takes precedence over `name`).
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// Output sub-directory that receives the theme `assets/` directory.
    #[serde(default = "default_assets")]
    pub assets: String,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            name: default_theme(),
            path: None,
            assets: default_assets(),
        }
    }
}

/// `[markdown]` – markdown pipeline feature switches.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkdownConfig {
    /// Replace a line that contains only `[TOC]` with a table of contents.
    #[serde(default = "default_true")]
    pub toc: bool,
    /// Render `$...$` / `$$...$$` math with KaTeX.
    #[serde(default = "default_true")]
    pub math: bool,
    /// Highlight fenced code blocks with Prism.
    #[serde(default = "default_true")]
    pub highlight: bool,
    /// Diagram rendering mode: `static` (rendered to SVG at build time) or
    /// `none` (leave the fenced block as a code block). `placeholder` exists
    /// only for the markdown parity test harness.
    #[serde(default)]
    pub mermaid: MermaidMode,
}

impl Default for MarkdownConfig {
    fn default() -> Self {
        Self {
            toc: true,
            math: true,
            highlight: true,
            mermaid: MermaidMode::Static,
        }
    }
}

/// How Mermaid fenced blocks are handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MermaidMode {
    /// Render each diagram to static inline SVG at build time.
    #[default]
    Static,
    /// Emit deterministic placeholders (used by the markdown parity test
    /// harness to compare against the reference renderer).
    Placeholder,
    /// Do not special-case mermaid blocks (they become ordinary code blocks).
    None,
}

/// `[profile]` – optional profile README generation.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileConfig {
    /// Directory that receives `README.md`.
    #[serde(default = "default_profile_path")]
    pub path: PathBuf,
    /// Number of recent articles to list.
    #[serde(default = "default_profile_limit")]
    pub limit: usize,
}

impl Default for ProfileConfig {
    fn default() -> Self {
        Self {
            path: default_profile_path(),
            limit: default_profile_limit(),
        }
    }
}

/// `[comments]` – optional comment system configuration.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommentsConfig {
    /// giscus integration.
    #[serde(default)]
    pub giscus: Option<GiscusConfig>,
}

/// `[comments.giscus]` – giscus options emitted into the page.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GiscusConfig {
    /// `owner/repo` of the discussion repository.
    pub repo: String,
    /// GraphQL repository id starting with `R_`/`MDEw...`.
    pub repo_id: String,
    /// Discussion category name.
    #[serde(default)]
    pub category: Option<String>,
    /// Discussion category id.
    #[serde(default)]
    pub category_id: Option<String>,
    /// Discussion mapping, e.g. `title` or `pathname`.
    #[serde(default = "default_giscus_mapping")]
    pub mapping: String,
    /// Emit discussion metadata.
    #[serde(default = "default_zero")]
    pub reactions_enabled: String,
    /// Emit discussion metadata.
    #[serde(default = "default_zero")]
    pub emit_metadata: String,
    /// Comment input position.
    #[serde(default = "default_input_position")]
    pub input_position: String,
    /// giscus theme name.
    #[serde(default = "default_giscus_theme")]
    pub theme: String,
    /// UI language.
    #[serde(default = "default_giscus_lang")]
    pub lang: String,
    /// Load the script lazily.
    #[serde(default = "default_true")]
    pub lazy_loading: bool,
}

/// `[rss]` – feed generation.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RssConfig {
    /// Generate the feed.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Feed file name inside the output directory.
    #[serde(default = "default_rss_file")]
    pub file: String,
    /// Maximum number of items.
    #[serde(default = "default_rss_limit")]
    pub limit: usize,
}

impl Default for RssConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            file: default_rss_file(),
            limit: default_rss_limit(),
        }
    }
}

/// `[serve]` – local preview server.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServeConfig {
    /// Interface to bind.
    #[serde(default = "default_host")]
    pub host: String,
    /// TCP port to bind.
    #[serde(default = "default_port")]
    pub port: u16,
}

impl Default for ServeConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
        }
    }
}

fn default_base_path() -> String {
    "/".to_string()
}
fn default_posts() -> PathBuf {
    PathBuf::from("posts")
}
fn default_output() -> PathBuf {
    PathBuf::from("public")
}
fn default_article_format() -> String {
    "md".to_string()
}
fn default_images() -> String {
    "images".to_string()
}
fn default_true() -> bool {
    true
}
fn default_theme() -> String {
    "default".to_string()
}
fn default_assets() -> String {
    "dist".to_string()
}
fn default_profile_path() -> PathBuf {
    PathBuf::from("profile")
}
fn default_profile_limit() -> usize {
    5
}
fn default_giscus_mapping() -> String {
    "title".to_string()
}
fn default_zero() -> String {
    "0".to_string()
}
fn default_input_position() -> String {
    "bottom".to_string()
}
fn default_giscus_theme() -> String {
    "preferred_color_scheme".to_string()
}
fn default_giscus_lang() -> String {
    "en".to_string()
}
fn default_rss_file() -> String {
    "rss.xml".to_string()
}
fn default_rss_limit() -> usize {
    20
}
fn default_host() -> String {
    "127.0.0.1".to_string()
}
fn default_port() -> u16 {
    9000
}

/// A loaded configuration together with the project root it belongs to.
#[derive(Debug, Clone)]
pub struct LoadedConfig {
    /// Directory that relative config paths are resolved against (the
    /// directory that contains the config file).
    pub root: PathBuf,
    /// Path of the configuration file.
    pub path: PathBuf,
    /// Parsed and validated configuration.
    pub config: Config,
}

impl LoadedConfig {
    /// Load the configuration from an explicit path.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let source = std::fs::read_to_string(path)
            .with_context(|| format!("reading configuration `{}`", path.display()))?;
        let root = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        Self::from_str(&source, path, &root)
    }

    /// Find and load the configuration file.
    ///
    /// When `explicit` is set, that file must exist. Otherwise the current
    /// directory is searched for `blogen.toml`; if it is not found the search
    /// continues in parent directories (so commands work from a sub-directory
    /// of the project).
    pub fn discover(explicit: Option<&Path>) -> Result<Self> {
        if let Some(path) = explicit {
            return Self::from_file(path);
        }
        let mut dir =
            std::env::current_dir().context("determining the current working directory")?;
        loop {
            let candidate = dir.join(DEFAULT_CONFIG_FILE);
            if candidate.is_file() {
                return Self::from_file(candidate);
            }
            if !dir.pop() {
                return Err(Error::Cli(format!(
                    "no `{DEFAULT_CONFIG_FILE}` found in the current directory or its parents; \
                     run `blogen init` to create a new site or pass `--config <path>`"
                )));
            }
        }
    }

    /// Parse and validate a configuration from a string.
    pub fn from_str(source: &str, path: &Path, root: &Path) -> Result<Self> {
        let config: Config = toml::from_str(source).map_err(|err| Error::config(path, err))?;
        let loaded = Self {
            root: root.to_path_buf(),
            path: path.to_path_buf(),
            config,
        };
        loaded.config.validate()?;
        Ok(loaded)
    }

    /// Resolve a configured path relative to the project root.
    pub fn resolve(&self, path: impl AsRef<Path>) -> PathBuf {
        let path = path.as_ref();
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            normalize_path(&self.root.join(path))
        }
    }

    /// Markdown content directory.
    pub fn posts_dir(&self) -> PathBuf {
        self.resolve(&self.config.build.posts)
    }

    /// Generated site directory.
    pub fn output_dir(&self) -> PathBuf {
        self.resolve(&self.config.build.output)
    }

    /// Profile README directory, when profiles are enabled.
    pub fn profile_dir(&self) -> Option<PathBuf> {
        self.config.profile.as_ref().map(|p| self.resolve(&p.path))
    }

    /// Directory that receives theme assets (`<output>/<theme.assets>`).
    pub fn assets_output_dir(&self) -> PathBuf {
        self.output_dir().join(&self.config.theme.assets)
    }

    /// Directory of downloaded/copied post images (`<posts>/<build.images>`).
    pub fn posts_images_dir(&self) -> PathBuf {
        self.posts_dir().join(&self.config.build.images)
    }

    /// Set the output directory (used by the `--output` CLI flag).
    pub fn override_output(&mut self, output: &Path) {
        self.config.build.output = output.to_path_buf();
    }
}

impl Config {
    /// Validate semantic constraints that `serde` cannot express.
    pub fn validate(&self) -> Result<()> {
        if self.site.name.trim().is_empty() {
            return Err(Error::validation("`site.name` must not be empty"));
        }
        if self.site.url.trim().is_empty() {
            return Err(Error::validation("`site.url` must not be empty"));
        }
        let url = self.site.url.trim();
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(Error::validation(format!(
                "`site.url` must start with `http://` or `https://` (got `{url}`)"
            )));
        }
        if url.ends_with('/') {
            return Err(Error::validation(
                "`site.url` must not end with `/`; use `site.base_path` for sub-path hosting",
            ));
        }
        if self.site.base_path.trim().is_empty() {
            return Err(Error::validation("`site.base_path` must not be empty"));
        }

        if self.build.article_format.trim().is_empty() {
            return Err(Error::validation(
                "`build.article_format` must not be empty",
            ));
        }
        if self.build.article_format.contains('/') || self.build.article_format.contains('\\') {
            return Err(Error::validation(
                "`build.article_format` must be a file extension, not a path",
            ));
        }
        if self.build.images.trim().is_empty() {
            return Err(Error::validation("`build.images` must not be empty"));
        }
        let output = self.build.output.to_string_lossy();
        if output.trim().is_empty() || output == "." || output == ".." {
            return Err(Error::validation(
                "`build.output` must be a dedicated directory (not `.`, `..` or empty)",
            ));
        }
        if has_parent_dir(&self.build.output) {
            return Err(Error::validation(
                "`build.output` must not contain `..` path segments",
            ));
        }
        if self.build.posts == self.build.output {
            return Err(Error::validation(
                "`build.posts` and `build.output` must be different directories",
            ));
        }

        if self.theme.assets.trim().is_empty() {
            return Err(Error::validation("`theme.assets` must not be empty"));
        }
        if has_parent_dir(Path::new(&self.theme.assets))
            || Path::new(&self.theme.assets).is_absolute()
        {
            return Err(Error::validation(
                "`theme.assets` must be a relative sub-directory name",
            ));
        }
        if self.theme.path.is_none() && self.theme.name.trim().is_empty() {
            return Err(Error::validation(
                "`theme.name` must not be empty when `theme.path` is not set",
            ));
        }

        if let Some(profile) = &self.profile {
            if profile.limit == 0 {
                return Err(Error::validation("`profile.limit` must be greater than 0"));
            }
            if profile.path.as_os_str().is_empty() {
                return Err(Error::validation("`profile.path` must not be empty"));
            }
        }

        if let Some(comments) = &self.comments {
            if let Some(giscus) = &comments.giscus {
                if !giscus.repo.contains('/') {
                    return Err(Error::validation(
                        "`comments.giscus.repo` must look like `owner/repo`",
                    ));
                }
                if giscus.repo_id.trim().is_empty() {
                    return Err(Error::validation(
                        "`comments.giscus.repo_id` must not be empty",
                    ));
                }
            }
        }

        if self.rss.limit == 0 {
            return Err(Error::validation("`rss.limit` must be greater than 0"));
        }
        if self.rss.file.trim().is_empty() || self.rss.file.contains('/') {
            return Err(Error::validation(
                "`rss.file` must be a file name inside the output directory",
            ));
        }
        Ok(())
    }

    /// Site description, falling back to the site name.
    pub fn site_description(&self) -> &str {
        self.site
            .description
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.site.name)
    }

    /// Site language.
    pub fn site_language(&self) -> &str {
        self.site
            .language
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("en")
    }

    /// Effective default author.
    pub fn default_author(&self) -> &str {
        self.site
            .default_author
            .as_deref()
            .filter(|s| !s.is_empty())
            .or_else(|| self.site.web_master.as_deref().filter(|s| !s.is_empty()))
            .unwrap_or(&self.site.name)
    }

    /// Effective web master name used in the footer.
    pub fn web_master(&self) -> &str {
        self.site
            .web_master
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.site.name)
    }

    /// Normalized base path: always starts and ends with `/`.
    pub fn base_path(&self) -> String {
        let mut base = self.site.base_path.trim().to_string();
        if !base.starts_with('/') {
            base.insert(0, '/');
        }
        if !base.ends_with('/') {
            base.push('/');
        }
        base
    }

    /// URL prefix of theme assets, e.g. `/dist` (no trailing slash).
    pub fn assets_url(&self) -> String {
        format!(
            "{}/{}",
            self.base_path().trim_end_matches('/'),
            self.theme.assets.trim_matches('/')
        )
    }

    /// URL prefix of copied post images, e.g. `/images` (no trailing slash).
    pub fn images_url(&self) -> String {
        format!(
            "{}/{}",
            self.base_path().trim_end_matches('/'),
            self.build.images.trim_matches('/')
        )
    }
}

/// Lexically normalize a path: remove `.` components and collapse duplicate
/// separators without touching the filesystem.
pub fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn has_parent_dir(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, Component::ParentDir))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal() -> &'static str {
        r#"
[site]
name = "Test"
url = "https://example.com"
"#
    }

    fn load(source: &str) -> Result<LoadedConfig> {
        LoadedConfig::from_str(source, Path::new("blogen.toml"), Path::new("/project"))
    }

    #[test]
    fn parses_minimal_config_with_defaults() {
        let loaded = load(minimal()).unwrap();
        assert_eq!(loaded.config.site.name, "Test");
        assert_eq!(loaded.config.build.posts, PathBuf::from("posts"));
        assert_eq!(loaded.config.build.output, PathBuf::from("public"));
        assert_eq!(loaded.config.theme.name, "default");
        assert_eq!(loaded.config.assets_url(), "/dist");
        assert_eq!(loaded.config.images_url(), "/images");
        assert!(loaded.config.rss.enabled);
        assert_eq!(loaded.config.default_author(), "Test");
    }

    #[test]
    fn parses_full_config() {
        let source = r#"
[site]
name = "Blog"
url = "https://sf-zhou.github.io"
description = "desc"
language = "zh-CN"
default_author = "SF-Zhou"
web_master = "SF-Zhou"
google_analytics_id = "G-X"
base_path = "/blog/"

[build]
posts = "content"
output = "out"
article_format = "markdown"
images = "img"
clean = false

[theme]
name = "custom"
path = "themes/custom"
assets = "static"

[profile]
path = "profile"
limit = 10

[comments.giscus]
repo = "owner/repo"
repo_id = "R_123"
"#;
        let loaded = load(source).expect("full config should parse");
        assert_eq!(loaded.config.site.base_path, "/blog/");
        assert_eq!(loaded.config.assets_url(), "/blog/static");
        assert_eq!(loaded.config.images_url(), "/blog/img");
        assert_eq!(loaded.config.build.posts, PathBuf::from("content"));
        assert!(!loaded.config.build.clean);
        assert_eq!(loaded.config.theme.assets, "static");
        assert_eq!(loaded.config.profile.as_ref().unwrap().limit, 10);
        assert_eq!(
            loaded
                .config
                .comments
                .as_ref()
                .unwrap()
                .giscus
                .as_ref()
                .unwrap()
                .repo,
            "owner/repo"
        );
    }

    #[test]
    fn rejects_unknown_top_level_tables() {
        let source = format!("{}\n[typo]\nx = 1\n", minimal());
        let err = load(&source).unwrap_err();
        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn rejects_unknown_site_keys() {
        let source = r#"
[site]
name = "Test"
url = "https://example.com"
typo = true
"#;
        assert!(load(source).is_err());
    }

    #[test]
    fn validates_site_url() {
        let source = minimal().replace("https://example.com", "example.com");
        let err = load(&source).unwrap_err();
        assert!(matches!(err, Error::ConfigValidation(_)));
        assert!(err.to_string().contains("site.url"));
    }

    #[test]
    fn rejects_trailing_slash_in_url() {
        let source = minimal().replace("https://example.com", "https://example.com/");
        assert!(load(&source).is_err());
    }

    #[test]
    fn base_path_is_normalized() {
        let mut loaded = load(minimal()).unwrap();
        loaded.config.site.base_path = "blog".into();
        assert_eq!(loaded.config.base_path(), "/blog/");
        assert_eq!(loaded.config.assets_url(), "/blog/dist");
    }

    #[test]
    fn resolves_paths_relative_to_config() {
        let loaded = load(minimal()).unwrap();
        assert_eq!(loaded.posts_dir(), PathBuf::from("/project/posts"));
        assert_eq!(loaded.output_dir(), PathBuf::from("/project/public"));
        assert_eq!(
            loaded.posts_images_dir(),
            PathBuf::from("/project/posts/images")
        );
    }

    #[test]
    fn rejects_dot_output() {
        let source = minimal().replace("[site]", "[build]\noutput = \".\"\n\n[site]");
        assert!(load(&source).is_err());
    }

    #[test]
    fn giscus_defaults() {
        let source = format!(
            "{}\n[comments.giscus]\nrepo = \"a/b\"\nrepo_id = \"R_1\"\n",
            minimal()
        );
        let loaded = load(&source).unwrap();
        let giscus = loaded.config.comments.unwrap().giscus.unwrap();
        assert_eq!(giscus.mapping, "title");
        assert_eq!(giscus.reactions_enabled, "0");
        assert!(giscus.lazy_loading);
    }

    #[test]
    fn resolves_parent_dirs_lexically() {
        assert_eq!(
            normalize_path(Path::new("a/./b/../c")),
            PathBuf::from("a/c")
        );
    }
}
