# Theme authoring

A theme is a directory that controls the HTML of every generated page and ships
the static assets referenced by that HTML.

```
my-theme/
├── theme.toml
├── templates/
│   ├── base.html
│   ├── article.html
│   ├── index.html
│   ├── profile.md
│   └── partials/
│       ├── card.html
│       └── comments.html
└── assets/
    └── ...                    # copied verbatim to <output>/<theme.assets>/
```

The bundled `default` theme is the original SF-Zhou blog look; extract it as a
starting point:

```bash
blogen theme extract default my-theme
```

and select it in `blogen.toml`:

```toml
[theme]
path = "my-theme"
# assets = "dist"    # output sub-directory for assets/ (default: dist)
```

## `theme.toml`

```toml
name = "my-theme"           # required
version = "1.0.0"           # required, semver
description = "..."         # optional, shown by `blogen theme list`
license = "MIT OR Apache-2.0" # optional, SPDX identifier (the bundled default theme)
min_blogen_version = "0.1.0" # optional; build fails on older blogen

[templates]
article = "article.html"    # default: article.html
index = "index.html"        # default: index.html
profile = "profile.md"      # optional; required only if [profile] is configured
```

Templates are rendered with [minijinja] (Jinja2-style: `{% extends %}`,
`{% include %}`, `{% for %}`, `{% if %}`, filters). Auto-escaping is enabled for
`.html`/`.htm`/`.xml` templates and disabled for everything else (e.g. `.md`).

Undefined variables are an error, so typos fail the build instead of rendering
empty strings.

## Template context

Every template receives:

```jsonc
{
  "blogen": {
    "version": "0.1.0",
    "generated_at": "2026-11-13T01:02:03Z"
  },
  "site": {
    "name": "SF-Zhou's Blog",
    "url": "https://sf-zhou.github.io",
    "description": "SF-Zhou's Blog",
    "language": "zh-CN",
    "default_author": "SF-Zhou",
    "web_master": "SF-Zhou",
    "google_analytics_id": "G-...",   // null when not configured
    "base_path": "/",
    "assets_url": "/dist",            // base_path + [theme].assets
    "images_url": "/images"
  },
  "theme": { "name": "default", "version": "0.1.0" },
  "comments": {                        // null when [comments] is absent
    "giscus": {
      "repo": "owner/repo",
      "repo_id": "R_...",
      "category": "Announcements",     // null when not configured
      "category_id": "DIC_...",        // null when not configured
      "mapping": "title",
      "reactions_enabled": "0",
      "emit_metadata": "0",
      "input_position": "bottom",
      "theme": "preferred_color_scheme",
      "lang": "en",
      "lazy_loading": true
    }
  },
  "page": {
    "kind": "article" | "index" | "profile",
    "title_string": "Article title | Site name", // use for <title>
    "title": "Article title",
    "url_path": "/rdma/post.html",
    "date": "2026.11.12",           // article only, null on index/profile
    "author": "SF-Zhou",            // article only
    "author_url": "https://github.com/SF-Zhou", // "" for display names
    "tags": ["Rust", "RDMA"],       // article only
    "hidden": false,                // article only
    "html": "<p>...</p>"            // rendered article body, empty on index/profile
  },
  "articles": [                       // summaries; empty on article pages
    {
      "title": "...",
      "date": "2026.11.12",
      "author": "SF-Zhou",
      "tags": ["Rust"],
      "filename": "post",
      "author_url": "https://github.com/SF-Zhou",
      "url_path": "/rdma/post.html"
    }
  ]
}
```

`article.html` receives the article context; `index.html` receives the article
summary list (newest first); `profile.md` receives the first `[profile].limit`
summaries. Use `{{ page.html | safe }}` to include rendered article HTML — it is
intentionally marked as raw. All other values are escaped by default.

The default theme's `article.html` and `index.html` extend `base.html`:

```
{% extends "base.html" %}
{% block title %}...{% endblock %}
{% block head %}...{% endblock %}
{% block content %}...{% endblock %}
{% block scripts %}...{% endblock %}
```

## Assets

Everything under `assets/` is copied verbatim to `<output>/<theme.assets>/`.
The copy is content-aware: unchanged files keep their mtime, removed files are
deleted through the generation manifest, and the copy runs in parallel-friendly
sorted order.

The default theme uses a flat layout that mirrors the legacy site:

```
assets/main.css            -> /dist/main.css
assets/highlight.css       -> /dist/highlight.css
assets/katex.min.css       -> /dist/katex.min.css
assets/fonts/**            -> /dist/fonts/**      (referenced by katex.min.css)
assets/main.js             -> /dist/main.js
assets/favicon.ico/.png    -> /dist/favicon.*
```

A theme is free to use sub-directories instead; just reference the resulting
URLs from the templates.

## Mermaid in a custom theme

Mermaid diagrams are rendered to inline SVG **at build time** by blogen itself
(using a pure-Rust renderer); themes do not need any JavaScript. For every
```` ```mermaid ```` block the markdown renderer emits:

```html
<figure class="mermaid" data-mermaid-digest="…">
  <div class="mermaid-light"><svg id="mermaid-…-light" …>…</svg></div>
  <div class="mermaid-dark"><svg id="mermaid-…-dark" …>…</svg></div>
</figure>
```

The light/dark variants use Mermaid's `default` and `dark` themes. The default
theme shows the matching variant with CSS (`prefers-color-scheme`):

```css
.markdown figure.mermaid .mermaid-dark { display: none; }
@media (prefers-color-scheme: dark) {
  .markdown figure.mermaid .mermaid-light { display: none; }
  .markdown figure.mermaid .mermaid-dark { display: block; }
}
```

A custom theme can restyle the figure/svg but should keep both wrappers. No
Mermaid JavaScript is shipped or required; diagrams work with JavaScript
disabled.

## Theme packaging checklist

- `theme.toml` with `name`, `version` and the template names.
- All templates are UTF-8.
- All CSS/JS/fonts are bundled locally; no CDN dependencies.
- `assets/` contains only files that should be published.
- Run `blogen build` and `blogen serve` with the theme selected; check the
  light and dark styles, math, code blocks, tables, figures and Mermaid
  diagrams.
- Include license information for any third-party assets you bundle.

[minijinja]: https://docs.rs/minijinja
