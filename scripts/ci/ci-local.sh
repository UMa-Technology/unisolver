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

PKGS="-p unisolver-starmatch -p unisolver-core -p unisolver-synth -p unisolver-cabi -p namesgen -p solvecli -p xtask"
FEATURES="imageio satellites narrow"

step() {
  local name=$1; shift
  echo "==> $name"
  if ! "$@"; then
    echo "FAILED: $name" >&2
    exit 1
  fi
}

# third_party/tetra3 must equal the pinned upstream plus the patch queue; exit 2 (a newer
# upstream release exists) is reported but does not fail the gate
upstream_check() {
  cargo xtask upstream check
  case $? in 0|2) return 0 ;; *) return 1 ;; esac
}

step "public text" python3 scripts/ci/check_public_text.py
step "rust licenses" python3 scripts/ci/rust_licenses.py --check
step "rustfmt" cargo fmt $PKGS -p unisolver_frb -- --check
# --no-deps: lint our packages only, not the vendored workspace members (third_party/)
step "clippy" cargo clippy --no-deps $PKGS --all-targets --features "$FEATURES" -- -D warnings
step "tests" cargo test --workspace --release --features "$FEATURES"
step "windows cross-check" bash scripts/ci/check_windows.sh
step "upstream patch queue" upstream_check
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
