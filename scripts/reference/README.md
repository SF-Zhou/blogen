# Reference renderer (dev only)

This directory contains a copy of the original markdown pipeline
(`marked.js`, from `SF-Zhou/blog` `src/marked.js`) pinned to the exact versions
that produced the current site, plus a small CLI. It exists **only** to verify
that `blogen`'s Rust markdown renderer produces equivalent HTML. It is not part
of the `blogen` crate and is never required to build a site.

The only difference from the original: Mermaid blocks are not rendered with
puppeteer; they are left as placeholders (or replaced with marker divs when
`--mermaid placeholder` is passed) so HTML diffs are deterministic.

## Setup

```bash
source ~/.nvm/nvm.sh && nvm use 24   # or any Node >= 18
npm install
```

## Usage

```bash
node render.mjs input.md                  # HTML on stdout
node render.mjs input.md --out out.html   # HTML to a file
node render.mjs --corpus ../../posts --out-dir /tmp/reference-html
```

Pinned versions: markdown-it 14.3.0, markdown-it-anchor 9.2.1,
markdown-it-implicit-figures 0.12.0, markdown-it-table-of-contents 1.2.0,
markdown-it-texmath 1.0.0, katex 0.18.1, prismjs 1.30.0.
