# Visual parity evidence

These images were captured from the **migrated blog** (built by blogen) and the
frozen **reference baseline** of the same posts (the last Node.js-generator
commit `d4cb7aafad90`) with `scripts/verify/screenshots.mjs` and
`scripts/verify/make_parity_images.mjs`: Chromium 150 (puppeteer headless
shell), 1100 px viewport, no external requests, light + dark
`prefers-color-scheme`, animations disabled.

Measured result against the baseline: **0.000% differing pixels on every
non-Mermaid page/scheme**, and only the two Mermaid articles differ (static
`merman` SVG vs the old server-rendered mermaid-cli SVG — an accepted visual
difference, see below). Content parity is exact: 88/90 article pages
byte-identical, 2/90 equivalent only because of the Mermaid figure contents,
with identical `index.json`, index cards, `rss.xml` and profile README
(203/203 generated files, 913 local links resolve).

## Static Mermaid rendering

blogen renders Mermaid fences **to inline SVG at build time** with the pure-Rust
[`merman`](https://crates.io/crates/merman) renderer (route A; no browser, no
client-side JS):

- one `<svg>` per diagram for Mermaid's default ("light") and dark themes,
  wrapped in the same markup as the old server-rendered pipeline:
  `<figure class="mermaid" data-mermaid-digest="…"><div class="mermaid-light"><svg…`
  + `<div class="mermaid-dark">…`;
- generated pages load **no Mermaid JavaScript** and the theme assets
  (`public/dist/`, 66 files) ship no `mermaid*` bundle (the old client-side
  `mermaid.min.js`/`mermaid-init.js` are gone);
- diagrams are therefore visible with JavaScript disabled, work offline, and
  add no script/parse cost;
- article page size is back to the baseline range: the two Mermaid pages are
  304,937 B and 64,234 B (baseline: 305,553 B and 64,406 B); whole site 203
  files / ~20 MB.

Visual output is close to, but not pixel-identical with, the old
browser-rendered SVG: the Mermaid version, theme palette and text measurement
differ slightly. That is the accepted trade-off for fully static output; the
pixel-diff numbers below quantify it.

### JS-free verification

`node scripts/verify/check_static_mermaid.mjs --site ../blog/public
--summary-out docs/parity/static-mermaid-check.json` (full run report in
`scripts/verify/.cache/static-mermaid/report.json`, committed summary in
`static-mermaid-check.json`):

- 2 pages, 6 Mermaid figures, **12 inline SVGs** (6 light + 6 dark,
  `data-mermaid-digest` present on every figure);
- **0** `<script>` tags referencing Mermaid, **0** linked Mermaid assets,
  **0** `mermaid*` files in the generated site (203 files total);
- Chromium 150 captures with `setJavaScriptEnabled(false)` render the diagrams
  (visible SVG e.g. 704×565 px) and are **pixel-identical to the
  JavaScript-enabled captures** (0.000% differing pixels for all 4
  page/scheme checks).

## Screenshot parity numbers

16 comparisons (8 page(s) × light/dark), threshold 0.5%, per-channel tolerance
12, full page (capped at 6000 CSS px):

| page | light | dark |
| --- | ---: | ---: |
| `index.html` | 0.000% | 0.000% |
| `algorithm/eigenvalues_and_eigvectors.html` | 0.000% | 0.000% |
| `algorithm/fitted_exp_function.html` | 0.000% | 0.000% |
| `algorithm/generic_dp.htm` | 0.000% | 0.000% |
| `beegfs/beegfs_01.htm` | 0.000% | 0.000% |
| `algorithm/lagrange_multipliers.html` | 0.000% | 0.000% |
| `rdma/rdma_buffer_safe_reuse.html` (Mermaid, soft) | 16.286% | 16.499% |
| `rdma/ruapc_safe_zero_copy.html` (Mermaid, soft) | 10.682% | 13.649% |

Mermaid pages are **soft**: the difference is the accepted static-SVG
rendering difference (diagram layout/palette/text metrics; the page grows by
28 px, +0.1%, on `ruapc_safe_zero_copy`), not a content or layout regression.
Every non-Mermaid comparison must stay at 0.000% (hard 0-diff checks).

## Images

- `index-light.jpg` – home page, blogen static build.
- `math-code-light.jpg` – article with KaTeX math, Prism-highlighted code and a
  GFM table, blogen static build (`programming/robin_hood_hashing.html`).
- `mermaid-dark.jpg` – Mermaid article (`rdma/rdma_buffer_safe_reuse.html`) in
  dark mode, static inline SVG, no JavaScript.
- `mermaid-static-vs-baseline.png` – four-panel comparison of one sequence
  diagram (light + dark): reference baseline (server-rendered mermaid-cli SVG)
  vs blogen static build-time SVG (`merman`). Both are pure SVG served in the
  HTML; the visible differences are the accepted renderer/theme-metric ones.
- `static-mermaid-check.json` – machine-readable JS-free evidence: figure and
  inline-SVG counts, `data-mermaid-digest` values, zero mermaid scripts/assets,
  JS-disabled pixel deltas and the Chromium version.

The directory is kept below 600 KiB.

## Regenerate

Dev-only; needs the legacy Node toolchain to rebuild the frozen baseline and
puppeteer (see `scripts/verify/README.md`):

```bash
# 1. frozen reference baseline (once; auto-uses the pinned legacy commit)
scripts/verify/make_baseline.sh --bootstrap-node-modules

# 2. full parity gate + screenshot diffs (writes .cache/screenshots/)
scripts/verify/run.sh --candidate ../blog --screenshots

# 3. JS-free / static-SVG evidence
node scripts/verify/check_static_mermaid.mjs --site ../blog/public \
    --summary-out docs/parity/static-mermaid-check.json

# 4. refresh the evidence images below (raw captures in .cache/parity-raw/)
node scripts/verify/make_parity_images.mjs
python3 scripts/verify/make_parity_images.py
```
