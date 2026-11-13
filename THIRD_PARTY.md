# Third-party notices

`blogen` embeds third-party source code and assets. This file lists everything
that is redistributed inside the crate, with the upstream project, version,
license and the sha256/MD5 checksum of the vendored copy.

## Vendored JavaScript

The JavaScript bundles under `vendor/js/` are the *official, unmodified*
upstream builds. They are embedded into the `blogen` binary at compile time
(`include_str!`) and executed inside the `quick-js` JavaScript engine to render
math and highlight code server-side, without an external JavaScript toolchain at build time.
They are not part of any public JavaScript API of the generated site.

### KaTeX 0.18.1

- File: `vendor/js/katex.min.js` (271,715 bytes)
- MD5: `80aa34726ff95d33453466c36f30a471`
- Upstream: <https://github.com/KaTeX/KaTeX>
- Source: upstream package `katex@0.18.1`, `dist/katex.min.js`
- License: MIT — Copyright (c) 2013-2020 Khan Academy and other contributors
  (<https://github.com/KaTeX/KaTeX/blob/main/LICENSE>)
- Used by: `src/markdown/js.rs` (`katex.renderToString`) via
  `src/markdown/math.rs`.

### Prism 1.30.0

- Core file: `vendor/js/prism.js` (58,240 bytes), which bundles the Prism core
  plus the `markup`, `css`, `clike` and `javascript` grammars
  (upstream file `prismjs@1.30.0/prism.js`).
- MD5 `7659a5599c2cb206218bcb4b1389d26e`
- Language components (upstream `prismjs@1.30.0/components/`):

  | file | bytes | MD5 |
  | --- | ---: | --- |
  | `prism-bash.min.js` | 6,143 | `c8e89824570c2c13fba9ec5834f02d97` |
  | `prism-c.min.js` | 1,893 | `434e7bb526a0dc43c57fd310c4cf109b` |
  | `prism-cmake.min.js` | 10,474 | `4fbfc4ab9e8c9d6484c206e1fed4d18f` |
  | `prism-cpp.min.js` | 2,596 | `291e2672e5d1710a90f3f372773d2e90` |
  | `prism-json.min.js` | 449 | `242d107e99fcaf0e6e45284220ceca49` |
  | `prism-lua.min.js` | 598 | `958f0b487dbfccc5cc416efe971bc3b1` |
  | `prism-nasm.min.js` | 699 | `cd930da6b9f2550554618e5b9521dede` |
  | `prism-protobuf.min.js` | 981 | `bef9d0a3d0a5741fb78b9e0fc59e79fd` |
  | `prism-python.min.js` | 2,113 | `547961f651d376e906ff261e15dd5697` |
  | `prism-rust.min.js` | 2,472 | `0c367c2dc52e7475f7d79fb77873d942` |
  | `prism-toml.min.js` | 955 | `cc709ecf68ecec254b457216a79ca216` |
  | `prism-yaml.min.js` | 1,970 | `2b5b1727158168331fa44aa06db77c3b` |

- Upstream: <https://github.com/PrismJS/prism>
- License: MIT — Copyright (c) 2012 Lea Verou
  (<https://github.com/PrismJS/prism/blob/master/LICENSE>)
- Used by: `src/markdown/js.rs` (`Prism.highlight`) via `src/markdown/prism.rs`.

The component set matches exactly what the original pipeline loaded
(`prismjs/components/index.js`: `bash`, `cmake`, `cpp`, `json`, `lua`, `nasm`,
`protobuf`, `python`, `rust`, `toml`, `yaml` plus their dependencies `c` and
`clike`, both of which ship with the core bundle).

## How the files are kept in sync

Both bundles are copied verbatim from the upstream packages the original blog pinned
(`markdown-it` reference pipeline in `scripts/reference/README.md`). To verify a
checkout:

```bash
md5sum vendor/js/*
```

Any modification invalidates the exact output parity guaranteed by the tests in
`src/markdown/` (the reference oracle renders with the same package versions).

## Bundled default theme assets

The embedded `default` theme (`themes/default/`, compiled into the binary with
`include_dir!`) redistributes the styling of the original SF-Zhou blog together
with MIT-licensed third-party components. Full provenance, sizes, SHA-256
checksums and regeneration commands are documented in
[`themes/default/README.md`](themes/default/README.md).

| Asset | Origin | License |
| --- | --- | --- |
| `assets/main.css` | compiled from `themes/default/src/main.less`, the blog author's own stylesheet (ported from the original blog) | MIT OR Apache-2.0 (same as blogen; © SF-Zhou) |
| `assets/main.js` | the blog author's tag-filter script (from the original blog) | MIT OR Apache-2.0 (same as blogen; © SF-Zhou) |
| `assets/favicon.ico`, `assets/favicon.png` | original blog favicons | MIT OR Apache-2.0 (same as blogen; © SF-Zhou) |
| `assets/highlight.css` | `prism-tomorrow.css` from PrismJS 1.30.0 | MIT, © PrismJS contributors |
| `assets/katex.min.css`, `assets/fonts/**` | KaTeX 0.18.1 `dist/` | MIT, © Khan Academy and other contributors |

## Build-time Rust dependencies

Static Mermaid rendering is provided by the [`merman`](https://crates.io/crates/merman)
crate (version 0.10, MIT OR Apache-2.0), compiled into the `blogen` binary and
used headlessly (no browser or JavaScript runtime). blogen enables its `svg`
and `all-diagrams` features only; the optional ELK/Cytoscape layout features
(and their EPL-2.0 notices) are not compiled in. Mermaid JavaScript bundles are
no longer shipped, neither in the crate nor in generated sites.

## Bundled default theme assets

The entire `default` theme (templates + assets + design sources), like blogen
itself, is dual licensed **MIT OR Apache-2.0**. The theme stylesheet, script
and favicons are the blog author's own work, contributed to this crate by the
copyright holder; copies of both license texts ship inside
`themes/default/` so an extracted theme stays self-describing. Third-party
components bundled with the theme remain under their upstream MIT licenses
(see the table above).
