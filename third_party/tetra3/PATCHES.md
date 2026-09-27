# Patches vs upstream tetra3rs v0.13.0

Vendored from tetra3rs **v0.13.0**. A pristine checkout of that tag is the baseline for the
three-way diff: `diff -r <upstream v0.13.0> third_party/tetra3` must equal the patches
below.

- Upstream: <https://github.com/ssmichael1/tetra3rs>
- Pinned tag: `v0.13.0` (released 2026-09-04)
- Sync history: v0.11.0 → v0.13.0 on 2026-09-27
- `tests/` are **UNMODIFIED**; `src/` carries patch 3 below.
- Drift check: `scripts/check_upstream.sh`.

1. Cargo.toml: removed `[workspace]` table (upstream lists itself + python/ as a
   workspace; a package with its own workspace table cannot be a member of ours).
   The python/ crate is not vendored.
2. Cargo.toml: removed `[profile.test]` / `[profile.release]` tables — cargo only
   honors profiles at the workspace root; they are replicated in the root Cargo.toml.
3. mmap pattern-table storage (`UNISOLV2` container) — deep-DB RSS 1.1 GB → ~120 MB.
   - NEW `src/solver/storage.rs`: `PatternStore` (Owned Vec / memory-mapped view;
     Deref/DerefMut to `[PatternEntry]`, serde wire-compatible with `Vec`),
     `write_v2` / `read_v2` / `is_v2_file`. Little-endian only (all targets are).
   - `src/solver/mod.rs`: `PatternCatalog.entries: Vec<PatternEntry>` → `PatternStore`;
     module registration. Call sites unchanged (Deref).
   - `src/solver/database.rs`: NEW `save_to_file_v2` writes the container
     (upstream's `save_to_file`/`to_bytes` keep their `"T3DB"` format untouched,
     so upstream's own tests still pass — unisolver's callers use `_v2`);
     `load_from_file` sniffs the `UNISOLV2` magic (→ mmap) and otherwise defers
     to upstream `from_bytes`, which owns the `"T3DB"` header and the pre-header
     legacy path; `validate` split into `validate_head` (no table sweep, used by
     the mmap path) + full `validate`.
   - `src/solver/pattern_search.rs`: probe-time bounds check on
     `entry.star_indices` (replaces the whole-table validate sweep for mmap'd
     files; 4 u32 compares per candidate). Lived in `solve.rs` before 0.13 split
     the lost-in-space search into `preprocess` / `pattern_search` / `verify`.
   - Cargo.toml: + `memmap2 = "0.9"`.
   Rebase note: upstream changes to `PatternEntry` layout (size/align/fields) must
   update the compile-time guards in storage.rs and bump the container magic.
   (Unchanged in 0.13, so v2 files written by earlier unisolver builds still load.)

## Downstream config divergence (not a patch — unisolver's own defaults)

`SolveConfig::pattern_checking_stars` (new in 0.13, upstream default 24) is set to
`u32::MAX` in `crates/unisolver-core/src/solver.rs`. Upstream tuned 24 for clean
tracker frames where the brightest detections are catalog stars; on phone frames
the brightest routinely include hot pixels, light-pollution blobs and trailed
stars, so the true quads fall outside the cap — the 47-image regression went
44 → 42, and a lost solve also costs the full FOV ladder (36 ms → 1309 ms).
unisolver already bounds the search with `max_centroids`, so it does not cap a
second time. Upstream documents raising the value for exactly this input class.
