# Releasing blogen

Releases are produced by `.github/workflows/release.yml`. A release publishes:

- four target builds (Linux x86_64/aarch64 glibc, macOS Intel/Apple Silicon),
  one `tar.gz` archive per target,
- `SHA256SUMS` and the `curl | sh` installer,
- GitHub build-provenance attestations for every archive,
- the crate on crates.io (via Trusted Publishing),
- a multi-arch container image on `ghcr.io`,
- floating `v<major>` and `v<major>.<minor>` tags for
  `uses: SF-Zhou/blogen@v0`-style action references.

## One-time setup

### Repository permissions

1. **Actions → General → Workflow permissions**: allow read and write
   permissions, or leave the default and rely on the explicit `permissions:`
   blocks in the workflow (they request `contents: write` where needed).
2. **Branch protection** on `main`: the release commit
   (`release: vX.Y.Z`) is pushed by the workflow. Allow `github-actions[bot]`
   to push, or configure the `RELEASE_TOKEN` secret (see below) with an account
   that can bypass the protection.
3. **Tag protection** (if enabled): the workflow pushes the release tag with
   the built-in `GITHUB_TOKEN` (so the tag push does not start a second
   release run). Allow `github-actions[bot]` to create tags, and allow the
   identity behind `RELEASE_TOKEN` to update the floating `v0` / `v0.2` tags.
   If tag protection forbids the built-in token, push the tag manually
   (Option B) instead of using the automatic release.
4. **GitHub Container Registry**: the first image push creates the
   `sf-zhou/blogen` package. Set its visibility to public in the package
   settings if you want unauthenticated pulls (this is what the documentation
   and `docker run` examples assume).

### Secrets

| Secret | Required | Purpose |
| --- | --- | --- |
| `RELEASE_TOKEN` | Optional | Fine-grained PAT with `contents: write` (and `workflows` if you edit workflows) used to push the release commit and bypass branch protection. Falls back to `GITHUB_TOKEN`. |

No crates.io token is stored: the crate is published with Trusted Publishing.

### crates.io Trusted Publishing

The `publish-crates` job uses GitHub OIDC to obtain a short-lived crates.io
token at release time, so there is no `CARGO_REGISTRY_TOKEN` secret to create or
rotate. Trusted Publishing is configured per existing crate; `blogen` must
already exist on crates.io (`0.1.0` was published manually), which it does.

1. On crates.io, open the `blogen` crate's settings and add a **Trusted
   Publisher** for GitHub with:
   - repository: `SF-Zhou/blogen`
   - workflow filename: `release.yml`
   - environment: leave empty, unless you want to require a GitHub environment
     (then add `environment: <name>` to the `publish-crates` job).
2. Keep `id-token: write` on the `publish-crates` job; the action
   (`rust-lang/crates-io-auth-action`) needs it to exchange the OIDC token.
3. If the version already exists on crates.io the job skips cleanly; otherwise
   it runs `cargo publish` with the temporary token, which is revoked when the
   job finishes.

If Trusted Publishing is not configured the job fails and the GitHub release
stays a draft, so the problem can be fixed and the release retried.

## Making a release

### Option A: workflow dispatch (recommended)

1. Make sure `main` is green and `CHANGELOG.md` has entries under
   `## [Unreleased]`.
2. Go to **Actions → Release → Run workflow** and choose:
   - `bump`: `patch`, `minor` or `major`. Use `none` when the version in
     `Cargo.toml` is already bumped and you just want to publish it — this is
     also the retry path for an existing tag. For the very first release,
     `none` publishes the version already in `Cargo.toml` (0.1.0) without
     bumping it.
   - `dry_run`: leave `false` for a real release; `true` builds and packages
     everything without tagging or publishing,
   - `publish_crates`: whether to publish to crates.io.
3. The workflow then:
   - verifies that `main` is up to date and `Cargo.lock` is tracked,
   - runs `scripts/release/release.py bump`, which updates `Cargo.toml` and
     moves the `[Unreleased]` changelog entries into
     `## [X.Y.Z] - <date>`, then refreshes `Cargo.lock` with `cargo metadata`,
   - creates and pushes `release: vX.Y.Z` and the annotated tag `vX.Y.Z`,
   - runs the test suite on the release commit,
   - builds, smoke-tests and packages all four targets,
   - creates a **draft** GitHub release with archives, `SHA256SUMS`, the
     installer and attestations,
   - obtains a short-lived crates.io token through GitHub OIDC and publishes
     the crate (unless the version already exists),
   - publishes the GitHub release (un-drafts it) only after the crate step
     succeeded (or was intentionally skipped),
   - pushes the container image and moves the floating tags.

The GitHub release is only published when every build and the crates.io step
succeeded (or crates.io was intentionally skipped), so a broken target never
results in a public half-release. A re-run resumes the tagged commit instead of
bumping again, and a release that is already published is left untouched.

### Option B: tag push

```sh
git switch main
git pull --ff-only
# bump Cargo.toml + CHANGELOG.md manually (or run the script below)
python3 scripts/release/release.py bump --part minor
cargo metadata --format-version 1 >/dev/null   # refresh Cargo.lock
git commit -am "release: v0.2.0"
git tag -a v0.2.0 -m "blogen v0.2.0"
git push origin main --follow-tags
```

Pushing the tag triggers the same pipeline. The workflow validates that the tag
matches `Cargo.toml` and the changelog, and that the tagged commit is on
`main`.

### Dry run

Dispatch the workflow with `dry_run: true` to validate the whole build matrix
without touching tags or publishing anything. The archives are available as
workflow artifacts.

### Pre-release versions

Use `python3 scripts/release/release.py set --version 0.3.0-rc.1` (then commit
and tag `v0.3.0-rc.1`), or dispatch with `bump: none` after bumping manually.
Pre-releases are published as GitHub pre-releases and get a container image
tagged with the exact pre-release version, but do not publish to crates.io and
do not move the container's floating tags or the floating action tags.

### GitHub Marketplace

`blogen` is listed at <https://github.com/marketplace/actions/install-blogen>.
Marketplace publication is a per-release checkbox in the GitHub web UI and
requires 2FA; it is not exposed by the REST/GraphQL APIs or `gh`, so the
workflow cannot set it. After the automated release is published, open
`https://github.com/SF-Zhou/blogen/releases/edit/vX.Y.Z`, check **Publish this
Action to the GitHub Marketplace**, and update the release. The generated
release notes already link to the listing.

## Manual checklist before releasing

The release workflow builds and tests the code, but two things are deliberately
not automated because they need the legacy generator checkout:

1. Full parity gate:

   ```sh
   scripts/verify/run.sh --candidate <blogen build dir> --screenshots
   ```

2. A final read of `CHANGELOG.md` (it becomes the GitHub release notes) and of
   the version number (`cargo pkgid` / `python3 scripts/release/release.py current`).

The usual local checks:

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
python3 -m unittest discover -s scripts/release -p 'test_*.py'
shellcheck scripts/install.sh
```

## Failure recovery

- **A target failed before the draft release was created**: fix the build,
  then re-run the failed jobs. If the tag was already pushed, re-run the
  workflow from the tag (Actions → Release → Re-run all jobs) or dispatch with
  `bump: none` — do not delete and recreate the tag.
- **The draft release exists but crates.io failed**: fix the problem, then
  re-run the failed jobs, or dispatch the workflow with `bump: none` to re-run
  the whole pipeline against the existing tag (the tag is reused, not moved).
  Re-running the crate step is safe: it skips a version that is already on
  crates.io.
- **The release was published but the Docker/floating-tag jobs failed**:
  re-run those jobs only. They are idempotent.
- **A bad release was published**: never move or delete a published tag and
  never republish the same version. Publish a new patch release with the fix.
  Delete the broken GitHub release and `cargo yank` the crate if necessary.
- **`tag already exists` when dispatching**: the release commit is already
  tagged. Re-run the workflow (it resumes the tagged commit) or dispatch with
  `bump: none`. Use a larger bump only when you intend to create a new
  version.

## Adding or removing a target

1. Add the target to the `matrix.include` list in
   `.github/workflows/release.yml` (choose a native runner whenever possible;
   cross-compiling the QuickJS and AWS-LC C sources is fragile). The Arm64
   Linux runners used here are available for public repositories; a private
   fork must replace them with self-hosted or larger runners.
2. Add the required packages to the job if the target needs them (NASM,
   cmake, `patch`, …).
3. `scripts/install.sh` detects OS/arch automatically; only update it if the
   new target is the default for a platform family.
4. Update `[package.metadata.binstall]` in `Cargo.toml` if the archive naming
   changes.
5. Update the platform table in `docs/install.md`.

## How the pieces fit together

| Path | Purpose |
| --- | --- |
| `.github/workflows/release.yml` | Release orchestration |
| `.github/workflows/ci.yml` | Tests, installer tests, packaging checks |
| `scripts/release/release.py` | Version bump, changelog and tag validation |
| `scripts/release/package.py` | Deterministic tar.gz archive creation |
| `scripts/release/checksums.py` | `SHA256SUMS` generation |
| `scripts/install.sh` | POSIX installer (Linux/macOS) |
| `action.yml` | Composite GitHub Action wrapper around the installer |
| `packaging/release-notes-footer.md` | Install instructions appended to release notes |
| `Dockerfile` | Container image around the Linux glibc binary |

## Verifying a published release

```sh
version=0.2.0
curl -fsSLO "https://github.com/SF-Zhou/blogen/releases/download/v${version}/SHA256SUMS"
sha256sum -c SHA256SUMS --ignore-missing
gh attestation verify "blogen-${version}-x86_64-unknown-linux-gnu.tar.gz" \
  --repo SF-Zhou/blogen
cargo binstall blogen --version "$version"
docker run --rm ghcr.io/sf-zhou/blogen:"$version" --version
```
