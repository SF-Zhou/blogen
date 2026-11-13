//! Error handling for blogen.
//!
//! All user-facing failures are represented by [`Error`]. The CLI prints the
//! chain of causes and exits with a non-zero status; library users can inspect
//! the variants.

use std::fmt;
use std::path::{Path, PathBuf};

/// Convenient result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by blogen.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The configuration file could not be read or parsed.
    #[error("failed to load configuration from `{path}`: {message}")]
    Config {
        /// Configuration file that failed to load.
        path: PathBuf,
        /// Underlying parse or read error.
        message: String,
    },

    /// The configuration is syntactically valid but semantically wrong.
    #[error("invalid configuration: {0}")]
    ConfigValidation(String),

    /// A filesystem operation failed.
    #[error("{context}: {source}")]
    Io {
        /// What blogen was trying to do.
        context: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// The theme directory or one of its files is invalid.
    #[error("theme error: {0}")]
    Theme(String),

    /// A template could not be rendered.
    #[error("template error: {0}")]
    Template(String),

    /// Markdown rendering failed (for example a broken KaTeX expression that
    /// even the fallback path cannot handle).
    #[error("markdown rendering error: {0}")]
    Markdown(String),

    /// A remote image referenced from markdown could not be fetched.
    #[error("failed to download image `{url}`: {message}")]
    RemoteImage {
        /// Image URL.
        url: String,
        /// Error description.
        message: String,
    },

    /// A path referenced by blogen escaped its allowed root or is otherwise
    /// unsafe to write to.
    #[error("unsafe path `{path}`: {message}")]
    UnsafePath {
        /// Path that was rejected.
        path: PathBuf,
        /// Why the path is unsafe.
        message: String,
    },

    /// The command line was used incorrectly.
    #[error("{0}")]
    Cli(String),
}

impl Error {
    /// Build an [`Error::Io`] with a human readable context message.
    pub fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Error::Io {
            context: context.into(),
            source,
        }
    }

    /// Build a configuration parse error.
    pub fn config(path: impl AsRef<Path>, message: impl fmt::Display) -> Self {
        Error::Config {
            path: path.as_ref().to_path_buf(),
            message: message.to_string(),
        }
    }

    /// Build a configuration validation error.
    pub fn validation(message: impl fmt::Display) -> Self {
        Error::ConfigValidation(message.to_string())
    }
}

/// Extension trait adding context to [`std::io::Result`].
pub trait IoContext<T> {
    /// Wrap the I/O error with a context message.
    fn context(self, context: impl Into<String>) -> Result<T>;
    /// Like [`IoContext::context`], but takes a closure so the message is only
    /// built on the error path.
    fn with_context<F, S>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> S,
        S: Into<String>;
}

impl<T> IoContext<T> for std::io::Result<T> {
    fn context(self, context: impl Into<String>) -> Result<T> {
        self.map_err(|source| Error::io(context.into(), source))
    }

    fn with_context<F, S>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> S,
        S: Into<String>,
    {
        self.map_err(|source| Error::io(f().into(), source))
    }
}

/// Report an error together with all of its causes.
pub fn format_error_chain(error: &dyn std::error::Error) -> String {
    let mut out = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        out.push_str("\n  caused by: ");
        out.push_str(&cause.to_string());
        source = cause.source();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_context_wraps_message() {
        let err = std::fs::read("/definitely/not/a/file")
            .context("reading test file")
            .unwrap_err();
        let message = format_error_chain(&err);
        assert!(message.contains("reading test file"));
    }

    #[test]
    fn format_chain_includes_sources() {
        let inner = std::io::Error::new(std::io::ErrorKind::NotFound, "inner");
        let err = Error::io("outer", inner);
        let formatted = format_error_chain(&err);
        assert!(formatted.contains("outer"));
        assert!(formatted.contains("inner"));
    }
}
