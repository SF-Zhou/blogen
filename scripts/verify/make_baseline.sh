#!/usr/bin/env bash
# Rebuild the ORACLE baseline for blogen parity checks.
#
# The baseline is produced by the original generator (`SF-Zhou/blog`),
# built from a *pristine* `git archive` copy in a temporary directory. The blog
# checkout is never modified or built in: only `git` read commands and its
# legacy `node_modules` (read-only) are used.
#
# The blog repository has been migrated to blogen: its current HEAD no longer
# contains the legacy generator (`src/main.js`). This script therefore
# picks the source commit in this order:
#
#   1. `--ref REF` / `$BLOGEN_BASELINE_REF` (must contain `src/main.js`);
#   2. `HEAD`, when HEAD still contains the legacy generator;
#   3. the pinned legacy commit below (used after the migration commit);
#
# and exits with a clear error when none of them is available.
#
# Pinned legacy generator commit (last legacy blog build used by this gate):
LEGACY_BASELINE_REF="d4cb7aafad90f09cdcd85e2482eae739483743a5"
#
# Output (git-ignored):
#   scripts/verify/.cache/baseline/
#     public/            generated site (index.html, index.json, rss.xml, articles, dist/, images/)
#     profile/README.md  generated profile page source
#     config.json        frozen copy of the blog config used for the build
#     build.log          stdout/stderr of build.sh
#     manifest.json      source ref/commit, node/dep versions, output hashes
#
# Usage:
#   scripts/verify/make_baseline.sh [--blog-dir DIR] [--baseline DIR]
#                                   [--ref REF] [--node-modules DIR]
#                                   [--bootstrap-node-modules] [--quiet]
#
# Environment:
#   BLOG_DIR                    path to the read-only legacy blog checkout
#                               (default: ../../../blog next to the blogen repo)
#   BLOGEN_BASELINE_REF         legacy-generator commit/ref (default: auto, see above)
#   BLOGEN_BASELINE_NODE_MODULES  legacy node_modules directory
#                               (default: $BLOG_DIR/node_modules, else
#                               .cache/legacy-deps/node_modules)
#   BLOGEN_BASELINE_BOOTSTRAP=1  allow `npm ci --ignore-scripts` of the legacy
#                               package.json/package-lock.json into
#                               .cache/legacy-deps/node_modules when node_modules is gone
#   NODE_BIN                    optional node binary; nvm is loaded when `node`
#                               is not already on PATH (`nvm use 24`).
#
# The script is re-runnable: it always rebuilds into a temp dir and atomically
# replaces the baseline directory. Exit code: 0 = success, 1 = build failure,
# 2 = usage error (e.g. no legacy ref / node_modules available).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
DEFAULT_BLOG_DIR="$(cd "$REPO_ROOT/.." && pwd)/blog"

BLOG_DIR="${BLOG_DIR:-$DEFAULT_BLOG_DIR}"
BASELINE_DIR="$SCRIPT_DIR/.cache/baseline"
BASELINE_REF="${BLOGEN_BASELINE_REF:-}"
NODE_MODULES_DIR="${BLOGEN_BASELINE_NODE_MODULES:-}"
BOOTSTRAP_NODE_MODULES="${BLOGEN_BASELINE_BOOTSTRAP:-0}"
QUIET=0

die() { printf 'make_baseline: %s\n' "$*" >&2; exit 2; }
say() { if [ "$QUIET" -eq 0 ]; then printf 'make_baseline: %s\n' "$*"; fi; }

while [ $# -gt 0 ]; do
  case "$1" in
    --blog-dir) [ $# -ge 2 ] || die "--blog-dir needs a value"; BLOG_DIR="$2"; shift 2 ;;
    --baseline) [ $# -ge 2 ] || die "--baseline needs a value"; BASELINE_DIR="$2"; shift 2 ;;
    --ref) [ $# -ge 2 ] || die "--ref needs a value"; BASELINE_REF="$2"; shift 2 ;;
    --node-modules) [ $# -ge 2 ] || die "--node-modules needs a value"; NODE_MODULES_DIR="$2"; shift 2 ;;
    --bootstrap-node-modules) BOOTSTRAP_NODE_MODULES=1; shift ;;
    --force) shift ;;   # accepted for compatibility: the script always rebuilds
    --quiet|-q) QUIET=1; shift ;;
    -h|--help) sed -n '2,60p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "unknown argument: $1" ;;
  esac
done

[ -d "$BLOG_DIR" ] || die "blog checkout not found: $BLOG_DIR"
git -C "$BLOG_DIR" rev-parse --git-dir >/dev/null 2>&1 \
  || die "$BLOG_DIR is not a git repository; the baseline must be built from a git archive"

# ---------------------------------------------------------------------------
# Resolve the legacy generator commit.
# ---------------------------------------------------------------------------
HEAD_COMMIT="$(git -C "$BLOG_DIR" rev-parse HEAD)"
if git -C "$BLOG_DIR" cat-file -e "HEAD:src/main.js" 2>/dev/null; then
  HEAD_HAS_LEGACY=1
else
  HEAD_HAS_LEGACY=0
fi

RESOLVED_REF=""
RESOLVED_COMMIT=""
REF_SOURCE=""

ref_exists() { git -C "$BLOG_DIR" rev-parse --verify --quiet "$1^{commit}" >/dev/null 2>&1; }
ref_has_legacy_generator() { git -C "$BLOG_DIR" cat-file -e "$1:src/main.js" 2>/dev/null; }

if [ -n "$BASELINE_REF" ]; then
  ref_exists "$BASELINE_REF" || die "legacy baseline ref '$BASELINE_REF' not found in $BLOG_DIR.
  The blog HEAD was migrated to blogen, so the legacy generator only
  exists in older commits. Pass --ref <commit> or set BLOGEN_BASELINE_REF to a
  commit that contains src/main.js (pinned default: $LEGACY_BASELINE_REF), or
  fetch the full blog history first."
  ref_has_legacy_generator "$BASELINE_REF" || die "ref '$BASELINE_REF' does not contain src/main.js,
  so it is not a legacy generator commit. Use the pinned commit
  $LEGACY_BASELINE_REF or another commit that contains src/main.js."
  RESOLVED_REF="$BASELINE_REF"
  RESOLVED_COMMIT="$(git -C "$BLOG_DIR" rev-parse "$BASELINE_REF^{commit}")"
  REF_SOURCE="explicit"
elif [ "$HEAD_HAS_LEGACY" -eq 1 ]; then
  RESOLVED_REF="HEAD"
  RESOLVED_COMMIT="$HEAD_COMMIT"
  REF_SOURCE="HEAD"
  say "using legacy generator from HEAD ($(git -C "$BLOG_DIR" log -1 --pretty='%h %s' "$RESOLVED_COMMIT"))"
else
  say "HEAD no longer contains the legacy generator (src/main.js); blog migrated to blogen"
  ref_exists "$LEGACY_BASELINE_REF" || die "HEAD was migrated to blogen and the pinned legacy commit
  $LEGACY_BASELINE_REF is not present in $BLOG_DIR.
  Pass --ref <commit> or set BLOGEN_BASELINE_REF to a legacy generator
  commit (one that contains src/main.js), or fetch the full blog history first."
  ref_has_legacy_generator "$LEGACY_BASELINE_REF" || die "pinned legacy commit $LEGACY_BASELINE_REF has no src/main.js;
  set BLOGEN_BASELINE_REF=<legacy commit> to rebuild the baseline."
  RESOLVED_REF="$LEGACY_BASELINE_REF"
  RESOLVED_COMMIT="$LEGACY_BASELINE_REF"
  REF_SOURCE="pinned-legacy-fallback"
  say "using pinned legacy ref $LEGACY_BASELINE_REF"
fi
[ -n "$RESOLVED_COMMIT" ] || die "could not resolve a legacy generator commit"

CACHE_DIR="$SCRIPT_DIR/.cache"
mkdir -p "$CACHE_DIR"

# ---------------------------------------------------------------------------
# Legacy toolchain: build.sh needs node + npx and the old node_modules.
# ---------------------------------------------------------------------------
if [ -n "${NODE_BIN:-}" ]; then
  NODE_DIR="$(cd "$(dirname "$NODE_BIN")" && pwd)"
  PATH="$NODE_DIR:$PATH"
fi
if ! command -v node >/dev/null 2>&1; then
  for nvm_sh in "${NVM_DIR:-$HOME/.nvm}/nvm.sh" "$HOME/.nvm/nvm.sh"; do
    if [ -s "$nvm_sh" ]; then
      # shellcheck disable=SC1090
      . "$nvm_sh" >/dev/null 2>&1 || true
      nvm use --silent 24 >/dev/null 2>&1 || true
      break
    fi
  done
fi
command -v node >/dev/null 2>&1 || die "node not found; install Node 24 or set NODE_BIN"

usable_node_modules() {
  [ -n "${1:-}" ] && [ -d "$1" ] && [ -x "$1/.bin/lessc" ]
}

# An explicit --node-modules / BLOGEN_BASELINE_NODE_MODULES must be usable as-is.
if [ -n "$NODE_MODULES_DIR" ]; then
  usable_node_modules "$NODE_MODULES_DIR" || die "legacy node_modules at '$NODE_MODULES_DIR' is missing or has no .bin/lessc.
  It must be the old blog dependency tree (or a node_modules bootstrapped by
  --bootstrap-node-modules)."
fi

# Otherwise prefer the old blog node_modules, then a previously bootstrapped
# copy in .cache (which survives the post-migration deletion of node_modules).
if [ -z "$NODE_MODULES_DIR" ]; then
  for candidate in "$BLOG_DIR/node_modules" "$CACHE_DIR/legacy-deps/node_modules"; do
    if usable_node_modules "$candidate"; then
      NODE_MODULES_DIR="$candidate"
      break
    fi
  done
fi

if [ -z "$NODE_MODULES_DIR" ]; then
  if [ "$BOOTSTRAP_NODE_MODULES" -eq 1 ] || [ "${BLOGEN_BASELINE_BOOTSTRAP:-0}" = "1" ]; then
    command -v npm >/dev/null 2>&1 || die "npm not found; cannot bootstrap the legacy dependencies (install Node 24/npm first)"
    command -v python3 >/dev/null 2>&1 || die "python3 not found; cannot bootstrap the legacy dependencies"
    [ -f "$SCRIPT_DIR/legacy-deps.json" ] || die "scripts/verify/legacy-deps.json is missing; cannot bootstrap node_modules"
    say "legacy node_modules missing; bootstrapping the frozen legacy dependency set (npm --ignore-scripts, needs network)"
    BOOTSTRAP_ROOT="$(mktemp -d "$CACHE_DIR/.legacy-node-modules.XXXXXX")"
    BOOTSTRAP_TARGET="$CACHE_DIR/legacy-deps/node_modules"
    python3 - "$SCRIPT_DIR/legacy-deps.json" "$BOOTSTRAP_ROOT/package.json" <<'PYBOOTSTRAP'
import json
import sys

deps = json.load(open(sys.argv[1]))["packages"]
pkg = {
    "name": "blogen-legacy-baseline-deps",
    "private": True,
    "version": "1.0.0",
    "description": "frozen legacy dependencies for the blogen parity baseline",
    "dependencies": deps,
}
with open(sys.argv[2], "w") as handle:
    json.dump(pkg, handle, indent=2)
    handle.write("\n")
PYBOOTSTRAP
    if [ -f "$SCRIPT_DIR/legacy-package-lock.json" ]; then
      cp "$SCRIPT_DIR/legacy-package-lock.json" "$BOOTSTRAP_ROOT/package-lock.json"
      NPM_BOOTSTRAP_CMD=(npm ci --ignore-scripts --no-audit --no-fund --loglevel=error)
    else
      NPM_BOOTSTRAP_CMD=(npm install --ignore-scripts --no-audit --no-fund --loglevel=error)
    fi
    if ! ( cd "$BOOTSTRAP_ROOT" && "${NPM_BOOTSTRAP_CMD[@]}" ); then
      rm -rf "$BOOTSTRAP_ROOT"
      die "npm install of the frozen legacy dependencies failed; fix network/registry access or restore node_modules
  manually (see scripts/verify/legacy-deps.json) and pass --node-modules."
    fi
    rm -rf "$CACHE_DIR/legacy-deps"
    mkdir -p "$CACHE_DIR/legacy-deps"
    mv "$BOOTSTRAP_ROOT/node_modules" "$BOOTSTRAP_TARGET"
    cp "$BOOTSTRAP_ROOT/package.json" "$CACHE_DIR/legacy-deps/package.json" 2>/dev/null || true
    rm -rf "$BOOTSTRAP_ROOT"
    NODE_MODULES_DIR="$BOOTSTRAP_TARGET"
    say "legacy node_modules bootstrapped into $NODE_MODULES_DIR"
    say "(kept in .cache/legacy-deps/; delete that directory to force a re-install)"
  else
    die "legacy dependencies (node_modules) are gone: neither $BLOG_DIR/node_modules
  nor $CACHE_DIR/legacy-deps/node_modules exists.
  The baseline build runs the original pipeline (./build.sh, lessc,
  prismjs, katex), so it needs the legacy dependency tree. Options:
    - rebuild it once from the pinned ref: scripts/verify/make_baseline.sh --bootstrap-node-modules
      (runs npm ci --ignore-scripts into .cache/legacy-deps/node_modules; needs network)
    - or point --node-modules / BLOGEN_BASELINE_NODE_MODULES at a preserved copy.
  An already-built .cache/baseline keeps working without this; only rebuilding
  or refreshing the baseline needs the legacy dependencies."
  fi
fi
NODE_MODULES_DIR="$(cd "$NODE_MODULES_DIR" && pwd)"
say "legacy node_modules: $NODE_MODULES_DIR"

# ---------------------------------------------------------------------------
# Build in a scratch dir on the same filesystem, then swap the baseline in.
# ---------------------------------------------------------------------------
BUILD_ROOT="$(mktemp -d "$CACHE_DIR/.baseline-build.XXXXXX")"
cleanup() { rm -rf "$BUILD_ROOT"; }
trap cleanup EXIT

SRC_DIR="$BUILD_ROOT/src"
STAGE_DIR="$BUILD_ROOT/stage"
mkdir -p "$SRC_DIR" "$STAGE_DIR"

say "archiving legacy generator from $BLOG_DIR (ref: $RESOLVED_REF)"
git -C "$BLOG_DIR" archive "$RESOLVED_REF" | tar -x -C "$SRC_DIR"
# build.sh reads node_modules for lessc/prismjs/katex; a symlink keeps the
# baseline immutable and avoids copying ~440MB.
ln -s "$NODE_MODULES_DIR" "$SRC_DIR/node_modules"

COMMIT="$RESOLVED_COMMIT"
COMMIT_SUBJECT="$(git -C "$BLOG_DIR" log -1 --pretty=%s "$COMMIT")"
BRANCH="$(git -C "$BLOG_DIR" rev-parse --abbrev-ref HEAD 2>/dev/null || true)"
WORKTREE_DIRTY=0
if [ -n "$(git -C "$BLOG_DIR" status --porcelain --untracked-files=no 2>/dev/null)" ]; then
  WORKTREE_DIRTY=1
fi

NODE_BIN_PATH="$(command -v node)"
NODE_VERSION="$(node --version)"
say "building with Node $NODE_VERSION (commit ${COMMIT:0:10})"
BUILD_START="$(date +%s)"
if ! ( cd "$SRC_DIR" && ./build.sh ) >"$BUILD_ROOT/build.log" 2>&1; then
  printf 'make_baseline: build.sh failed; last 40 log lines:\n' >&2
  tail -n 40 "$BUILD_ROOT/build.log" >&2
  exit 1
fi
BUILD_END="$(date +%s)"
BUILD_SECONDS=$((BUILD_END - BUILD_START))

[ -f "$SRC_DIR/public/index.html" ] || die "build produced no public/index.html"
[ -f "$SRC_DIR/profile/README.md" ] || die "build produced no profile/README.md"

mv "$SRC_DIR/public" "$STAGE_DIR/public"
mv "$SRC_DIR/profile" "$STAGE_DIR/profile"
cp "$SRC_DIR/config.json" "$STAGE_DIR/config.json"
cp "$BUILD_ROOT/build.log" "$STAGE_DIR/build.log"

# ---------------------------------------------------------------------------
# manifest.json: exact provenance + output fingerprints.
# ---------------------------------------------------------------------------
python3 - "$BLOG_DIR" "$STAGE_DIR" "$COMMIT" "$COMMIT_SUBJECT" "$BRANCH" \
  "$WORKTREE_DIRTY" "$NODE_BIN_PATH" "$NODE_VERSION" "$BUILD_SECONDS" \
  "$RESOLVED_REF" "$REF_SOURCE" "$HEAD_COMMIT" "$HEAD_HAS_LEGACY" \
  "$NODE_MODULES_DIR" "$LEGACY_BASELINE_REF" "$SCRIPT_DIR" <<'PY'
import hashlib
import json
import os
import re
import subprocess
import sys

(blog_dir, stage_dir, commit, subject, branch, dirty, node_path, node_version,
 build_seconds, resolved_ref, ref_source, head_commit, head_has_legacy,
 node_modules_dir, legacy_ref, script_dir) = sys.argv[1:17]

DEP_NAMES = [
    "markdown-it", "markdown-it-anchor", "markdown-it-implicit-figures",
    "markdown-it-table-of-contents", "markdown-it-texmath", "katex", "prismjs",
    "md5", "mustache", "less", "download", "file-type", "puppeteer", "mermaid",
    "@mermaid-js/mermaid-cli",
]


def version_of(name):
    try:
        with open(os.path.join(node_modules_dir, name, "package.json")) as f:
            return json.load(f).get("version")
    except OSError:
        return None


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


RSS_LAST_BUILD_RE = re.compile(rb"<lastBuildDate>.*?</lastBuildDate>", re.DOTALL)
RSS_LAST_BUILD_STUB = b"<lastBuildDate>__LAST_BUILD_DATE__</lastBuildDate>"


def sha256_file_for_manifest(path, rel):
    """Hash file contents; normalise the volatile RSS build timestamp.

    rss.xml embeds the build time, so a plain hash would change on every run.
    Normalising it makes `outputs.public_sha256` a reproducible fingerprint of
    the generated site (everything else is deterministic).
    """
    if rel == "rss.xml":
        with open(path, "rb") as f:
            data = RSS_LAST_BUILD_RE.sub(RSS_LAST_BUILD_STUB, f.read())
        return hashlib.sha256(data).hexdigest()
    return sha256_file(path)


def tree_manifest(root):
    entries = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames.sort()
        for name in sorted(filenames):
            full = os.path.join(dirpath, name)
            if os.path.islink(full) or not os.path.isfile(full):
                continue
            rel = os.path.relpath(full, root).replace(os.sep, "/")
            digest = sha256_file_for_manifest(full, rel)
            size = os.path.getsize(full)
            if rel == "rss.xml":
                with open(full, "rb") as f:
                    size = len(RSS_LAST_BUILD_RE.sub(RSS_LAST_BUILD_STUB, f.read()))
            entries.append((rel, size, digest))
    combined = hashlib.sha256()
    for rel, size, digest in entries:
        combined.update(("%s\0%s\n" % (rel, digest)).encode())
    return entries, combined.hexdigest()


public = os.path.join(stage_dir, "public")
profile = os.path.join(stage_dir, "profile")
public_entries, public_hash = tree_manifest(public)
profile_entries, profile_hash = tree_manifest(profile)

article_pages = hidden_pages = 0
for rel, _size, _digest in public_entries:
    if rel.startswith("dist/") or "/" not in rel:
        continue
    if rel.endswith(".html"):
        article_pages += 1
    elif rel.endswith(".htm"):
        hidden_pages += 1

try:
    npm_version = subprocess.run(
        [os.path.join(os.path.dirname(node_path), "npm"), "--version"],
        capture_output=True, text=True, check=False).stdout.strip() or None
except OSError:
    npm_version = None

manifest = {
    "schema_version": 2,
    "generated_by": "scripts/verify/make_baseline.sh",
    "purpose": "parity oracle for blogen: site generated by the original pipeline",
    "source": {
        "repo": os.path.abspath(blog_dir),
        "ref": resolved_ref,
        "ref_source": ref_source,
        "commit": commit,
        "commit_subject": subject,
        "branch": branch,
        "head_commit": head_commit,
        "head_contains_legacy_generator": bool(int(head_has_legacy)),
        "pinned_legacy_ref": legacy_ref,
        "worktree_dirty_ignored": bool(int(dirty)),
        "archive": "git -C <blog> archive %s | tar -x -C <tmp>" % resolved_ref,
    },
    "generator": {
        "pipeline": "legacy generator (blog/src/main.js + blog/build.sh)",
        "command": "./build.sh",
        "node": node_version,
        "node_path": node_path,
        "npm": npm_version,
        "node_modules": os.path.abspath(node_modules_dir),
        "deps": {name: version_of(name) for name in DEP_NAMES},
        "legacy_deps_sha256": sha256_file(os.path.join(script_dir, "legacy-deps.json"))
        if os.path.isfile(os.path.join(script_dir, "legacy-deps.json")) else None,
        "legacy_package_lock_sha256": sha256_file(os.path.join(script_dir, "legacy-package-lock.json"))
        if os.path.isfile(os.path.join(script_dir, "legacy-package-lock.json")) else None,
    },
    "build": {"duration_seconds": int(build_seconds)},
    "frozen_files": {
        "config.json": sha256_file(os.path.join(stage_dir, "config.json")),
        "build.log": sha256_file(os.path.join(stage_dir, "build.log")),
    },
    "outputs": {
        "public_files": len(public_entries),
        "public_bytes": sum(size for _rel, size, _d in public_entries),
        "article_pages": article_pages,
        "hidden_pages": hidden_pages,
        "profile_files": len(profile_entries),
        "public_sha256": public_hash,
        "public_sha256_note": "rss.xml <lastBuildDate> normalised; everything else byte-identical",
        "profile_sha256": profile_hash,
    },
}

with open(os.path.join(stage_dir, "manifest.json"), "w") as f:
    json.dump(manifest, f, indent=2, sort_keys=False)
    f.write("\n")

print("  ref:      %s (%s)" % (resolved_ref, ref_source))
print("  commit:   %s (%s)" % (commit[:12], subject))
print("  node:     %s, deps: markdown-it %s, katex %s, prismjs %s, mermaid-cli %s" % (
    node_version, manifest["generator"]["deps"]["markdown-it"],
    manifest["generator"]["deps"]["katex"], manifest["generator"]["deps"]["prismjs"],
    manifest["generator"]["deps"]["@mermaid-js/mermaid-cli"]))
print("  output:   %d files (%d article, %d hidden), %.1f MiB, sha256 %s" % (
    len(public_entries), article_pages, hidden_pages,
    sum(size for _rel, size, _d in public_entries) / (1 << 20), public_hash[:16]))
PY

rm -rf "$BASELINE_DIR"
mv "$STAGE_DIR" "$BASELINE_DIR"
say "baseline ready: $BASELINE_DIR"
