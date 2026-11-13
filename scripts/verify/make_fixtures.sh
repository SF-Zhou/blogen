#!/usr/bin/env bash
# Regenerate tests/fixtures/expected/ with the original generator.
#
# The oracle is built in a temporary staging copy of the blog (`git archive
# HEAD`), never in the blog checkout itself. See tests/fixtures/README.md.
#
# Usage:
#   scripts/verify/make_fixtures.sh              # regenerate expected/
#   scripts/verify/make_fixtures.sh --check      # check expected/ is up to date
#   scripts/verify/make_fixtures.sh --blog-dir DIR --fixtures-dir DIR
#
# Node 24 is required; nvm is loaded automatically when `node` is not on PATH.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

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
command -v node >/dev/null 2>&1 || { echo "make_fixtures: node not found (install Node 24)" >&2; exit 2; }

exec python3 "$SCRIPT_DIR/make_fixtures.py" "$@"
