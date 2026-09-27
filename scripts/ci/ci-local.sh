#!/usr/bin/env bash
# The whole local gate, in the order CI runs it. This script is the source of truth for
# what the gate checks; .github/workflows/ci.yml mirrors it.
#
#   scripts/ci/ci-local.sh            # everything
#   scripts/ci/ci-local.sh --no-flutter
#
# Stops at the first failing step and names it.
set -uo pipefail
cd "$(dirname "$0")/../.."

FLUTTER=1
for a in "$@"; do
  case "$a" in
    --no-flutter) FLUTTER=0 ;;
    *) echo "usage: scripts/ci/ci-local.sh [--no-flutter]" >&2; exit 2 ;;
  esac
done

PKGS="-p unisolver-core -p unisolver-synth -p unisolver-cabi -p namesgen -p solvecli"
FEATURES="imageio satellites"

step() {
  local name=$1; shift
  echo "==> $name"
  if ! "$@"; then
    echo "FAILED: $name" >&2
    exit 1
  fi
}

step "public text" python3 scripts/ci/check_public_text.py
step "rust licenses" python3 scripts/ci/rust_licenses.py --check
step "rustfmt" cargo fmt $PKGS -p unisolver_frb -- --check
step "clippy" cargo clippy $PKGS --all-targets --features "$FEATURES" -- -D warnings
step "tests" cargo test --workspace --release --features "$FEATURES"
step "windows cross-check" bash scripts/ci/check_windows.sh
# Maintainers link the internal tree, which adds its own steps
if [ -x internal/scripts/ci.sh ]; then
  step "internal" internal/scripts/ci.sh
fi

if [ $FLUTTER -eq 1 ]; then
  if command -v flutter >/dev/null; then
    step "plugin tests" bash -c "cd packages/unisolver_flutter && flutter test"
    step "plugin analyze" bash -c "cd packages/unisolver_flutter && flutter analyze --no-pub --no-fatal-infos lib test"
    step "example tests" bash -c "cd packages/unisolver_flutter/example && flutter test test/"
    step "example analyze" bash -c "cd packages/unisolver_flutter/example && flutter analyze --no-pub lib test integration_test"
  else
    echo "==> flutter not on PATH: skipped the Flutter steps (use --no-flutter to silence)"
  fi
fi
echo "==> all green"
