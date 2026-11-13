# Releasing blogen

1. Make sure the tree is clean and everything passes:

   ```bash
   cargo fmt --all --check
   cargo clippy --all-targets -- -D warnings
   cargo test
   RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
   ```

2. Run the full parity gate against the original generator (needs the
   sibling `blog` checkout with its legacy `node_modules`):

   ```bash
   scripts/verify/run.sh --candidate <blogen build dir> --screenshots
   ```

3. Update `CHANGELOG.md` (move entries from `[Unreleased]` to the new version),
   bump `version` in `Cargo.toml`, then refresh the lockfile:

   ```bash
   cargo update -p blogen
   cargo build
   ```

4. Commit, tag and push:

   ```bash
   git commit -am "release: v0.1.0"
   git tag -a v0.1.0 -m "blogen v0.1.0"
   git push origin main --tags
   ```

5. Verify the packaged crate, then publish:

   ```bash
   cargo package
   cargo publish            # requires a crates.io API token
   ```

6. Optional: attach prebuilt binaries to the GitHub release

   ```bash
   cargo build --profile dist
   ```

The blog repository deploys with
`cargo install blogen --locked --git https://github.com/SF-Zhou/blogen --tag v0.1.0`,
so the tag must be pushed for deployments to pick up a release.
