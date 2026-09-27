#!/usr/bin/env bash
# Drift check for the vendored tetra3rs copy.
#
# Answers two questions nothing else in the repo does:
#   1. Has our own tree drifted from what PATCHES.md claims? (hard failure)
#   2. Has upstream released something newer, and does it touch our patch
#      surface? (advisory — a human decides whether to sync)
#
# It never syncs: patch 3 rewrites the database container, so applying upstream
# blindly would change solver behaviour in a shipped product.
#
# Exit: 0 in sync | 1 undocumented local drift | 2 behind upstream.
# Portable to macOS bash 3.2 (no mapfile / associative arrays).
set -uo pipefail
cd "$(dirname "$0")/.."

PRISTINE=internal/reference/tetra3rs-main   # maintainer-only until the patch queue replaces this script
VENDORED=third_party/tetra3
PATCHES=$VENDORED/PATCHES.md

# Paths allowed to differ between the pristine snapshot and the vendored copy.
# Keep in lockstep with PATCHES.md.
ALLOWED_DIFF="Cargo.toml src/solver/database.rs src/solver/mod.rs src/solver/pattern_search.rs"
ALLOWED_EXTRA="PATCHES.md data src/solver/storage.rs"

in_list() { for w in $2; do [ "$1" = "$w" ] && return 0; done; return 1; }

fail=0

echo "== 1. local drift =================================================="
pinned=$(sed -n 's/^- Pinned tag: `\(v[0-9.]*\)`.*/\1/p' "$PATCHES" | head -1)
[ -n "$pinned" ] || { echo "FAIL: no pinned tag in $PATCHES"; exit 1; }
echo "pinned upstream tag: $pinned"

while IFS= read -r line; do
  case "$line" in
    "Only in $PRISTINE"*) continue ;;               # upstream-only, never vendored
    "Only in $VENDORED"*)
      p=$(printf '%s' "$line" | sed "s|^Only in $VENDORED/*||;s|: |/|;s|^/||")
      in_list "$p" "$ALLOWED_EXTRA" || { echo "DRIFT: undocumented extra path: $p"; fail=1; } ;;
    Files*)
      p=$(printf '%s' "$line" | sed "s|^Files $PRISTINE/||;s| and .*||")
      in_list "$p" "$ALLOWED_DIFF" || { echo "DRIFT: undocumented modified file: $p"; fail=1; } ;;
  esac
done <<EOF
$(diff -rq "$PRISTINE" "$VENDORED" 2>/dev/null)
EOF

if [ $fail -eq 0 ]; then
  echo "OK: tree matches PATCHES.md (4 patched files + storage.rs)"
fi

echo
echo "== 2. upstream releases ==========================================="
tags=$(curl -sf --max-time 20 https://api.github.com/repos/ssmichael1/tetra3rs/tags 2>/dev/null \
       | sed -n 's/.*"name": "\(v[0-9.]*\)".*/\1/p')
if [ -z "$tags" ]; then
  echo "SKIP: cannot reach github (offline?) — the drift check above still holds"
  exit $fail
fi
latest=$(printf '%s\n' "$tags" | head -1)
echo "latest upstream tag: $latest"
if [ "$latest" = "$pinned" ]; then
  echo "OK: in sync with upstream"
  exit $fail
fi
behind=$(printf '%s\n' "$tags" | awk -v p="$pinned" 'BEGIN{n=0} {if($0==p) exit; n++} END{print n}')
echo "BEHIND: $behind release(s) — $pinned -> $latest"

echo
echo "-- changelog entries newer than $pinned, flagged where they touch us --"
curl -sf --max-time 30 \
  https://raw.githubusercontent.com/ssmichael1/tetra3rs/main/CHANGELOG.md 2>/dev/null \
  | awk -v p="${pinned#v}" '/^## /{ if ($2==p) exit; seen=1 } seen' \
  | grep -E '^(- |\*\*)' \
  | while IFS= read -r line; do
      tag="    "
      case "$line" in
        *PatternEntry*|*save_to_file*|*load_from_file*|*to_bytes*|*validate*|*pattern_catalog*) tag="!!  " ;;
        *regenerat*|*Regenerat*) tag="!!  " ;;
        *SolveConfig*|*Breaking*|*breaking*) tag=" !  " ;;
      esac
      printf '%s%.150s\n' "$tag" "$line"
    done
echo
echo "!!  = touches the patch surface, or asks for a database regeneration"
echo " !  = new/changed config field, or a breaking change"
[ $fail -eq 1 ] && exit 1
exit 2
