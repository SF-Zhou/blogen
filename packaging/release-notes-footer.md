
## Installation

Also available on the [GitHub Marketplace](https://github.com/marketplace/actions/install-blogen).

Linux and macOS:

```sh
curl -LsSf https://github.com/SF-Zhou/blogen/releases/latest/download/blogen-installer.sh | sh
```

Pin a version (recommended for reproducible CI):

```sh
curl -LsSf https://github.com/SF-Zhou/blogen/releases/download/v@@VERSION@@/blogen-installer.sh | sh -s -- --version @@VERSION@@
```

GitHub Actions:

```yaml
- uses: SF-Zhou/blogen@v0
- run: blogen build
```

Cargo:

```sh
cargo binstall blogen   # prebuilt binary
cargo install blogen    # build from source
```

Docker:

```sh
docker run --rm -v "$PWD:/site" ghcr.io/sf-zhou/blogen:@@VERSION@@ build
```

All download targets, checksum verification and `gh attestation verify` instructions
are documented in the [installation guide](https://github.com/SF-Zhou/blogen/blob/main/docs/install.md).
