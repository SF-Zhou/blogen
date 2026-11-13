# Changelog

All notable changes to this project are documented in this file. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this
project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

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
