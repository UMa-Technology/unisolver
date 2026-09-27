#!/usr/bin/env bash
# Fast-forward the github branch to origin/main (or a commit on it) and push it to GitHub's main,
# with the v* tags. Work happens on develop and lands on main; github is a delayed snapshot of
# main, and only this script moves it or pushes to GitHub. See docs/releasing.md.
#   scripts/sync-github.sh              # github := origin/main
#   scripts/sync-github.sh <commit>     # github := <commit> (must be on origin/main)
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
git remote get-url github >/dev/null 2>&1 || {
  echo "error: no github remote (git remote add github git@github.com:UMa-Technology/unisolver.git)" >&2
  exit 2
}
[ "$(git branch --show-current)" != github ] || { echo "error: switch off the github branch first" >&2; exit 2; }
git fetch -q origin
TARGET="${1:-origin/main}"
git merge-base --is-ancestor "$TARGET" origin/main || { echo "error: $TARGET is not on origin/main" >&2; exit 2; }
OLD=$(git rev-parse -q --verify --short github 2>/dev/null || echo '(none)')
echo "==> github: $OLD -> $(git rev-parse --short "$TARGET")"
[ "$OLD" = '(none)' ] || git log --oneline -50 "github..$TARGET"
git branch -f --no-track github "$TARGET"
git push -q origin github
git push github github:main
if [ -n "$(git tag -l 'v*')" ]; then
  git push github 'refs/tags/v*:refs/tags/v*'
fi
echo "==> GitHub main is at $(git rev-parse --short github)"
