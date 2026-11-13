# Contributing

Thanks for your interest in blogen!

## Development setup

```bash
git clone https://github.com/SF-Zhou/blogen
cd blogen
cargo build
cargo test
```

Rust 1.95 or newer is required.

## Before sending a pull request

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

- Keep the public API documented (`#![deny(missing_docs)]` is enabled).
- Add tests for new behaviour; the markdown pipeline should be verified against
  `scripts/reference/` (dev-only reference oracle).
- Update `docs/config-reference.md` / `docs/theme-authoring.md` when adding
  configuration keys or template context fields.
- Do not commit `node_modules`, build output or generated sites.

## Parity with the legacy generator

The migration target is the site in the sibling `blog` repository. The
verification workflow lives in `scripts/verify/`:

```bash
scripts/verify/run.sh --candidate /path/to/blogen/public
```

It rebuilds the reference baseline from the blog repository and compares article
bodies, `index.json`, RSS, profile output and screenshots.
