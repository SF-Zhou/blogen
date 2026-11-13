# Parity fixtures for blogen integration tests

This directory holds a small self-contained **fixture site** for `blogen` and the
**oracle outputs** produced for exactly that site by the original blog generator (`SF-Zhou/blog`, the generator blogen replaces).

Everything here is committed and deterministic: no network access, no
timestamps, no absolute paths. Total size is ~60 KiB.

```
tests/fixtures/
  site/
    blogen.toml                  fixture site config (docs/architecture.md schema)
    posts/
      rust/[2024.01.02 Rust,C++]first_post.md
      cpp/[2024.02.03 C++]code_highlighting.md
      linux/[2024.03.04 Linux,Tools Some Display Name]display_name_author.md
      misc/[2024.04.05 Misc]figure_image.md
      misc/[2024.05.06 Misc]mermaid_diagram.md
      secret/[2024.06.07 Hidden,Secret]hidden_note.md
      notes/[2024.07.08 Notes]filename_title.md
      notes/deep/[2023.12.31 Notes]nested_post.md
      productivity/[2024.08.09 Productivity,Tools]lists_quotes_unknown_lang.md
      images/photo.png          16x16 PNG used by the figure post
  expected/                     ORACLE (generated, do not hand-edit)
    MANIFEST.json               provenance + sha256 of every file below
    pages/<rel>                 full old-site article pages
    pages/index.html            full old-site index page
    bodies/<rel>                inner HTML of <div class="markdown"> per article
    cards.html                  inner HTML of the index page's <div class="markdown">
    index.json                  old-site index.json
    rss.xml                     old-site rss.xml (lastBuildDate is a placeholder)
    profile/README.md           old-site profile output
    mermaid/<rel>.json          mermaid fence definitions per article, in order
```

## Feature coverage

| fixture post (source) | generated page | covers |
| --- | --- | --- |
| `rust/[2024.01.02 Rust,C++]first_post.md` | `/rust/first_post.html` | front-matter bracket (date, 2 tags), `# title` line, `[TOC]`, inline `$…$` + display `$$…$$` + `$$…$$ (1)` eqno math, rust fence, GFM table with alignments |
| `cpp/[2024.02.03 C++]code_highlighting.md` | `/cpp/code_highlighting.html` | C++ fence, fence without language (`markup`), inline code |
| `linux/[2024.03.04 Linux,Tools Some Display Name]display_name_author.md` | `/linux/display_name_author.html` | multi-word display-name author (no GitHub link), multi-tag |
| `misc/[2024.04.05 Misc]figure_image.md` | `/misc/figure_image.html` | implicit figure from image with alt + title, emphasized caption, relative `../images/…` path |
| `misc/[2024.05.06 Misc]mermaid_diagram.md` | `/misc/mermaid_diagram.html` | mermaid fence; source definitions stored in `expected/mermaid/…json` |
| `secret/[2024.06.07 Hidden,Secret]hidden_note.md` | `/secret/hidden_note.htm` | hidden post: still generated (`.htm`), no comments, excluded from index/rss/profile |
| `notes/[2024.07.08 Notes]filename_title.md` | `/notes/filename_title.html` | title fallback to file name (no `# ` heading) |
| `notes/deep/[2023.12.31 Notes]nested_post.md` | `/notes/deep/nested_post.html` | nested output directory |
| `productivity/[2024.08.09 Productivity,Tools]lists_quotes_unknown_lang.md` | `/productivity/lists_quotes_unknown_lang.html` | blockquote, ordered/unordered lists, literal task-list syntax, unknown-language fence (no highlighting) |

8 visible posts with distinct dates exercise ordering (newest first) and the
profile limit (`[profile] limit = 5` in `blogen.toml`); the 9th post is hidden.
`default_author = "fixture-author"` gets a GitHub profile link, while the
multi-word `Some Display Name` author must be rendered without a link.

## Oracle conventions

The oracle was produced by the **original pipeline** in a throwaway
staging copy of the blog; blogen must be equivalent, not necessarily
byte-identical, for full pages. To keep the oracle small and stable, two
volatile parts are normalised:

1. **Mermaid** — the old generator rendered Mermaid server-side with puppeteer
   (two large SVGs per diagram); blogen renders each diagram to static light and
   dark SVG at build time, so the exact SVG differs. The expected files contain
   a bare placeholder instead:

   ```html
   <figure class="mermaid"></figure>
   ```

   The diagram source text is stored separately in `expected/mermaid/<rel>.json`
   (a JSON array of fence contents in document order). blogen's static figures
   carry `data-mermaid-digest="<md5 of the source>"`, and the Rust parity test
   compares those digests against the fixture definitions. The internal
   `placeholder` mermaid mode (used by the markdown corpus parity test) emits
   the old deterministic placeholder markup and is not part of the documented
   config surface; normalise the `<figure class="mermaid">…</figure>` subtree to
   `<figure class="mermaid"></figure>` exactly as for the default client mode.
   When comparing, apply the same normalisation to the actual output: replace
   each `<figure class="mermaid">…</figure>` subtree (regex
   `(?s)<figure class="mermaid">.*?</figure>`) with `<figure class="mermaid"></figure>`.

2. **RSS lastBuildDate** — the old generator wrote the build time. The expected
   file contains the literal placeholder `__LAST_BUILD_DATE__`; parse
   `rss.xml` and ignore `<lastBuildDate>` (the rest of the channel, including
   `atom:link`, and every item must match).

Additional comparison rules used by `scripts/verify/compare_sites.py` and
recommended for Rust tests:

* **body HTML** (`expected/bodies/<rel>`): replace the mermaid figure as above
  in both expected and actual, then compare. The old template puts indentation
  around the body, so `trim()` the extracted `<div class="markdown">` inner HTML
  before comparing (leading/trailing whitespace only — do not collapse inner
  whitespace).
* **index.json**: parse JSON; compare article objects by `url_path` and fields
  `title, date, author, author_url, tags, filename, url_path` (tag order
  significant, article order must also match here because the fixture has
  distinct dates).
* **cards.html / index.html**: compare parsed card content (title text,
  `url_path`, date, author, author link, tag text+href, in order), or compare
  `cards.html` after collapsing whitespace between tags.
* **rss.xml**: compare channel fields and items (title/link/guid/isPermaLink/
  pubDate/dc:creator/category list); ignore `lastBuildDate`.
* **profile/README.md**: compare exactly (a trailing-newline-only difference is
  acceptable); the fixture URL is `https://sf-zhou.github.io` so the old
  hard-coded profile template is comparable.
* **full pages** (`pages/<rel>`): useful for a theme test; compare the
  extracted `div.markdown` body against `bodies/`, plus `<title>`, `h1`, date,
  author text and author `href`. Full-page bytes may differ legitimately
  (template whitespace, comment script formatting).
* Assert the hidden post is **not** in `index.json`, `rss.xml` or the profile,
  but `secret/hidden_note.htm` exists and has no comment script.
* Assert the built site also contains `images/photo.png` (copied from
  `posts/images/`) so the figure's relative `../images/photo.png` resolves.

## Regeneration

The oracle must be regenerated with the original Node generator, never by
blogen. It is deterministic: `--check` regenerates in memory and fails if the
committed files are stale.

```bash
# from the blogen repo root; needs Node 24 and the blog checkout's node_modules
source ~/.nvm/nvm.sh && nvm use 24
scripts/verify/make_fixtures.sh            # rewrite tests/fixtures/expected/
scripts/verify/make_fixtures.sh --check    # CI-style staleness check (exit 1 if differs)

# the node_modules used by the oracle
BLOG_DIR=${BLOG_DIR:-../blog}
```

The blog repository has been migrated to blogen and no longer has the legacy
generator at HEAD, so the oracle source is resolved as `--ref` /
`$BLOGEN_BASELINE_REF`, else HEAD if it still has `src/main.js`, else the pinned
legacy commit `d4cb7aafad90f09cdcd85e2482eae739483743a5`. The legacy
`node_modules` is resolved as `--node-modules` / `$BLOGEN_BASELINE_NODE_MODULES`,
else `<blog>/node_modules`, else `scripts/verify/.cache/legacy-deps/node_modules`
(bootstrap once with `scripts/verify/make_baseline.sh --bootstrap-node-modules`;
frozen versions live in `scripts/verify/legacy-deps.json` and
`legacy-package-lock.json`).

`make_fixtures.sh`:
1. `git archive <legacy-ref>` of the Node blog into a temporary directory (the
   blog checkout is never modified or built in),
2. symlinks the legacy `node_modules` (exact pinned versions),
3. writes a staging `config.json` that points `posts_path` at a copy of
   `tests/fixtures/site/posts`,
4. runs `node src/main.js` (the same entry point `build.sh` runs),
5. extracts bodies, replaces Mermaid figures with the placeholder, rewrites the
   RSS placeholder, and writes `expected/` + `MANIFEST.json`.

Oracle provenance is pinned in `expected/MANIFEST.json`: blog commit
`d4cb7aafad90`, Node v24.19.0, markdown-it 14.3.0, markdown-it-anchor 9.2.1,
markdown-it-implicit-figures 0.12.0, markdown-it-table-of-contents 1.2.0,
markdown-it-texmath 1.0.0, katex 0.18.1, prismjs 1.30.0, mustache 4.2.0,
mermaid 11.16.0.

## Notes / known old-pipeline behaviour encoded here

* The old `markdown-it` has **no task-list plugin**: `- [x] done` renders as the
  literal text `<li>[x] done</li>` (no checkbox). The fixture encodes that.
* `tags` in the file-name bracket is the first token after the date:
  `[2024.03.04 Linux,Tools Some Display Name]` means date `2024.03.04`, tags
  `Linux`, `Tools`, author `Some Display Name`. Everything after the tags token
  is the author.
* Only authors matching `^[A-Za-z0-9-]+$` get a
  `https://github.com/<author>` link; display names with spaces do not.
* Hidden posts are written as `<name>.htm` (not `.html`) and keep their `.htm`
  extension in `expected/`.
* The fixture config disables remote-image downloading; the only image is
  committed in `posts/images/`.
