# Changelog

All notable changes to this project are documented in this file. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this
project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Automated release pipeline (`.github/workflows/release.yml`): one click or a
  `v*` tag builds and publishes prebuilt binaries for Linux (x86_64/aarch64,
  glibc and static musl) and macOS (Intel and Apple Silicon), with SHA-256
  checksums, build-provenance attestations, a crates.io publish via Trusted
  Publishing (GitHub OIDC, no stored token), a multi-arch `ghcr.io` container
  image and floating `v<major>`/`v<major>.<minor>` tags.
- `curl | sh` installer (`scripts/install.sh`) with checksum verification and
  version pinning.
- Composite GitHub Action (`uses: SF-Zhou/blogen@v0`) that installs the
  prebuilt binary and adds it to `PATH`.
- `cargo binstall blogen` support through `[package.metadata.binstall]`.
- Installation guide (`docs/install.md`) and a release runbook
  (`RELEASING.md`).

### Changed

- `Cargo.lock` is now committed so release builds with `--locked` are
  reproducible.
- Release archives use `--profile dist` (fat LTO) and contain the executable,
  license files, changelog and a `BUILD_INFO.txt` with commit/toolchain data.

## [0.1.0] - 2026-11-13

### Added

- Initial standalone Rust implementation of the blog generator.
- TOML configuration, article discovery and filename metadata parsing.
- markdown-it/Prism/KaTeX-compatible markdown pipeline: heading anchors, table
  of contents, implicit figures, math, syntax highlighting, autolinks, GFM
  tables, strikethrough (task-list syntax stays literal, like the legacy pipeline).
- Bundled default theme reproducing the original SF-Zhou blog design.
- Index page, `index.json`, RSS feed and optional profile README generation.
- Mermaid diagrams rendered to static inline SVG at build time by a
  pure-Rust renderer (light and dark variants, no client-side JavaScript).
- `blogen build`, `init`, `serve` (with watch/live reload), `clean` and `theme`
  commands.
- Write-if-changed output, generation manifest and stale-file cleanup.
- Parallel rendering and remote image downloading with content-addressed files.
