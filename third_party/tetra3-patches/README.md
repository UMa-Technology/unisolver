# tetra3rs patch queue

`third_party/tetra3` is upstream [tetra3rs](https://github.com/ssmichael1/tetra3rs) at the commit
pinned in [`../tetra3.lock`](../tetra3.lock), filtered to the lock's `include` paths, with the
patches listed in [`series`](series) applied in order. Never edit the vendored tree directly:
change the patches with `cargo xtask upstream edit` and `export`, and verify with
`cargo xtask upstream check` ([docs/upstream.md](../../docs/upstream.md)).

- Sync history: v0.11.0 → v0.13.0 on 2026-09-27
- Not vendored: upstream's `python/`, `docs/`, `examples/`, `scripts/`, `CLAUDE.md`,
  `CONTRIBUTING.md` and `.gitignore`

## 0001-cargo-drop-workspace-table

Cargo.toml: removes the `[workspace]` table. Upstream lists itself and `python/` as a workspace; a
package with its own workspace table cannot be a member of ours. The `python/` crate is not
vendored.

## 0002-cargo-drop-profile-tables

Cargo.toml: removes the `[profile.test]` and `[profile.release]` tables. Cargo only honors profiles
at the workspace root; they are replicated in the root Cargo.toml.

## 0003-mmap-pattern-store

Memory-mapped pattern-table storage (`UNISOLV2` container): a deep database's resident memory drops
from 1.1 GB to about 120 MB.

- NEW `src/solver/storage.rs`: `PatternStore` (owned `Vec` or memory-mapped view; `Deref`/`DerefMut`
  to `[PatternEntry]`, serde wire-compatible with `Vec`), `write_v2` / `read_v2` / `is_v2_file`.
  Little-endian only (all targets are).
- `src/solver/mod.rs`: `PatternCatalog.entries: Vec<PatternEntry>` → `PatternStore`; module
  registration. Call sites unchanged (`Deref`).
- `src/solver/database.rs`: NEW `save_to_file_v2` writes the container (upstream's
  `save_to_file`/`to_bytes` keep their `"T3DB"` format untouched, so upstream's own tests still
  pass; unisolver's callers use `_v2`); `load_from_file` sniffs the `UNISOLV2` magic (→ mmap) and
  otherwise defers to upstream `from_bytes`, which owns the `"T3DB"` header and the pre-header
  legacy path; `validate` split into `validate_head` (no table sweep, used by the mmap path) + full
  `validate`.
- `src/solver/pattern_search.rs`: probe-time bounds check on `entry.star_indices` (replaces the
  whole-table validate sweep for mmap'd files; 4 u32 compares per candidate). Lived in `solve.rs`
  before 0.13 split the lost-in-space search into `preprocess` / `pattern_search` / `verify`.
- Cargo.toml: + `memmap2 = "0.9"`.

Rebase note: upstream changes to `PatternEntry` layout (size/align/fields) must update the
compile-time guards in `storage.rs` and bump the container magic. (Unchanged in 0.13, so v2 files
written by earlier unisolver builds still load.)

## Downstream config divergence (not a patch — unisolver's own defaults)

`SolveConfig::pattern_checking_stars` (new in 0.13, upstream default 24) is set to `u32::MAX` in
`crates/unisolver-core/src/solver.rs`. Upstream tuned 24 for clean tracker frames where the
brightest detections are catalog stars; on phone frames the brightest routinely include hot pixels,
light-pollution blobs and trailed stars, so the true quads fall outside the cap — the 47-image
regression went 44 → 42, and a lost solve also costs the full FOV ladder (36 ms → 1309 ms).
unisolver already bounds the search with `max_centroids`, so it does not cap a second time.
Upstream documents raising the value for exactly this input class.
