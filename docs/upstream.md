# Upstreams: vendored crates as patch queues

One upstream project is vendored as a generated tree, managed by `cargo xtask upstream`:

| Name | Upstream | Vendored tree | Patches |
|---|---|---|---|
| `tetra3` | [tetra3rs](https://github.com/ssmichael1/tetra3rs) | `third_party/tetra3` | [third_party/tetra3-patches/README.md](../third_party/tetra3-patches/README.md) |

`third_party/<name>` is upstream at the commit pinned in `third_party/<name>.lock`, filtered to the
lock's `include` paths, with the patches in `third_party/<name>-patches/` applied in `series` order.
The only other entry is tetra3's `data/`, upstream's test-data cache (its `gaia_merged.bin` is also
the star catalog the bundled tier is built from). What each patch changes and why is in the queue's
README.

## Files

| Path | Contents |
|---|---|
| `third_party/<name>.lock` | `repo`, `tag`, `commit` (the tag's commit, full SHA; annotated tags peeled), `include` |
| `third_party/<name>-patches/series` | patch file names, in order |
| `third_party/<name>-patches/*.patch` | `git format-patch` output, one commit each |
| `target/upstream/<name>/cache` | clone of upstream that `check` resets freely (not tracked) |
| `target/upstream/<name>/work` | clone where the queue is edited, branch `unisolver` (not tracked) |

Set `UNISOLVER_TETRA3_REPO` to fetch from a mirror instead of the lock's `repo`. Only the first run
needs the network; after that `check` works offline from the cache clone.

## Commands

| Command | What it does |
|---|---|
| `cargo xtask upstream check` | For every upstream (or only `--name <name>`): rebuilds the vendored tree from the lock and the queue and compares it byte for byte (tetra3's `data/` excluded), then lists upstream releases newer than the lock, flagging changelog entries that reach the patch surface (`!!`) or change config (` !`). Exit 0 in sync, 1 local drift, 2 behind upstream, 3 could not verify; without `--name` the worst of all upstreams |
| `cargo xtask upstream --name <name> edit [--force]` | Rebuilds branch `unisolver` in `target/upstream/<name>/work`: the locked commit plus one commit per patch. Refuses to drop unexported commits or uncommitted changes unless `--force` |
| `cargo xtask upstream --name <name> export` | Writes branch `unisolver` back: its tree (filtered) replaces `third_party/<name>` (except tetra3's `data/`), its commits become the patch files and `series` |
| `cargo xtask upstream --name <name> sync <tag>` | Rebuilds the branch, rebases it onto `<tag>`, exports, updates the lock, runs the upstream's own tests and the workspace tests (`--no-test` skips them), and prints the upstream changelog with flags plus a checklist. Stops on a rebase conflict: resolve it in the work clone, `git rebase --continue`, then `sync --continue` (or `sync --abort`) |

## Changing a patch

```bash
cargo xtask upstream --name <name> edit
cd target/upstream/<name>/work
git rebase -i <locked tag>     # to change an existing patch: mark it `edit`, amend, continue
git commit                     # to add a patch: commit on top; the subject becomes its file name
cd -
cargo xtask upstream --name <name> export
cargo xtask upstream check
```

Patches may change only paths in `include`; `export` refuses anything else. Exported patches carry
a fixed author, `unisolver <patches@unisolver.invalid>`, so no maintainer address is published.
Describe a new patch in the queue's README.

## Syncing to a new upstream release

```bash
cargo xtask upstream --name tetra3 sync <tag>
```

A sync is a reviewed change, never automatic. tetra3's patch 0003 replaces the pattern-table
container, so a release can change solver behavior or the database format. Before committing, go
through the checklist the command prints: the `!!` entries, the "Sync history" line in the queue's
README, and a CHANGELOG entry. Commit as `chore(<name>): sync to <tag>`.

The narrow-field engine's matching core was ported from seiza into `crates/unisolver-starmatch`
(see that crate's documentation); it is our own code and does not follow seiza releases.

## Rules

- Never edit `third_party/<name>` by hand. `check` runs in `scripts/ci/ci-local.sh` and CI and
  fails on any byte the queue does not produce.
- A newer upstream release is reported (exit 2) but never applied without a `sync`.
