# blogen architecture

`blogen` is a standalone Rust static-site generator for the blog that used to be
built by the generator program in `src/` of `SF-Zhou/blog`. It must be a
self-contained, publishable crate: one binary, no external toolchain required at
build time or at site-build time.

## Goals

1. Reproduce the article/blog generation semantics of the original generator
   (posts discovery, filename metadata, index, RSS, profile, hidden posts, theme
   assets, remote-image downloading).
2. Make the visual result of every generated page equivalent to the original
   generator's output. HTML does **not** have to be byte-identical, but the
   rendered content
   (text, headings/anchors/TOC, code highlighting, math, diagrams, images,
   comments, metadata) must match. Differences that only reduce to whitespace,
   attribute order, or equivalent markup are acceptable.
3. Themeable: templates, styles, scripts and static assets live in a theme
   directory; the current blog look is the bundled `default` theme. The site
   author only needs a config file + markdown files to build a site.
4. Configuration is TOML. Everything user-specific (site name, URL, author,
   analytics id, giscus comments, paths, profile) lives in the config file.
5. Industrial quality: typed errors, no panics on user input, deterministic
   output, unit + integration tests, docs, CI, clippy/fmt clean, crates.io
   metadata.

## Key decisions

| Topic | Decision |
| --- | --- |
| Markdown | `pulldown-cmark` event stream converted into a markdown-it-like token model, then rendered with blogen's own HTML renderer. This gives us control over markdown-it-specific output (heading anchors, TOC, implicit figures, `<eq>`/`<eqn>` wrappers, Prism classes). |
| Math | Server-side KaTeX. The official `katex.min.js` (same version as the Node pipeline, 0.18.x) is vendored and executed through the `quick-js` engine. Zero Node dependency. |
| Syntax highlighting | Server-side Prism. The official `prismjs` core + the same language components the Node pipeline loads are vendored and executed through the same `quick-js` engine, so token classes are identical and the existing `prism-tomorrow` stylesheet keeps working. |
| Mermaid | Rendered to static inline SVG at build time by the pure-Rust [`merman`](https://crates.io/crates/merman) renderer: one SVG per diagram for Mermaid's default and dark themes, wrapped in the same `<figure class="mermaid"><div class="mermaid-light">…` markup as before. Generated pages contain no Mermaid JavaScript and work with JS disabled. Visual output is close to, but not pixel-identical with, the old browser-based renderer (Mermaid version, theme palette and deterministic text measuring differ slightly) — an accepted trade-off for fully static output. |
| Templates | `minijinja` (Jinja-like: inheritance, includes, better error messages). Theme templates are `.html` / `.md`; auto-escaping depends on the file extension. |
| Embedded theme | The default theme is compiled into the binary (`include_dir`) so `blogen init && blogen build` works with no repository checkout. External themes override via `[theme] path = "..."`. |
| Less | The default theme ships compiled CSS (the old `dist/main.less` is the source of truth for the CSS author, but blogen does not run a LESS compiler). Theme authors provide CSS directly. |
| Node dependency | None. Node is only used by *dev-only* parity scripts under `scripts/reference/` and `scripts/verify/`, which are not part of the crate. |
| Remote images | `https?` images in markdown are downloaded, content-hashed (MD5 like the old pipeline), stored under `<posts>/images/`, and rewritten to relative paths at render time. Unlike the old pipeline, source markdown is **not** modified. |

## Repository layout

```
Cargo.toml
src/
  main.rs          CLI entry point
  cli.rs           command definitions + dispatch
  error.rs         typed error enum
  config.rs        TOML config + defaults + validation
  model.rs         Article / site model types
  article.rs       article discovery + filename/title parsing
  site.rs          build orchestration
  output.rs        write-if-changed + generation manifest/cleanup
  assets.rs        theme/posts asset copying, remote image download
  feed.rs          RSS generation
  server.rs        `blogen serve` (static server + watch/live reload)
  init.rs          `blogen init` scaffolding
  theme.rs         theme loading (embedded/external)
  templates.rs     minijinja environment + template context
  markdown/        markdown pipeline (pre-passes, parser, tokens, renderer)
vendor/js/         vendored KaTeX/Prism bundles (see THIRD_PARTY.md)
themes/default/    embedded default theme (templates, assets, design sources)
scripts/reference/ dev-only reference renderer (parity oracle)
scripts/verify/    dev-only migration/parity harness (baseline, diffs, screenshots)
tests/             integration, CLI and fixture parity tests
docs/              architecture, configuration, theme authoring
```

## Configuration schema

All paths are relative to the directory containing the config file unless
absolute.

```toml
# ---- required ----------------------------------------------------------
[site]
name = "SF-Zhou's Blog"
url  = "https://sf-zhou.github.io"      # canonical site URL, no trailing slash
description = "SF-Zhou's Blog"
language = "zh-CN"
default_author = "SF-Zhou"
web_master = "SF-Zhou"
google_analytics_id = "G-XXXX"          # optional; GA snippet omitted if empty
base_path = "/"                         # optional, for sites served under a sub-path

# ---- build -------------------------------------------------------------
[build]
posts = "posts"                # markdown tree
output = "public"              # generated site root
article_format = "md"          # markdown extension
images = "images"              # image dir inside posts (copied to output/<images>)
download_remote_images = true  # fetch https images into posts/<images>
clean = true                   # remove outputs recorded in the previous manifest
parallel = 0                   # 0 = number of CPUs

# ---- theme -------------------------------------------------------------
[theme]
name = "default"               # bundled theme name ...
# path = "themes/mytheme"      # ... or an external theme directory (wins)
assets = "dist"                # output sub-directory for theme assets

# ---- profile (optional) ------------------------------------------------
[profile]
path = "profile"               # README.md written there
limit = 5

# ---- comments (optional) ----------------------------------------------
[comments.giscus]
repo = "SF-Zhou/sf-zhou.github.io"
repo_id = "MDEwOlJlcG9zaXRvcnk4MDc5ODgwNg=="
category = "Announcements"
category_id = "DIC_kwDOBNDkVs4CA6GQ"
mapping = "title"              # optional (defaults shown)
reactions_enabled = "0"
emit_metadata = "0"
input_position = "bottom"
theme = "preferred_color_scheme"
lang = "en"

# ---- rss ---------------------------------------------------------------
[rss]
enabled = true
file = "rss.xml"
limit = 20

# ---- serve -------------------------------------------------------------
[serve]
host = "127.0.0.1"
port = 9000
```

Unknown keys are rejected with a helpful error (`deny_unknown_fields`) so typos
never silently change behaviour.

## Theme contract

```
themes/<name>/
  theme.toml
  templates/
    base.html        layout, blocks: head, body
    article.html     extends base.html
    index.html       extends base.html
    profile.md       profile README
    partials/comments.html
  assets/            copied to <output>/<theme.assets>/ (default dist/)
```

`theme.toml`:

```toml
name = "default"
version = "0.1.0"
description = "The original SF-Zhou blog look"
license = "MIT OR Apache-2.0"
min_blogen_version = "0.1.0"

[templates]
article = "article.html"
index = "index.html"
profile = "profile.md"
```

If `[templates].profile` is missing the theme does not support profiles and
`[profile]` config is an error. `theme list` lists bundled themes, `theme
extract <dir>` writes a bundled theme to disk for customisation.

### Template context

Every template receives the same top-level context (see
`docs/theme-authoring.md` for the full field-by-field documentation):

```jsonc
{
  "blogen": { "version": "0.1.0", "generated_at": "2026-11-13T01:02:03Z" },
  "site": {
    "name": "...", "url": "...", "description": "...", "language": "zh-CN",
    "default_author": "...", "web_master": "...",
    "google_analytics_id": "G-..." | null,
    "base_path": "/", "assets_url": "/dist", "images_url": "/images"
  },
  "theme": { "name": "default", "version": "0.1.0" },
  "comments": { "giscus": { ... } } | null,
  "page": {
    "kind": "article" | "index" | "profile",
    "title_string": "...", "title": "...", "url_path": "...",
    "date": "2026.11.12" | null, "author": "..." | null,
    "author_url": "..." | null, "tags": [...], "hidden": false,
    "html": "<p>...</p>"        // rendered body, article pages only
  },
  "articles": [ { "title", "date", "author", "author_url", "tags", "filename", "url_path" } ]
}
```

The index page template loops over `articles` and includes the theme's card
partial; the article template embeds `page.html` with `| safe`; the profile
template renders a Markdown list from the first `[profile].limit` summaries.
Undefined variables are errors (`UndefinedBehavior::Strict`) and auto-escaping
is enabled for `.html` files.

## Build pipeline

1. Load+validate config, resolve paths, load theme (external dir or embedded).
2. Discover `**/*.<article_format>` under `[build].posts` (sorted, deterministic)
   and directories (for output dirs).
3. For each article: parse filename metadata `[date tags author...]name.md`,
   extract `# title` from the first line, resolve remote images, render markdown
   (parallel), build `Article`.
4. Render article pages through the theme; hidden (`Hidden` tag) articles produce
   `name.htm` and are excluded from index/RSS/profile.
5. Sort visible articles (date desc, then title asc); render `index.html` (cards +
   theme layout), `index.json`, `rss.xml`, optional profile README.
6. Copy theme assets to `<output>/<theme.assets>/` and post images to
   `<output>/images/`; record every generated file in
   `<output>/.blogen-manifest.json`; delete stale files from the previous
   manifest when `clean = true`.
7. Files are only rewritten when their content changed (mtime-stable output).

## Markdown parity checklist

The reference implementation (dev-only) is `scripts/reference/render.mjs`, a
faithful copy of the old `src/marked.js` (without puppeteer/Mermaid). Its output
for any markdown input is the oracle. Required behaviours:

- CommonMark + GFM tables + strikethrough + autolink literals (the legacy
  pipeline has no task-list plugin, so `- [x] task` stays literal text)
  (`linkify: true`) — **note:** the old config passes `typography: true`, which
  markdown-it ignores; the real option is `typographer`, so no typographic
  replacements happen. blogen therefore defaults to no typographer.
- Fenced/indented code → `<pre class="language-<lang>"><code class="language-<lang>">…</code></pre>`
  with Prism highlighting; `''` means `markup`; aliases `c++`→`cpp`, `yml`→
  `yaml`, `asm`/`assembly`→`nasm`; unknown languages are HTML-escaped without
  highlighting and keep their class.
- `mermaid` fences become static inline SVG at build time:
  `<figure class="mermaid" data-mermaid-digest="…"><div class="mermaid-light">SVG</div><div class="mermaid-dark">SVG</div></figure>`
  (light = Mermaid `default` theme, dark = Mermaid `dark` theme). No Mermaid
  JavaScript is shipped; diagrams render with JS disabled.
- KaTeX: inline `$…$` → `<eq>…katex html…</eq>`; `$$…$$` → `<section><eqn>…</eqn></section>`
  (display mode), including the `$$…$$ (number)` eqno form.
- Headings: `id="<slug>"` where slug = `value.trim().toLowerCase()` with runs of
  whitespace replaced by `-`; duplicate slugs get `-1`, `-2`… suffixes;
  `tabindex="-1"`; content wrapped in `<a href="#<percent-encoded-slug>">…</a>`.
- `[TOC]` line replaced by `<div class="table-of-contents"><ul>…nested…</ul></div>`
  for heading levels 1–3, link text from heading text/code, links use the raw
  slug.
- Paragraph that contains only an image (or a link wrapping an image) becomes
  `<figure tabindex="N"><a href="src"><img …></a><figcaption>alt as inline
  markdown</figcaption></figure>`; `alt` is cleared and used for the caption;
  `tabindex` counts from 1.
- Raw HTML is escaped (`html: false` semantics).
- Links: `[text](url "title")`, autolinks, bare URL/email linkification.

## Parity status

Validated against a frozen reference oracle of the real blog (commit
`d4cb7aafad90`) built from the same posts:

- Real blog corpus: **90/90 article bodies equivalent** to the original
  pipeline (after canonicalising the Mermaid figures), verified by the in-crate
  parity test and by `scripts/verify/compare_sites.py`.
- Site-level comparison: 88 article pages byte-identical, 2 "equivalent" only
  because Mermaid diagrams are now rendered statically by a pure-Rust renderer
  (slightly different theme palette/geometry than the old browser renderer);
  `index.json`, index cards, `rss.xml` and the profile README are identical;
  the generated site is 203 files / ~20 MB again (the 3.5 MB Mermaid JS bundle
  is gone), and 913 local references resolve with 0 broken links.
- Screenshots (1100 px, light + dark, full page): **0.000% pixel difference**
  for all 12 non-Mermaid page/scheme comparisons; the four Mermaid page/scheme
  comparisons differ by 10.682%, 13.649%, 16.286% and 16.499% because of the
  static renderer's different theme palette and text metrics — an accepted
  trade-off for JS-free static diagrams. All Mermaid comparisons are reported
  but treated as "soft" in the harness.
- JS-free verification: the two diagram pages contain 6 static figures / 12
  inline SVGs, zero Mermaid `<script>` tags and zero Mermaid assets; Chromium
  with JavaScript disabled renders diagrams pixel-identically to the JS-enabled
  build (0.000% difference on all 4 page/scheme checks). Machine-readable
  evidence: `docs/parity/static-mermaid-check.json`.
- Markdown pipeline unit/fixture tests: 161 Rust tests green, including
  byte-exact golden snippets for every supported markdown feature.
- The migration gate is `scripts/verify/run.sh --candidate <dir>` (dev-only,
  needs the legacy toolchain to build the oracle; not needed to build or use blogen).

## Non-goals / intentional divergences

- Source markdown is not mutated when remote images are downloaded.
- Article ordering is a well-defined (date desc, title asc) sort instead of the
  old unstable comparator.
- TOC/HTML nesting may differ in insignificant whitespace.
- Mermaid diagrams are rendered to static inline SVG at build time by the
  pure-Rust `merman` renderer. Output is close to, but not pixel-identical
  with, the old browser-based renderer (Mermaid version, default/dark theme
  palette and deterministic text metrics differ slightly); no JavaScript is
  shipped and diagrams work with JS disabled. This is the accepted trade-off
  for fully static rendering (see "Parity status").
- Fonts/CSS/favicons live in the theme; blogen does not invoke `lessc`.

## Verification strategy

- `scripts/reference/render.mjs` renders any markdown exactly like the old
  pipeline (math/highlight/figure/anchor/TOC); blogen has a parity test that
  compares normalized HTML for a corpus of posts.
- `scripts/verify/` builds the real blog with both generators and compares
  `index.json`, `rss.xml` (date fields normalized), profile output, and article
  body text/structure; screenshots of representative pages (light+dark) are
  compared visually.
- Rust tests: unit tests for filename metadata, slugify, RSS dates, config
  validation; integration test builds `tests/fixtures/site` with the embedded
  theme and checks generated files against `tests/fixtures/expected`.
