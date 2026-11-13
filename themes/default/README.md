# blogen default theme — static assets

Everything in this directory is copied verbatim into `<output>/<theme.assets>/`
(default `/dist/`) by `blogen build` (relative paths are preserved), so the
published site looks exactly like the original blog:

```
dist/
  main.css            compiled src/main.less
  highlight.css       prism-tomorrow.css  (PrismJS 1.30.0)
  katex.min.css       KaTeX 0.18.1 stylesheet
  fonts/              KaTeX 0.18.1 web fonts (60 files, referenced as fonts/...)
  main.js             tag/hash filter for the index page
  favicon.ico
  favicon.png
```

The theme contains **no JavaScript for diagrams**. Mermaid diagrams are
rendered to static inline SVG at build time (see below), so the generated site
works with JavaScript disabled.

## Template integration

The theme templates must reference the flat `/dist/` layout:

```html
<link rel="shortcut icon" href="{{ site.assets_url }}/favicon.ico">
<link rel="apple-touch-icon" href="{{ site.assets_url }}/favicon.png">
<link rel="stylesheet" href="{{ site.assets_url }}/highlight.css">
<link rel="stylesheet" href="{{ site.assets_url }}/katex.min.css">
<link rel="stylesheet" href="{{ site.assets_url }}/main.css">
...
<!-- before </body> -->
<script src="{{ site.assets_url }}/main.js"></script>
```

`main.js` keeps the original behaviour: on `/` (or `/index.html`) it filters the
card list by the `#/Tag` hash, adds `.selected` to the active tag and hides
cards that do not carry it. It is the only script the theme ships.

## Mermaid diagrams (static, build-time)

Mermaid is rendered **while the site is built**, by the pure-Rust `merman`
renderer (not by the theme and not in the browser). No JavaScript runtime, no
Node.js and no browser is involved at build time or on the published page; the
generated HTML already contains the final inline SVG. For each diagram the
markdown renderer emits two variants:

```html
<figure class="mermaid" data-mermaid-digest="<md5 of the diagram source>">
  <div class="mermaid-light"><svg id="mermaid-<digest8>-<i>-light">…</svg></div>
  <div class="mermaid-dark"><svg id="mermaid-<digest8>-<i>-dark">…</svg></div>
</figure>
```

* `mermaid-light` is rendered with Mermaid's `default` theme, `mermaid-dark`
  with its `dark` theme.
* `data-mermaid-digest` (MD5 of the diagram source) is used by the parity tests
  and keeps generated pages easy to diff; SVG ids are stable and unique per
  page (`<digest8>` + diagram index).
* The `main.css` rules from `src/main.less` do the rest: `.markdown
  figure.mermaid { text-align: center }`, `svg { max-width: 100%; height: auto }`,
  `.mermaid-dark { display: none }`, and under
  `@media (prefers-color-scheme: dark)` the inverse — so exactly one variant is
  shown and the SVG scales down on narrow screens.
* Because the markup is fully static, diagrams are visible with JavaScript
  disabled and the page has no client-side rendering cost. There is no
  `mermaid.min.js`, no lazy loader and no `pre.mermaid-source` element.

The renderer lives in `src/markdown/mermaid_static.rs` and depends on the
`merman` crate; both are covered by `THIRD_PARTY.md`. Its visual output is
close to, but not byte-identical with, the previous browser-based renderer
(Mermaid version, theme palette and text measuring differ slightly) — the
accepted trade-off for fully static output.

## Provenance, versions, licenses

| File | Origin | Version | License |
| --- | --- | --- | --- |
| `main.css` | `npx lessc themes/default/src/main.less` (byte-identical output) | less 4.8.0 | MIT OR Apache-2.0 (the blog author's own work); lessc is Apache-2.0 |
| `highlight.css` | `node_modules/prismjs/themes/prism-tomorrow.css` | PrismJS 1.30.0 | MIT, © PrismJS contributors |
| `katex.min.css`, `fonts/**` | `node_modules/katex/dist/` | KaTeX 0.18.1 | MIT, © Khan Academy |
| `main.js` | `themes/default/src/main.js` (verbatim, from the original blog) | — | MIT OR Apache-2.0 (the blog author's own work) |
| `favicon.ico`, `favicon.png` | original blog favicons | — | MIT OR Apache-2.0 (the blog author's own work) |

The default theme (templates, `main.css`, `main.js` and favicons), like blogen
itself, is dual licensed **MIT OR Apache-2.0**. The stylesheet, script and
favicons are the blog author's own work, ported from the original blog into
this theme by the same copyright holder; the standalone theme directory ships
copies of both license texts (`LICENSE-MIT`, `LICENSE-APACHE`) so an extracted
theme remains self-describing. The third-party assets above (Prism stylesheet,
KaTeX CSS/fonts) keep their upstream MIT licenses. The build-time Mermaid
renderer is part of the crate rather than the theme and is documented in
`THIRD_PARTY.md`.

## Regeneration commands

Set these paths, then run the commands in order:

```bash
THEME=/path/to/blogen/themes/default/assets
SRC=/path/to/blogen/themes/default/src        # main.less / main.js (original sources)
TMP=$(mktemp -d)                              # npm dependencies for regeneration
```

Regeneration needs the npm toolchain (only for regenerating the theme, never at
site-build time) and network access for the pinned packages:

```bash
cd "$TMP"
npm install --no-save less@4.8.0 prismjs@1.30.0 katex@0.18.1
```

### CSS

`main.css` is the plain lessc output of the theme's LESS source; nothing is
appended afterwards:

```bash
npx lessc "$SRC/main.less" "$THEME/main.css"
```

### Prism stylesheet, KaTeX stylesheet + fonts, main.js, favicons

```bash
cp "$TMP/node_modules/prismjs/themes/prism-tomorrow.css" "$THEME/highlight.css"
cp "$TMP/node_modules/katex/dist/katex.min.css"          "$THEME/katex.min.css"
cp -r "$TMP/node_modules/katex/dist/fonts"               "$THEME/fonts"
cp "$SRC/main.js"                                        "$THEME/main.js"
cp "$SRC/favicon.ico" "$SRC/favicon.png"                 "$THEME/"
```

`katex.min.css` references its fonts as `fonts/KaTeX_…`; because the theme is
copied flat, `/dist/katex.min.css` + `/dist/fonts/**` resolve correctly —
do not move the CSS into a sub-directory without rewriting those URLs.

Nothing else in the theme is generated: Mermaid SVGs are produced by the Rust
build, not by a theme tool.

## File sizes and SHA-256

`sha256sum` of the top-level assets (sizes in bytes). Regenerated files are
deterministic for these exact tool/package versions.

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| `main.css` | 5174 | `690ea2d9e4d71b122d659a3ce10a53c2de58ff7214915d0274372f0409b77fab` |
| `highlight.css` | 1766 | `d1d928842f5912ea6a59bab4852e5c551e1041727e9a9e8dda8272ba4b3a82dd` |
| `katex.min.css` | 24727 | `0fb711c9c74cb1718661933948b653fbc09a627da5dde8926b4d10585370993e` |
| `main.js` | 739 | `f50c9360dd70f8f4ccb999dca12325e2302b349d1759694f177bd82b6577230b` |
| `favicon.ico` | 5558 | `cee41583945a4ec124961132a78bb004d82d95793a3a6e8a53545752b5ae450c` |
| `favicon.png` | 8744 | `2d7c36e283a3801e81806193462a9d7969628500cf2e46c1715994d6d383c94c` |

`fonts/`: 60 files, 1 076 572 bytes total (upstream `katex@0.18.1/dist/fonts`).
Total assets directory: 1 123 280 bytes (~1.1 MB, 66 files), almost all of it
KaTeX fonts. All files are served locally; there are no CDN dependencies and
no diagram JavaScript bundle.

## Verification

The static-figure markup was verified against the Node/puppeteer-generated
baseline `/tmp/blogen-baseline-public` (scratch harness in
`/tmp/themer-static-verify/`, not committed). The test page contains the real
build output markup (inline light/dark SVGs plus `data-mermaid-digest`) and only
`main.css` — no scripts:

* light and dark screenshots at 1100 px vs the baseline article pages —
  **0 changed pixels** for all 5 diagrams of
  `posts/rdma/[2025.11.19 RDMA,Memory]rdma_buffer_safe_reuse.md` and the single
  diagram of `ruapc_safe_zero_copy.md`;
* `.mermaid-light` is `display: block` and `.mermaid-dark` `display: none` in
  light mode, the inverse in dark mode (`prefers-color-scheme`);
* with JavaScript fully disabled (`page.setJavaScriptEnabled(false)`) both
  variants are still present and the correct one is displayed — diagrams need
  no JS;
* at a 420 px viewport there is no horizontal overflow
  (`document.scrollWidth == 420`), the figure is 388 px wide and the SVG scales
  down to the same 388 px via `max-width: 100%`, with `height: auto`
  (e.g. 704×525 → 388×290);
* the page issues no requests for any Mermaid script/bundle.

Evidence files: `/tmp/themer-static-verify/screens/` (`rdma-*`, `ruapc-*`
static/baseline/no-JS/420px PNGs), `/tmp/themer-static-verify/report.json`
(display/geometry/{script,request} assertions) and
`/tmp/themer-static-verify/pixel-diff.json` (pixel statistics).
