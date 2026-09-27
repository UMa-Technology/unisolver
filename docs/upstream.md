# Upstream: the vendored tetra3rs

`third_party/tetra3` is a generated tree: upstream [tetra3rs](https://github.com/ssmichael1/tetra3rs)
at the commit pinned in `third_party/tetra3.lock`, filtered to the lock's `include` paths, with
the patches in `third_party/tetra3-patches/` applied in `series` order. The only other entry is
`data/`, upstream's test-data cache (its `gaia_merged.bin` is also the star catalog the bundled
tier is built from). What each patch changes and why:
[third_party/tetra3-patches/README.md](../third_party/tetra3-patches/README.md).

## Files

| Path | Contents |
|---|---|
| `third_party/tetra3.lock` | `repo`, `tag`, `commit` (the tag's commit, full SHA), `include` |
| `third_party/tetra3-patches/series` | patch file names, in order |
| `third_party/tetra3-patches/*.patch` | `git format-patch` output, one commit each |
| `target/upstream/cache` | clone of upstream that `check` resets freely (not tracked) |
| `target/upstream/work` | clone where the queue is edited, branch `unisolver` (not tracked) |

Set `UNISOLVER_TETRA3_REPO` to fetch upstream from a mirror instead of the lock's `repo`. Only
the first run needs the network; after that `check` works offline from `target/upstream/cache`.

## Commands

| Command | What it does |
|---|---|
| `cargo xtask upstream check` | Rebuilds the vendored tree from the lock and the queue and compares it byte for byte (`data/` excluded), then lists upstream releases newer than the lock, flagging changelog entries that reach the patch surface (`!!`) or change config (` !`). Exit 0 in sync, 1 local drift, 2 behind upstream, 3 could not verify |
| `cargo xtask upstream edit [--force]` | Rebuilds branch `unisolver` in `target/upstream/work`: the locked commit plus one commit per patch. Refuses to drop unexported commits or uncommitted changes unless `--force` |
| `cargo xtask upstream export` | Writes branch `unisolver` back: its tree (filtered) replaces `third_party/tetra3` except `data/`, its commits become the patch files and `series` |
| `cargo xtask upstream sync <tag>` | Rebuilds the branch, rebases it onto `<tag>`, exports, updates the lock, runs `cargo test -p tetra3` and the workspace tests (`--no-test` skips them), and prints the upstream changelog with flags plus a checklist. Stops on a rebase conflict: resolve it in the work clone, `git rebase --continue`, then `sync --continue` (or `sync --abort`) |

## Changing a patch

```bash
cargo xtask upstream edit
cd target/upstream/work
git rebase -i <locked tag>     # to change an existing patch: mark it `edit`, amend, continue
git commit                     # to add a patch: commit on top; the subject becomes its file name
cd -
cargo xtask upstream export
cargo xtask upstream check
```

Patches may change only paths in `include`; `export` refuses anything else. Exported patches carry
a fixed author, `unisolver <patches@unisolver.invalid>`, so no maintainer address is published.
Describe a new patch in `third_party/tetra3-patches/README.md`.

## Syncing to a new upstream release

```bash
cargo xtask upstream sync v0.14.0
```

A sync is a reviewed change, never automatic: patch 0003 replaces the pattern-table container, so
an upstream release can change solver behavior or the database format. Before committing, go
through the checklist the command prints: entries marked `!!` (does `storage.rs`'s compile-time
`PatternEntry` layout guard still hold, must the database tiers be regenerated?), the
"Sync history" line in the queue's README, and a CHANGELOG entry. Commit as
`chore(tetra3): sync to v0.14.0`.

## Rules

- Never edit `third_party/tetra3` by hand. `check` runs in `scripts/ci/ci-local.sh` and CI and
  fails on any byte the queue does not produce.
- A newer upstream release is reported (exit 2) but never applied without a `sync`.
