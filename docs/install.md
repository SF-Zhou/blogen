# Installing blogen

`blogen` ships as a single self-contained executable. There is no runtime
dependency: the default theme, KaTeX, Prism and the Mermaid renderer are
compiled into the binary.

## Supported platforms

| Operating system | Architecture | Rust target | Notes |
| --- | --- | --- | --- |
| Linux | x86_64 | `x86_64-unknown-linux-gnu` | Built on Ubuntu 22.04 (glibc ≥ 2.35) |
| Linux | aarch64 | `aarch64-unknown-linux-gnu` | Built on Ubuntu 22.04 (glibc ≥ 2.35) |
| macOS | Apple Silicon | `aarch64-apple-darwin` | macOS 11 or newer |
| macOS | Intel | `x86_64-apple-darwin` | macOS 11 or newer |

The Linux binaries are linked against glibc 2.35 (the version shipped with
Ubuntu 22.04), so they run on Ubuntu 22.04+, Debian 12+ and other
distributions with glibc 2.35 or newer. On older systems, build from source
with `cargo install blogen`.

## Install script (Linux and macOS)

```sh
curl -LsSf https://github.com/SF-Zhou/blogen/releases/latest/download/blogen-installer.sh | sh
```

Pin a version for reproducible builds:

```sh
curl -LsSf https://github.com/SF-Zhou/blogen/releases/download/v0.1.0/blogen-installer.sh \
  | sh -s -- --version 0.1.0
```

The script detects the OS/architecture, downloads the matching archive,
verifies its SHA-256 checksum against the release's `SHA256SUMS`, and installs
to `/usr/local/bin` when writable, otherwise `~/.local/bin`.

Useful options (see `--help` for the full list):

| Option | Description |
| --- | --- |
| `--version VERSION` | Install a specific version (`latest` by default) |
| `--dir DIR` | Install directory |
| `--target TARGET` | Override the Rust target triple |
| `--base-url URL` | Release base URL (advanced/testing) |
| `--no-verify` | Skip checksum verification (not recommended) |
| `--quiet` | Only print errors |

Environment variables: `BLOGEN_INSTALL_DIR`, `BLOGEN_VERSION`, `BLOGEN_TARGET`
and `BLOGEN_BASE_URL`.

## GitHub Actions

The repository is also a composite action that installs the prebuilt binary and
adds it to `PATH`:

```yaml
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: SF-Zhou/blogen@v0
      - run: blogen build

      # Optional: pin a version and install directory
      # - uses: SF-Zhou/blogen@v0
      #   with:
      #     version: 0.1.0
      #     bin-dir: ${{ runner.temp }}/bin
```

Inputs: `version` (default `latest`), `bin-dir` (default a user-local
directory), `target` and `base-url` (advanced). The installed version is
exposed as the step output `version`.

The action is listed on the [GitHub Marketplace](https://github.com/marketplace/actions/install-blogen).

## Other CI systems

The install script is POSIX `sh` and only needs `curl` (or `wget`), `tar` and
a SHA-256 tool (`sha256sum`, `shasum` or `openssl`), so it works in any
glibc-based Linux container or on macOS. Install into a project-local directory
so the pipeline needs no root access:

```sh
curl -LsSf https://github.com/SF-Zhou/blogen/releases/download/v0.1.0/blogen-installer.sh \
  | sh -s -- --version 0.1.0 --dir "$PWD/.bin"
export PATH="$PWD/.bin:$PATH"
blogen build
```

The Linux binary is dynamically linked against glibc 2.35, so use a glibc-based
image (Debian 12+, Ubuntu 22.04 or newer).

## Docker

A multi-arch image (`linux/amd64` and `linux/arm64`) is published to GitHub
Container Registry:

```sh
docker run --rm -v "$PWD:/site" ghcr.io/sf-zhou/blogen:latest build
```

The working directory is `/site` and the entrypoint is `blogen`, so any
sub-command works:

```sh
docker run --rm -p 9000:9000 -v "$PWD:/site" ghcr.io/sf-zhou/blogen:0.1.0 serve --host 0.0.0.0 --port 9000
```

When the host directory is not owned by root, pass your uid/gid so generated
files stay writable:

```sh
docker run --rm -u "$(id -u):$(id -g)" -v "$PWD:/site" ghcr.io/sf-zhou/blogen:latest build
```

## Cargo

From crates.io (builds from source, needs Rust 1.95 or newer):

```sh
cargo install blogen --locked
```

With [`cargo-binstall`](https://github.com/cargo-bins/cargo-binstall), which
downloads the prebuilt release archive instead of compiling:

```sh
cargo binstall blogen
```

On Linux, `cargo binstall` installs the glibc binary (requires glibc ≥ 2.35).

## Direct downloads

Every release attaches one archive per target, named
`blogen-<version>-<target>.tar.gz`, with the executable at the root of the
archive:

```sh
version=0.1.0
target=x86_64-unknown-linux-gnu
curl -fsSLO "https://github.com/SF-Zhou/blogen/releases/download/v${version}/blogen-${version}-${target}.tar.gz"
tar -xzf "blogen-${version}-${target}.tar.gz"
install -m 0755 blogen ~/.local/bin/blogen
```

Archives also contain `README.md`, `CHANGELOG.md`, the two license files and
`BUILD_INFO.txt` (commit, Rust version and build timestamp).

## Verifying a download

Each release publishes `SHA256SUMS` in `sha256sum` format:

```sh
curl -fsSLO "https://github.com/SF-Zhou/blogen/releases/download/v0.1.0/SHA256SUMS"
sha256sum -c SHA256SUMS --ignore-missing
# macOS: shasum -a 256 -c SHA256SUMS
```

The release workflow also generates [build provenance
attestations](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations)
for every archive. With the GitHub CLI:

```sh
gh attestation verify blogen-0.1.0-x86_64-unknown-linux-gnu.tar.gz \
  --repo SF-Zhou/blogen
```

## Uninstalling

- Install script: remove the `blogen` binary from the install directory.
- Cargo: `cargo uninstall blogen`.
- Docker: `docker rmi ghcr.io/sf-zhou/blogen`.

## Building from source

```sh
git clone https://github.com/SF-Zhou/blogen
cd blogen
cargo build --profile dist   # target/dist/blogen
cargo install --path . --locked
```

The release profile uses fat LTO, so the build takes a few minutes. The binary
is roughly 35 MB because the Mermaid renderer is compiled in.
