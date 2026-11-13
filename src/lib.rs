//! blogen – a fast, themeable static blog generator.
//!
//! The crate is both a library and the implementation of the `blogen` binary.
//! See `docs/architecture.md` for the design and `docs/config-reference.md` for
//! the configuration format.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![warn(clippy::all, clippy::pedantic)]
// The following pedantic lints are intentionally not enforced: the public API
// is documented at module/type level and adding `# Errors` / `# Panics`
// sections to every function would not improve the documentation.
#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    clippy::module_name_repetitions,
    clippy::doc_markdown,
    clippy::too_many_lines
)]

pub mod article;
pub mod assets;
pub mod cli;
pub mod config;
pub mod error;
pub mod feed;
mod hashing;
pub mod init;
pub mod markdown;
pub mod model;
pub mod output;
pub mod server;
pub mod site;
pub mod templates;
pub mod theme;
