#!/usr/bin/env bash
# End-to-end parity gate for blogen.
#
# NOTE: this was the one-time migration gate for replacing the legacy blog
# generator with blogen. The blog repository has been migrated, so the harness
# is kept for auditing/reproducing the migration result. The baseline is built
# from the pinned legacy generator commit (auto-detected; see make_baseline.sh)
# and still works with an existing .cache/baseline.
#
# Steps (each can be skipped with flags):
#   1. comparer self-test            (compare_sites.py --self-test)
#   2. Node baseline build           (make_baseline.sh, only when absent or --rebuild-baseline)
#   3. fixture oracle staleness      (make_fixtures.sh --check, only with --fixtures-check)
#   4. site comparison               (compare_sites.py, only with --candidate)
#   5. screenshots                   (screenshots.mjs, only with --screenshots)
#
# Usage:
#   scripts/verify/run.sh --candidate <blogen-output-dir>
#   scripts/verify/run.sh --candidate <dir> --screenshots
#   scripts/verify/run.sh --rebuild-baseline --candidate <dir> --pages index.html,rust/x.html
#   scripts/verify/run.sh                              # baseline build + self-test only
#
# The candidate directory is either the blogen site root (contains index.html)
# or a build directory containing public/index.html (+ profile/README.md).
#
# Options:
#   --candidate DIR       blogen output directory (site root or dir containing public/)
#   --baseline DIR        baseline dir (default: scripts/verify/.cache/baseline)
#   --blog-dir DIR        read-only legacy blog checkout (default: ../blog next to blogen)
#   --rebuild-baseline    force regeneration of the Node baseline
#   --bootstrap-node-modules  allow npm to re-install the frozen legacy Node
#                         dependency set when node_modules is gone (network)
#   --screenshots         run the screenshot/pixel-diff step
#   --pages LIST          comma-separated page paths for screenshots
#   --schemes LIST        comma-separated screenshot schemes (default: light,dark)
#   --fixtures-check      verify tests/fixtures/expected matches the Node oracle
#   --no-selftest         skip the comparer self-test
#   --verbose             verbose comparison output
#   --json                machine-readable compare report on stdout
#   --quiet               minimal output
#
# Exit codes: 0 = pass, 1 = parity/visual mismatch or stale fixtures,
# 2 = usage/infrastructure error (e.g. Node unavailable, baseline missing).
#
# All build artifacts live under scripts/verify/.cache/ (git-ignored); the blog
# checkout is only read (git archive + its node_modules).

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
DEFAULT_BLOG_DIR="$(cd "$REPO_ROOT/.." && pwd)/blog"

BASELINE_DIR="$SCRIPT_DIR/.cache/baseline"
BLOG_DIR="${BLOG_DIR:-$DEFAULT_BLOG_DIR}"
CANDIDATE=""
REBUILD_BASELINE=0
BOOTSTRAP_NODE_MODULES=0
SCREENSHOTS=0
PAGES=""
SCHEMES=""
FIXTURES_CHECK=0
SELFTEST=1
VERBOSE=""
JSON=0
QUIET=0

usage() {
  sed -n '2,40p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
  exit "${1:-0}"
}
die() { printf 'run: %s\n' "$*" >&2; exit 2; }
say() { if [ "$QUIET" -eq 0 ]; then printf '%s\n' "$*"; fi; }

while [ $# -gt 0 ]; do
  case "$1" in
    --candidate) [ $# -ge 2 ] || die "--candidate needs a value"; CANDIDATE="$2"; shift 2 ;;
    --baseline) [ $# -ge 2 ] || die "--baseline needs a value"; BASELINE_DIR="$2"; shift 2 ;;
    --blog-dir) [ $# -ge 2 ] || die "--blog-dir needs a value"; BLOG_DIR="$2"; shift 2 ;;
    --rebuild-baseline|--force-baseline) REBUILD_BASELINE=1; shift ;;
    --bootstrap-node-modules) BOOTSTRAP_NODE_MODULES=1; shift ;;
    --screenshots) SCREENSHOTS=1; shift ;;
    --pages) [ $# -ge 2 ] || die "--pages needs a value"; PAGES="$2"; shift 2 ;;
    --schemes) [ $# -ge 2 ] || die "--schemes needs a value"; SCHEMES="$2"; shift 2 ;;
    --fixtures-check) FIXTURES_CHECK=1; shift ;;
    --no-selftest) SELFTEST=0; shift ;;
    --verbose|-v) VERBOSE="--verbose"; shift ;;
    --json) JSON=1; shift ;;
    --quiet|-q) QUIET=1; shift ;;
    -h|--help) usage 0 ;;
    *) die "unknown argument: $1 (try --help)" ;;
  esac
done

# Node is needed for the baseline build and screenshots.
ensure_node() {
  if command -v node >/dev/null 2>&1; then return 0; fi
  for nvm_sh in "${NVM_DIR:-$HOME/.nvm}/nvm.sh" "$HOME/.nvm/nvm.sh"; do
    if [ -s "$nvm_sh" ]; then
      # shellcheck disable=SC1090
      . "$nvm_sh" >/dev/null 2>&1 || true
      nvm use --silent 24 >/dev/null 2>&1 || true
      break
    fi
  done
  command -v node >/dev/null 2>&1
}

command -v python3 >/dev/null 2>&1 || die "python3 not found (compare/screenshots need it)"
PY_OK=1
python3 - <<'PY' >/dev/null 2>&1 || PY_OK=0
import PIL  # noqa: F401
import numpy  # noqa: F401
PY
if [ "$PY_OK" -eq 0 ] && [ "$SCREENSHOTS" -eq 1 ]; then
  die "PIL+numpy are required for screenshot pixel diffs (pip install pillow numpy)"
fi

worst_rc=0
record_rc() { if [ "$1" -gt "$worst_rc" ]; then worst_rc="$1"; fi; }

# ---------------------------------------------------------------------------
# 1. comparer self-test
# ---------------------------------------------------------------------------
if [ "$SELFTEST" -eq 1 ]; then
  say "== comparer self-test"
  python3 "$SCRIPT_DIR/compare_sites.py" --self-test
  rc=$?
  record_rc "$rc"
  [ "$rc" -eq 0 ] || printf 'run: comparer self-test FAILED\n' >&2
fi

# ---------------------------------------------------------------------------
# 2. baseline
# ---------------------------------------------------------------------------
if [ "$REBUILD_BASELINE" -eq 1 ] || [ ! -f "$BASELINE_DIR/manifest.json" ] || [ ! -f "$BASELINE_DIR/public/index.html" ]; then
  ensure_node || die "node not found; cannot build the Node baseline (install Node 24 or set NODE_BIN)"
  say "== building Node baseline"
  baseline_args=(--blog-dir "$BLOG_DIR" --baseline "$BASELINE_DIR")
  [ "$BOOTSTRAP_NODE_MODULES" -eq 1 ] && baseline_args+=(--bootstrap-node-modules)
  [ "$QUIET" -eq 1 ] && baseline_args+=(--quiet)
  "$SCRIPT_DIR/make_baseline.sh" "${baseline_args[@]}"
  rc=$?
  record_rc "$rc"
  [ "$rc" -eq 0 ] || die "baseline build failed (exit $rc)"
else
  say "== Node baseline present: $BASELINE_DIR"
fi
if [ -f "$BASELINE_DIR/manifest.json" ] && [ "$QUIET" -eq 0 ]; then
  python3 - "$BASELINE_DIR/manifest.json" <<'PY'
import json, sys
with open(sys.argv[1]) as handle:
    manifest = json.load(handle)
source = manifest.get("source", {})
outputs = manifest.get("outputs", {})
print("   commit %s (%s), %d files, %d visible + %d hidden article pages"
      % (str(source.get("commit", "?"))[:12], source.get("commit_subject", ""),
         outputs.get("public_files", 0), outputs.get("article_pages", 0),
         outputs.get("hidden_pages", 0)))
PY
fi

# ---------------------------------------------------------------------------
# 3. fixture oracle staleness (optional)
# ---------------------------------------------------------------------------
if [ "$FIXTURES_CHECK" -eq 1 ]; then
  ensure_node || die "node not found; cannot verify fixture oracle"
  say "== fixture oracle check"
  "$SCRIPT_DIR/make_fixtures.sh" --check --blog-dir "$BLOG_DIR"
  rc=$?
  record_rc "$rc"
  [ "$rc" -eq 0 ] || printf 'run: fixture oracle is stale (run scripts/verify/make_fixtures.sh)\n' >&2
fi

# ---------------------------------------------------------------------------
# 4. site comparison
# ---------------------------------------------------------------------------
if [ -n "$CANDIDATE" ]; then
  say "== comparing candidate: $CANDIDATE"
  compare_args=(--baseline "$BASELINE_DIR" --candidate "$CANDIDATE")
  [ -n "$VERBOSE" ] && compare_args+=("$VERBOSE")
  [ "$JSON" -eq 1 ] && compare_args+=(--json)
  python3 "$SCRIPT_DIR/compare_sites.py" "${compare_args[@]}"
  rc=$?
  record_rc "$rc"
else
  say "== no --candidate given, site comparison skipped"
  say "   build blogen output, then run: scripts/verify/run.sh --candidate <output-dir>"
fi

# ---------------------------------------------------------------------------
# 5. screenshots
# ---------------------------------------------------------------------------
if [ "$SCREENSHOTS" -eq 1 ]; then
  ensure_node || die "node not found; screenshots need puppeteer from the blog node_modules"
  say "== screenshots"
  shot_args=(--baseline "$BASELINE_DIR/public")
  if [ -n "$CANDIDATE" ]; then
    shot_candidate="$CANDIDATE"
    if [ -f "$CANDIDATE/public/index.html" ] && [ ! -f "$CANDIDATE/index.html" ]; then
      shot_candidate="$CANDIDATE/public"
    fi
    shot_args+=(--candidate "$shot_candidate")
  fi
  [ -n "$PAGES" ] && shot_args+=(--pages "$PAGES")
  [ -n "$SCHEMES" ] && shot_args+=(--schemes "$SCHEMES")
  [ "$QUIET" -eq 1 ] && shot_args+=(--quiet)
  node "$SCRIPT_DIR/screenshots.mjs" "${shot_args[@]}"
  rc=$?
  record_rc "$rc"
fi

say ""
if [ "$worst_rc" -eq 0 ]; then
  say "run: PASS"
else
  say "run: FAIL (exit $worst_rc)"
fi
exit "$worst_rc"
