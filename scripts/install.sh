#!/bin/sh
# blogen installer -- downloads a prebuilt binary from a GitHub release and
# verifies its SHA-256 checksum before installing it.
#
# Usage:
#   curl -LsSf https://github.com/SF-Zhou/blogen/releases/latest/download/blogen-installer.sh | sh
#   curl -LsSf .../blogen-installer.sh | sh -s -- --version 0.2.0
#
# Environment variables (flags win over them):
#   BLOGEN_INSTALL_DIR  installation directory
#   BLOGEN_VERSION      version to install ("latest" by default)
#   BLOGEN_TARGET       Rust target triple override
#   BLOGEN_BASE_URL     release base URL (mainly for tests)
#
# The script is POSIX sh and needs only curl (or wget) and tar.

set -eu

REPO="${BLOGEN_REPO:-SF-Zhou/blogen}"
PROJECT="blogen"
VERSION="${BLOGEN_VERSION:-latest}"
TARGET="${BLOGEN_TARGET:-}"
INSTALL_DIR="${BLOGEN_INSTALL_DIR:-}"
BASE_URL="${BLOGEN_BASE_URL:-https://github.com/${REPO}/releases}"
VERIFY=1
QUIET=0

say() {
    [ "$QUIET" -eq 1 ] || printf '%s\n' "$*"
}

warn() {
    printf 'blogen-install: warning: %s\n' "$*" >&2
}

err() {
    printf 'blogen-install: error: %s\n' "$*" >&2
    exit 1
}

usage() {
    cat <<'EOF'
Install the blogen static blog generator from a GitHub release.

Usage: blogen-installer.sh [OPTIONS]

Options:
  --version VERSION   Version to install, e.g. 0.2.0 or v0.2.0 (default: latest)
  --target TARGET     Rust target triple (default: detected from the OS/arch)
  --dir DIR           Installation directory (default: /usr/local/bin when
                      writable, otherwise ~/.local/bin)
  --base-url URL      Release base URL (default: the GitHub release page)
  --no-verify         Skip SHA-256 checksum verification (not recommended)
  --quiet             Only print errors
  -h, --help          Show this help

Supported targets:
  x86_64-unknown-linux-musl      x86_64-unknown-linux-gnu
  aarch64-unknown-linux-musl     aarch64-unknown-linux-gnu
  x86_64-apple-darwin            aarch64-apple-darwin

Environment:
  BLOGEN_INSTALL_DIR, BLOGEN_VERSION, BLOGEN_TARGET, BLOGEN_BASE_URL

Examples:
  curl -LsSf https://github.com/SF-Zhou/blogen/releases/latest/download/blogen-installer.sh | sh
  curl -LsSf .../blogen-installer.sh | sh -s -- --version 0.2.0 --dir "$HOME/bin"
EOF
}

# ---------------------------------------------------------------------------
# Argument parsing
# ---------------------------------------------------------------------------
while [ "$#" -gt 0 ]; do
    case "$1" in
        --version)
            [ "$#" -ge 2 ] || err "--version needs a value"
            VERSION="$2"
            shift 2
            ;;
        --version=*)
            VERSION="${1#*=}"
            shift
            ;;
        --target)
            [ "$#" -ge 2 ] || err "--target needs a value"
            TARGET="$2"
            shift 2
            ;;
        --target=*)
            TARGET="${1#*=}"
            shift
            ;;
        --dir)
            [ "$#" -ge 2 ] || err "--dir needs a value"
            INSTALL_DIR="$2"
            shift 2
            ;;
        --dir=*)
            INSTALL_DIR="${1#*=}"
            shift
            ;;
        --base-url)
            [ "$#" -ge 2 ] || err "--base-url needs a value"
            BASE_URL="$2"
            shift 2
            ;;
        --base-url=*)
            BASE_URL="${1#*=}"
            shift
            ;;
        --no-verify)
            VERIFY=0
            shift
            ;;
        --quiet|-q)
            QUIET=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        --)
            shift
            break
            ;;
        *)
            err "unknown argument: $1 (see --help)"
            ;;
    esac
done

# ---------------------------------------------------------------------------
# Tool detection
# ---------------------------------------------------------------------------
if command -v curl >/dev/null 2>&1; then
    CURL="curl"
elif command -v wget >/dev/null 2>&1; then
    CURL=""
else
    err "curl or wget is required"
fi

fetch() {
    url="$1"
    dest="$2"
    if [ -n "$CURL" ]; then
        curl -fsSL --retry 3 --connect-timeout 20 -o "$dest" "$url"
    else
        wget -q -O "$dest" "$url"
    fi
}

fetch_text() {
    url="$1"
    if [ -n "$CURL" ]; then
        curl -fsSL --retry 3 --connect-timeout 20 "$url"
    else
        wget -q -O - "$url"
    fi
}

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    elif command -v openssl >/dev/null 2>&1; then
        openssl dgst -sha256 "$1" | sed 's/^.*= //'
    else
        err "no SHA-256 tool found (sha256sum, shasum or openssl)"
    fi
}

# ---------------------------------------------------------------------------
# Platform detection
# ---------------------------------------------------------------------------
case "$(uname -s)" in
    Linux) OS="linux" ;;
    Darwin) OS="darwin" ;;
    *)
        err "unsupported operating system: $(uname -s); use 'cargo install blogen' instead"
        ;;
esac

if [ -z "$TARGET" ]; then
    case "$(uname -m)" in
        x86_64|amd64) ARCH="x86_64" ;;
        arm64|aarch64) ARCH="aarch64" ;;
        *)
            err "unsupported architecture: $(uname -m); use --target or 'cargo install blogen'"
            ;;
    esac
    if [ "$OS" = "linux" ]; then
        # Static musl binaries run on every Linux distribution, regardless of
        # the system glibc version. Pass --target ...-gnu to opt into glibc.
        TARGET="${ARCH}-unknown-linux-musl"
    else
        TARGET="${ARCH}-apple-darwin"
    fi
fi

case "$TARGET" in
    *-unknown-linux-musl|*-unknown-linux-gnu|*-apple-darwin) ;;
    *) warn "installing an unusual target: $TARGET" ;;
esac

# ---------------------------------------------------------------------------
# Resolve the version
# ---------------------------------------------------------------------------
BASE_URL="${BASE_URL%/}"

if [ "$VERSION" = "latest" ]; then
    latest_url="${BASE_URL}/latest"
    if [ -n "$CURL" ]; then
        # Follow the /releases/latest redirect instead of using the REST API,
        # which is rate limited and needs a token on busy CI IPs.
        final_url="$(curl -fsSL --retry 3 --connect-timeout 20 -o /dev/null \
            -w '%{url_effective}' "$latest_url")" \
            || err "could not resolve the latest release from $latest_url"
    else
        final_url="$(fetch_text "https://api.github.com/repos/${REPO}/releases/latest" \
            | sed -n 's/.*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p')" \
            || err "could not resolve the latest release"
    fi
    tag="${final_url##*/}"
    case "$tag" in
        v[0-9]*) VERSION="${tag#v}" ;;
        *) err "could not determine the latest version (got '$tag'); pass --version" ;;
    esac
fi

VERSION="${VERSION#v}"
case "$VERSION" in
    *[!0-9A-Za-z.-]*|'')
        err "invalid version: $VERSION (expected something like 0.2.0)"
        ;;
esac

say "Installing blogen ${VERSION} for ${TARGET}..."

# ---------------------------------------------------------------------------
# Download, verify and extract
# ---------------------------------------------------------------------------
TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/blogen-install.XXXXXX")"
cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup 0 1 2 3 15

ARCHIVE="${PROJECT}-${VERSION}-${TARGET}.tar.gz"
DOWNLOAD_URL="${BASE_URL}/download/v${VERSION}/${ARCHIVE}"

say "Downloading ${DOWNLOAD_URL}"
fetch "$DOWNLOAD_URL" "${TMP_DIR}/${ARCHIVE}" \
    || err "download failed: ${DOWNLOAD_URL}"

if [ "$VERIFY" -eq 1 ]; then
    CHECKSUMS_URL="${BASE_URL}/download/v${VERSION}/SHA256SUMS"
    fetch "$CHECKSUMS_URL" "${TMP_DIR}/SHA256SUMS" \
        || err "could not download ${CHECKSUMS_URL}"
    expected="$(
        awk -v file="$ARCHIVE" '{
            name = $2
            sub(/^\*/, "", name)
            if (name == file) { print $1; exit }
        }' "${TMP_DIR}/SHA256SUMS"
    )"
    [ -n "$expected" ] || err "no checksum for ${ARCHIVE} in SHA256SUMS"
    actual="$(sha256_file "${TMP_DIR}/${ARCHIVE}")"
    if [ "$expected" != "$actual" ]; then
        err "checksum mismatch for ${ARCHIVE}: expected ${expected}, got ${actual}"
    fi
    say "Checksum verified."
else
    warn "skipping checksum verification (--no-verify)"
fi

tar -xzf "${TMP_DIR}/${ARCHIVE}" -C "$TMP_DIR" blogen \
    || err "could not extract ${ARCHIVE}"
if [ ! -f "${TMP_DIR}/blogen" ] || [ -L "${TMP_DIR}/blogen" ]; then
    err "archive does not contain a regular blogen binary"
fi

# ---------------------------------------------------------------------------
# Install
# ---------------------------------------------------------------------------
if [ -z "$INSTALL_DIR" ]; then
    if [ "$(id -u 2>/dev/null || printf 1)" = "0" ]; then
        INSTALL_DIR=/usr/local/bin
    elif [ -d /usr/local/bin ] && [ -w /usr/local/bin ]; then
        INSTALL_DIR=/usr/local/bin
    else
        INSTALL_DIR="${HOME}/.local/bin"
    fi
fi

mkdir -p "$INSTALL_DIR" 2>/dev/null || err "could not create ${INSTALL_DIR}"
[ -w "$INSTALL_DIR" ] || err "install directory is not writable: ${INSTALL_DIR}
Re-run with --dir \"\$HOME/.local/bin\" or use sudo."

DEST="${INSTALL_DIR}/blogen"
if command -v install >/dev/null 2>&1; then
    install -m 0755 "${TMP_DIR}/blogen" "$DEST" || err "could not install to ${DEST}"
else
    cp "${TMP_DIR}/blogen" "${DEST}.tmp.$$" || err "could not install to ${DEST}"
    chmod 0755 "${DEST}.tmp.$$" || err "could not install to ${DEST}"
    mv -f "${DEST}.tmp.$$" "$DEST" || err "could not install to ${DEST}"
fi

say "Installed to ${DEST}"
installed_version="$("$DEST" --version 2>/dev/null || true)"
[ -n "$installed_version" ] && say "$installed_version"

case ":${PATH}:" in
    *":${INSTALL_DIR}:"*) ;;
    *)
        say ""
        say "${INSTALL_DIR} is not on your PATH. Add it with:"
        say "  export PATH=\"${INSTALL_DIR}:\$PATH\""
        ;;
esac
