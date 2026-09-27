#!/usr/bin/env bash
# Cut a release: scripts/release.sh vX.Y.Z [--no-push]
# On a clean main that carries `cargo xtask release prepare X.Y.Z`: checks every manifest and the
# CHANGELOG against the tag, runs the local gate, tags vX.Y.Z (annotated) and pushes main and the
# tag to origin in one atomic push. GitHub receives both from scripts/sync-github.sh, and the tag
# there starts the release workflow. See docs/releasing.md.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
usage() { echo "usage: scripts/release.sh vX.Y.Z [--no-push]" >&2; exit 2; }
TAG="${1:-}"; PUSH=true
case "${2:-}" in "") ;; --no-push) PUSH=false ;; *) usage ;; esac
[[ "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || usage
branch=$(git branch --show-current)
[ "$branch" = main ] || { echo "error: release from main (on $branch)" >&2; exit 2; }
[ -z "$(git status --porcelain)" ] || { echo "error: the working tree is not clean" >&2; exit 2; }
if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
  echo "error: tag $TAG exists" >&2; exit 2
fi
# A main behind origin would be rejected only after the whole gate, leaving a local tag behind
if $PUSH; then
  git fetch -q origin main
  git merge-base --is-ancestor origin/main main \
    || { echo "error: origin/main has commits main lacks; pull first" >&2; exit 2; }
fi

echo "==> versions"
cargo xtask release check "$TAG" || exit 2
echo "==> gate"
scripts/ci/ci-local.sh
[ -z "$(git status --porcelain)" ] \
  || { echo "error: the gate changed tracked files:" >&2; git status --porcelain >&2; exit 1; }

echo "==> tag $TAG @ $(git rev-parse --short HEAD)"
git tag -a "$TAG" -m "unisolver $TAG"
if $PUSH; then
  # main and the tag in one atomic push: a tag on a commit that no pushed branch contains is
  # invisible to clones, and scripts/sync-github.sh (which follows origin/main) would skip it
  git push -q --atomic origin main "$TAG"
  echo "==> pushed main and $TAG to origin; next: scripts/sync-github.sh"
else
  echo "==> --no-push: tag $TAG is local only (delete it with git tag -d $TAG)"
fi
