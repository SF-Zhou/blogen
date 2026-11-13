//! Theme loading: bundled (compiled-in) themes and external theme directories.
//!
//! A theme is a directory with a `theme.toml` manifest, a `templates/`
//! directory and an `assets/` directory. The bundled themes are embedded in the
//! binary with the `include_dir` crate, so `blogen init` / `blogen build` work without a
//! source checkout.

use std::path::{Component, Path, PathBuf};

use include_dir::{include_dir, Dir};
use serde::{Deserialize, Serialize};

use crate::config::ThemeConfig;
use crate::error::{Error, IoContext, Result};

/// All themes embedded into the binary.
static EMBEDDED_THEMES: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/themes");

/// Name of the theme manifest file.
pub const THEME_MANIFEST_FILE: &str = "theme.toml";

/// Parsed `theme.toml`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeManifest {
    /// Theme name (must match the directory name for bundled themes).
    pub name: String,
    /// Theme version (semver).
    pub version: String,
    /// One line description.
    #[serde(default)]
    pub description: String,
    /// SPDX license identifier.
    #[serde(default)]
    pub license: String,
    /// Minimum blogen version required by the theme.
    #[serde(default)]
    pub min_blogen_version: Option<String>,
    /// Template file map.
    #[serde(default)]
    pub templates: TemplateManifest,
}

/// Template file names declared by a theme (`[templates]` in `theme.toml`).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateManifest {
    /// Article page template.
    #[serde(default = "default_article_template")]
    pub article: String,
    /// Index (home page) template.
    #[serde(default = "default_index_template")]
    pub index: String,
    /// Optional profile README template (`profile.md`). When absent, profile
    /// generation is unavailable for the theme.
    #[serde(default)]
    pub profile: Option<String>,
}

impl Default for TemplateManifest {
    fn default() -> Self {
        Self {
            article: default_article_template(),
            index: default_index_template(),
            profile: None,
        }
    }
}

fn default_article_template() -> String {
    "article.html".to_string()
}
fn default_index_template() -> String {
    "index.html".to_string()
}

/// Where a theme's files come from.
#[derive(Debug, Clone)]
enum ThemeOrigin {
    /// Bundled theme, by name.
    Embedded(String),
    /// External theme directory.
    External(PathBuf),
}

/// A loaded theme.
#[derive(Debug, Clone)]
pub struct Theme {
    manifest: ThemeManifest,
    origin: ThemeOrigin,
}

/// One asset file of a theme, ready to be copied into the output directory.
#[derive(Debug, Clone)]
pub struct ThemeAsset {
    /// Path relative to the theme's `assets/` directory (always `/` separated).
    pub relative_path: String,
    /// File contents.
    pub contents: Vec<u8>,
}

impl Theme {
    /// Load a theme according to `[theme]` configuration.
    pub fn load(config: &ThemeConfig, project_root: &Path) -> Result<Self> {
        match &config.path {
            Some(path) => {
                let root = if path.is_absolute() {
                    path.clone()
                } else {
                    project_root.join(path)
                };
                Self::load_external(&root)
            }
            None => Self::load_bundled(&config.name),
        }
    }

    /// Load a bundled (embedded) theme by name.
    pub fn load_bundled(name: &str) -> Result<Self> {
        if embedded_theme_root(name).is_none() {
            let available = Self::bundled_names().join(", ");
            return Err(Error::Theme(format!(
                "bundled theme `{name}` not found (available: {available})"
            )));
        }
        let manifest_file = EMBEDDED_THEMES
            .get_file(format!("{name}/{THEME_MANIFEST_FILE}"))
            .ok_or_else(|| {
                Error::Theme(format!(
                    "bundled theme `{name}` has no `{THEME_MANIFEST_FILE}`"
                ))
            })?;
        let source = manifest_file.contents_utf8().ok_or_else(|| {
            Error::Theme(format!("`{name}/{THEME_MANIFEST_FILE}` is not valid UTF-8"))
        })?;
        let manifest = parse_manifest(source, Path::new(name).join(THEME_MANIFEST_FILE))?;
        Self::validate_manifest(&manifest)?;
        Ok(Self {
            manifest,
            origin: ThemeOrigin::Embedded(name.to_string()),
        })
    }

    /// Load an external theme from a directory.
    pub fn load_external(root: &Path) -> Result<Self> {
        let manifest_path = root.join(THEME_MANIFEST_FILE);
        let source = std::fs::read_to_string(&manifest_path)
            .with_context(|| format!("reading theme manifest `{}`", manifest_path.display()))?;
        let manifest = parse_manifest(&source, manifest_path)?;
        Self::validate_manifest(&manifest)?;
        let theme = Self {
            manifest,
            origin: ThemeOrigin::External(root.to_path_buf()),
        };
        // Fail early when required templates are missing.
        theme.read_template(theme.article_template_name())?;
        theme.read_template(theme.index_template_name())?;
        Ok(theme)
    }

    fn validate_manifest(manifest: &ThemeManifest) -> Result<()> {
        if manifest.name.trim().is_empty() {
            return Err(Error::Theme("theme manifest has an empty `name`".into()));
        }
        if manifest.version.trim().is_empty() {
            return Err(Error::Theme("theme manifest has an empty `version`".into()));
        }
        if let Some(required) = &manifest.min_blogen_version {
            let required = required.trim();
            if !required.is_empty() {
                let current = env!("CARGO_PKG_VERSION");
                if version_lt(current, required) {
                    return Err(Error::Theme(format!(
                        "theme `{}` requires blogen >= {required}, but this is blogen {current}",
                        manifest.name
                    )));
                }
            }
        }
        Ok(())
    }

    /// Theme manifest.
    pub fn manifest(&self) -> &ThemeManifest {
        &self.manifest
    }

    /// Article page template name.
    pub fn article_template_name(&self) -> &str {
        &self.manifest.templates.article
    }

    /// Index page template name.
    pub fn index_template_name(&self) -> &str {
        &self.manifest.templates.index
    }

    /// Optional profile template name.
    pub fn profile_template_name(&self) -> Option<&str> {
        self.manifest.templates.profile.as_deref()
    }

    /// Read a template file as UTF-8.
    pub fn read_template(&self, name: &str) -> Result<String> {
        validate_relative_template_path(name)?;
        let bytes = self.read_file(&format!("templates/{name}")).map_err(|_| {
            Error::Theme(format!(
                "theme `{}` has no template `{name}`",
                self.manifest.name
            ))
        })?;
        String::from_utf8(bytes)
            .map_err(|_| Error::Theme(format!("template `{name}` is not valid UTF-8")))
    }

    /// Read a template file, returning `None` when it does not exist.
    ///
    /// Loader errors are kept distinct from "template not found" so that
    /// minijinja can report an unknown template instead of a broken theme.
    pub fn read_template_opt(&self, name: &str) -> Result<Option<String>> {
        if validate_relative_template_path(name).is_err() {
            return Ok(None);
        }
        match self.read_template(name) {
            Ok(source) => Ok(Some(source)),
            Err(Error::Theme(message))
                if message.starts_with("theme `") && message.contains("has no template") =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    /// Read any file of the theme by a `/`-separated relative path.
    pub fn read_file(&self, relative_path: &str) -> Result<Vec<u8>> {
        validate_relative_path(relative_path).map_err(|message| {
            Error::Theme(format!("invalid theme path `{relative_path}`: {message}"))
        })?;
        match &self.origin {
            ThemeOrigin::Embedded(name) => {
                let full_path = format!("{name}/{relative_path}");
                let file = EMBEDDED_THEMES.get_file(&full_path).ok_or_else(|| {
                    Error::Theme(format!("bundled theme `{name}` has no `{relative_path}`"))
                })?;
                Ok(file.contents().to_vec())
            }
            ThemeOrigin::External(root) => {
                let path = root.join(relative_path);
                std::fs::read(&path)
                    .with_context(|| format!("reading theme file `{}`", path.display()))
            }
        }
    }

    /// All files under the theme's `assets/` directory.
    pub fn assets(&self) -> Result<Vec<ThemeAsset>> {
        let mut assets = Vec::new();
        match &self.origin {
            ThemeOrigin::Embedded(name) => {
                let root =
                    embedded_theme_root(name).expect("embedded theme was validated at load time");
                let assets_prefix = format!("{name}/assets");
                collect_embedded_assets(root, &assets_prefix, &mut assets);
            }
            ThemeOrigin::External(root) => {
                let assets_root = root.join("assets");
                if assets_root.is_dir() {
                    collect_fs_assets(&assets_root, &assets_root, &mut assets)?;
                }
            }
        }
        assets.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        Ok(assets)
    }

    /// Extract this theme into a directory, preserving the theme layout.
    pub fn extract(&self, destination: &Path) -> Result<()> {
        std::fs::create_dir_all(destination)
            .with_context(|| format!("creating theme directory `{}`", destination.display()))?;
        let files: Vec<String> = match &self.origin {
            ThemeOrigin::Embedded(name) => {
                let root =
                    embedded_theme_root(name).expect("embedded theme was validated at load time");
                let mut files = Vec::new();
                collect_dir_files(root, root.path(), &mut files);
                files
            }
            ThemeOrigin::External(root) => {
                let mut files = Vec::new();
                for entry in walkdir::WalkDir::new(root).sort_by_file_name() {
                    let entry = entry.map_err(|err| {
                        Error::io(
                            format!("walking theme `{}`", root.display()),
                            std::io::Error::other(err),
                        )
                    })?;
                    if entry.file_type().is_file() {
                        let relative = entry
                            .path()
                            .strip_prefix(root)
                            .expect("walkdir entry inside root");
                        files.push(relative.to_string_lossy().replace('\\', "/"));
                    }
                }
                files.sort();
                files
            }
        };
        for relative in files {
            let bytes = self.read_file(&relative)?;
            let target = destination.join(&relative);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("creating directory `{}`", parent.display()))?;
            }
            std::fs::write(&target, bytes)
                .with_context(|| format!("writing theme file `{}`", target.display()))?;
        }
        Ok(())
    }

    /// Names of all bundled themes, sorted.
    pub fn bundled_names() -> Vec<String> {
        let mut names: Vec<String> = EMBEDDED_THEMES
            .dirs()
            .filter(|dir| {
                let name = dir.path().to_string_lossy().replace('\\', "/");
                EMBEDDED_THEMES
                    .get_file(format!("{name}/{THEME_MANIFEST_FILE}"))
                    .is_some()
            })
            .map(|dir| {
                dir.path()
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
            })
            .collect();
        names.sort();
        names
    }

    /// Load the manifests of all bundled themes.
    pub fn bundled_manifests() -> Vec<ThemeManifest> {
        Self::bundled_names()
            .iter()
            .filter_map(|name| Self::load_bundled(name).ok().map(|theme| theme.manifest))
            .collect()
    }
}

fn parse_manifest(source: &str, path: PathBuf) -> Result<ThemeManifest> {
    toml::from_str(source).map_err(|err| Error::config(path, err))
}

/// Top-level embedded theme directory.
fn embedded_theme_root(name: &str) -> Option<&'static Dir<'static>> {
    EMBEDDED_THEMES
        .dirs()
        .find(|dir| dir.path() == Path::new(name))
}

/// Collect the files under `<theme>/assets`, relative to the assets directory.
fn collect_embedded_assets(dir: &Dir<'_>, assets_prefix: &str, out: &mut Vec<ThemeAsset>) {
    for file in dir.files() {
        let path = file.path().to_string_lossy().replace('\\', "/");
        if let Some(relative_path) = path
            .strip_prefix(assets_prefix)
            .and_then(|rest| rest.strip_prefix('/'))
        {
            out.push(ThemeAsset {
                relative_path: relative_path.to_string(),
                contents: file.contents().to_vec(),
            });
        }
    }
    for subdir in dir.dirs() {
        collect_embedded_assets(subdir, assets_prefix, out);
    }
}

fn collect_dir_files(dir: &Dir<'_>, root: &Path, out: &mut Vec<String>) {
    for file in dir.files() {
        let relative = file
            .path()
            .strip_prefix(root)
            .unwrap_or(file.path())
            .to_string_lossy()
            .replace('\\', "/");
        out.push(relative);
    }
    for subdir in dir.dirs() {
        collect_dir_files(subdir, root, out);
    }
}

fn collect_fs_assets(root: &Path, current: &Path, out: &mut Vec<ThemeAsset>) -> Result<()> {
    for entry in walkdir::WalkDir::new(current).sort_by_file_name() {
        let entry = entry.map_err(|err| {
            Error::io(
                format!("walking theme assets `{}`", root.display()),
                std::io::Error::other(err),
            )
        })?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(root)
            .expect("walkdir entry inside assets root")
            .to_string_lossy()
            .replace('\\', "/");
        let contents = std::fs::read(entry.path())
            .with_context(|| format!("reading theme asset `{}`", entry.path().display()))?;
        out.push(ThemeAsset {
            relative_path: relative,
            contents,
        });
    }
    Ok(())
}

/// Reject absolute paths and `..` components in theme-internal paths.
fn validate_relative_path(path: &str) -> std::result::Result<(), &'static str> {
    if path.is_empty() {
        return Err("path is empty");
    }
    let path = Path::new(path);
    if path.is_absolute() {
        return Err("absolute paths are not allowed");
    }
    for component in path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir => return Err("`..` is not allowed"),
            Component::RootDir | Component::Prefix(_) => {
                return Err("absolute paths are not allowed")
            }
        }
    }
    Ok(())
}

fn validate_relative_template_path(path: &str) -> Result<()> {
    validate_relative_path(path).map_err(|message| Error::Theme(message.to_string()))
}

/// Naive semver comparison good enough for the `min_blogen_version` check.
fn version_lt(current: &str, required: &str) -> bool {
    fn parse(version: &str) -> Vec<u64> {
        version
            .split(['.', '-', '+'])
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect()
    }
    let current = parse(current.trim_start_matches('v'));
    let required = parse(required.trim_start_matches('v'));
    for index in 0..current.len().max(required.len()) {
        let a = current.get(index).copied().unwrap_or(0);
        let b = required.get(index).copied().unwrap_or(0);
        if a != b {
            return a < b;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ThemeConfig;

    #[test]
    fn loads_bundled_default_theme() {
        // The bundles theme is part of the repository; this also guards against
        // accidentally breaking the embedded theme layout.
        let theme = Theme::load_bundled("default").unwrap();
        assert_eq!(theme.manifest().name, "default");
        assert!(theme.read_template("base.html").unwrap().contains("<html"));
        assert!(!theme.assets().unwrap().is_empty());
    }

    #[test]
    fn rejects_unknown_bundled_theme() {
        let err = Theme::load_bundled("does-not-exist").unwrap_err();
        assert!(err.to_string().contains("not found"));
        assert!(err.to_string().contains("default"));
    }

    #[test]
    fn rejects_path_traversal() {
        let theme = Theme::load_bundled("default").unwrap();
        assert!(theme.read_file("../theme.toml").is_err());
        assert!(theme.read_file("/etc/passwd").is_err());
    }

    #[test]
    fn external_theme_roundtrip() {
        let temp = tempfile::tempdir().unwrap();
        let bundled = Theme::load_bundled("default").unwrap();
        let extracted = temp.path().join("my-theme");
        bundled.extract(&extracted).unwrap();
        let external = Theme::load_external(&extracted).unwrap();
        assert_eq!(external.manifest().name, "default");
        assert!(!external.assets().unwrap().is_empty());
    }

    #[test]
    fn load_uses_path_when_configured() {
        let temp = tempfile::tempdir().unwrap();
        Theme::load_bundled("default")
            .unwrap()
            .extract(&temp.path().join("custom"))
            .unwrap();
        let config = ThemeConfig {
            name: "ignored".into(),
            path: Some(PathBuf::from("custom")),
            assets: "dist".into(),
        };
        let theme = Theme::load(&config, temp.path()).unwrap();
        assert_eq!(theme.manifest().name, "default");
    }

    #[test]
    fn semver_comparison() {
        assert!(version_lt("0.1.0", "0.2.0"));
        assert!(version_lt("1.9.9", "2.0.0"));
        assert!(!version_lt("1.2.3", "1.2.3"));
        assert!(!version_lt("1.2.4", "1.2.3"));
    }
}
