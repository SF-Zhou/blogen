# blogen

A fast, themeable static blog generator written in Rust.

`blogen` turns a directory of Markdown files and a TOML configuration file into
a complete static site: HTML pages, an index page, a JSON article index and an
RSS feed (plus an optional profile README). It is designed as a drop-in
replacement for the original static blog generator that powers
[SF-Zhou's Blog](https://sf-zhou.github.io), but it is a standalone crate with no
external toolchain dependency.

## Features

- **Single binary, no runtime dependencies.** KaTeX math and Prism syntax
  highlighting are executed server-side through an embedded JavaScript engine;
  Mermaid diagrams are rendered to static inline SVG at build time by a
  pure-Rust Mermaid renderer, so generated pages need no JavaScript, no CDN and
  no external toolchain.
- **Markdown with batteries included.** GitHub-flavoured tables, heading
  anchors, a `[TOC]` table of contents, implicit figures (`![caption](img.png)`),
  autolinks, strikethrough, math and diagrams. (The legacy generator had no
  task-list plugin, so task-list syntax stays literal — matching its output.)
- **Themeable.** Templates (Jinja syntax via [minijinja]), styles, scripts and
  fonts live in a theme directory. The original blog look ships as the embedded
  `default` theme; the binary can extract it for customisation.
- **TOML configuration.** All site-specific settings (name, URL, author,
  analytics, comments, paths, profile) live in one file.
- **Safe & fast.** Parallel rendering with rayon, write-only-when-changed output,
  content-addressed copying of images, stale-file cleanup via a manifest,
  deterministic builds, strict template error reporting.
- **Developer friendly.** `blogen serve --watch` rebuilds and live-reloads on
  changes; no configuration is needed to get started.
- **Verified against the original generator.** The migration was validated
  against a reference oracle of the real blog: 88/90 article pages
  byte-identical and 2 "equivalent" only in the static rendering of their
  Mermaid diagrams, identical `index.json`/cards/RSS/profile, and 0.000% pixel
  difference on every non-Mermaid screenshot (Mermaid pages are close, not
  pixel-identical: the static renderer uses slightly different Mermaid themes
  and text metrics; ~10–16% of the pixels of those two pages differ). See
  [docs/architecture.md](docs/architecture.md#parity-status) and the
  [visual evidence](docs/parity/README.md).

## Installation

Prebuilt binaries are published for Linux (x86_64/aarch64, glibc 2.35+) and
macOS (Intel and Apple Silicon). On Linux and macOS:

```bash
curl -LsSf https://github.com/SF-Zhou/blogen/releases/latest/download/blogen-installer.sh | sh
```

In another CI pipeline, use the composite action (installs the binary and adds
it to `PATH`):

```yaml
- uses: SF-Zhou/blogen@v0
- run: blogen build
```

Other options:

```bash
cargo install blogen        # build from source (Rust 1.95+)
cargo binstall blogen       # prebuilt binary via cargo-binstall
docker run --rm -v "$PWD:/site" ghcr.io/sf-zhou/blogen:latest build
```

See [docs/install.md](docs/install.md) for version pinning,
checksum/attestation verification and the full platform table.

The release binary is ~35 MB: the pure-Rust Mermaid renderer is compiled in, so
no browser/runtime is needed to build a site.

## Quick start

```bash
blogen init my-blog
cd my-blog
blogen serve                 # http://127.0.0.1:9000, rebuilds on changes
```

When you are happy with the preview:

```bash
blogen build                 # writes ./public
```

## Writing articles

Articles are Markdown files inside the `posts` directory. Metadata is encoded in
the file name so that no front matter is needed:

```text
posts/[<date> <tag1,tag2,...> <author>]<file-name>.md
```

```text
posts/rdma/[2026.11.12 Rust,RDMA,RPC SF-Zhou]safe-rdma-zero-copy.md
posts/life/[2025.01.02 Life]new-year.md
posts/[2024.06.03 Hidden]about-page.md
```

- `date` – `YYYY.MM.DD`, used for sorting, the index and RSS.
- tags – comma separated, one whitespace-free token. The tag `Hidden` marks an
  article as unlisted: it is still generated (`name.htm`) but excluded from the
  index, RSS feed and profile.
- author – everything after the tags; may contain spaces. Only GitHub-handle
  looking names (`[A-Za-z0-9-]+`) get a profile link.
- The first line `# Title` becomes the article title.

Images referenced from markdown are copied from `posts/images/` into the output.
Remote (`http(s)://`) image URLs are downloaded once, stored content-addressed in
`posts/images/`, and rewritten to relative paths (disable with
`download_remote_images = false`).

## Configuration

```toml
[site]
name = "SF-Zhou's Blog"
url = "https://sf-zhou.github.io"
description = "SF-Zhou's Blog"
language = "zh-CN"
default_author = "SF-Zhou"
web_master = "SF-Zhou"
google_analytics_id = "G-GQ26H3JQ3G"

[build]
posts = "posts"
output = "public"

[theme]
name = "default"

[profile]
path = "profile"
limit = 5

[comments.giscus]
repo = "SF-Zhou/sf-zhou.github.io"
repo_id = "MDEwOlJlcG9zaXRvcnk4MDc5ODgwNg=="
category = "Announcements"
category_id = "DIC_kwDOBNDkVs4CA6GQ"
```

See [docs/config-reference.md](docs/config-reference.md) for every option,
default and validation rule.

## CLI

```text
blogen build   [--config FILE] [--output DIR] [--no-clean] [--strict]
blogen serve   [--config FILE] [--host HOST] [--port PORT] [--no-watch]
blogen init    [DIR] [--name NAME] [--url URL] [--force] [--no-example]
blogen clean   [--config FILE] [--force]
blogen theme   list
blogen theme   extract <name> <dir>
```

`--verbose` / `--quiet` control log verbosity; `RUST_LOG` overrides it.

## Themes

The bundled `default` theme reproduces the original SF-Zhou blog design. To
customise it:

```bash
blogen theme extract default my-theme
```

then point the configuration at it:

```toml
[theme]
path = "my-theme"
```

See [docs/theme-authoring.md](docs/theme-authoring.md) for the template context,
asset layout and packaging rules.

## Development

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The `scripts/reference/` directory contains a dev-only copy of the original
markdown pipeline (pinned to the versions that generated the old site) and a
CLI that dumps its HTML. `scripts/verify/` builds a reference baseline of the
real blog and compares it against a blogen build (article bodies, `index.json`,
RSS, profile, screenshots). They are not needed to build or use blogen.

## Migrating from the legacy generator

The legacy generator's `config.json` maps to:

| legacy `config.json` | blogen `blogen.toml` |
| --- | --- |
| `site_name` | `[site].name` |
| `site_url` | `[site].url` |
| `site_description` | `[site].description` |
| `site_language` | `[site].language` |
| `posts_path` | `[build].posts` |
| `output_path` | `[build].output` |
| `article_format` | `[build].article_format` |
| `default_author` | `[site].default_author` |
| `web_master` | `[site].web_master` |
| `google_analytics_id` | `[site].google_analytics_id` |
| `github_repo`, `github_repo_id` | `[comments.giscus].repo`, `.repo_id` |
| `profile_path` | `[profile].path` |

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. The embedded default theme (templates, styles, scripts and
favicons) is licensed under the same terms, **MIT OR Apache-2.0**; third-party
assets bundled with it are listed in [THIRD_PARTY.md](THIRD_PARTY.md).
