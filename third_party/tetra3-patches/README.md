# tetra3rs patch queue

`third_party/tetra3` is upstream [tetra3rs](https://github.com/ssmichael1/tetra3rs) at the commit
pinned in [`../tetra3.lock`](../tetra3.lock), filtered to the lock's `include` paths, with the
patches listed in [`series`](series) applied in order. Never edit the vendored tree directly:
change the patches with `cargo xtask upstream edit` and `export`, and verify with
`cargo xtask upstream check` ([docs/upstream.md](../../docs/upstream.md)).

- Sync history: v0.11.0 → v0.13.0 on 2026-09-27; v0.13.0 → v0.14.0 on 2026-10-04 (database
  format 2; the old 0003, the `UNISOLV2` container, was replaced by today's 0003)
- Not vendored: upstream's `python/`, `docs/`, `examples/`, `scripts/`, `CLAUDE.md`,
  `CONTRIBUTING.md` and `.gitignore`

## 0001-cargo-drop-workspace-table

Cargo.toml: removes the `[workspace]` table. Upstream lists itself and `python/` as a workspace; a
package with its own workspace table cannot be a member of ours. The `python/` crate is not
vendored.

## 0002-cargo-drop-profile-tables

Cargo.toml: removes the `[profile.test]` and `[profile.release]` tables. Cargo only honors profiles
at the workspace root; they are replicated in the root Cargo.toml.

## 0003-mmap-database-file

`SolverDatabase::open_mapped(path)`: memory-maps a database file instead of reading it. In format 2
(tetra3 0.14) the pattern table's packed section is byte-identical to the in-memory table, so a
mapped table keeps it on the file and pages it in as probes touch it: a large database's resident
memory follows the pages solves use (the stars, their vectors and the rank directory are decoded
into memory as upstream does).

- Cargo.toml: + `memmap2 = "0.9"`.
- `src/solver/pattern_catalog.rs`: `PackedStore::Mapped { map, start, len }`, a range of the
  mapping.
- `src/solver/database.rs`: `Owner` (shared buffer or mapping) replaces `decode`'s owner argument;
  NEW `open_mapped`; `validate` split into `validate_head` (every check but the star-index sweep)
  and `validate` (head + sweep). A mapped format-2 table gets `validate_head`: the sweep would page
  in the whole table. Format 1 files decode into memory and get the full `validate`. Upstream's
  `load_from_file` / `from_vec` / `from_bytes` are untouched. Test `open_mapped_keeps_the_table_on_the_file`.
- `src/solver/pattern_search.rs`: probe-time bounds check on `entry.star_indices` (four `u32`
  compares per candidate), standing in for the sweep on mapped tables.

Rebase note: a change to the packed entry layout (`PACKED_ENTRY_BYTES`) or a new database format
version means re-encoding the tiers. unisolver ships format-2 files only and refuses the old
`UNISOLV2` container (in `unisolver-core`, not here).

## Downstream config divergence (not a patch — unisolver's own defaults)

`SolveConfig::pattern_checking_stars` (new in 0.13, upstream default 24) is set to `u32::MAX` in
`crates/unisolver-core/src/solver.rs`. Upstream tuned 24 for clean tracker frames where the
brightest detections are catalog stars; on phone frames the brightest routinely include hot pixels,
light-pollution blobs and trailed stars, so the true quads fall outside the cap — the 47-image
regression went 44 → 42, and a lost solve also costs the full FOV ladder (36 ms → 1309 ms).
unisolver already bounds the search with `max_centroids`, so it does not cap a second time.
Upstream documents raising the value for exactly this input class.
