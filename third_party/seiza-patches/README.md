# seiza patch queue

`third_party/seiza` is upstream [seiza](https://github.com/theatrus/seiza) at the commit pinned in
[`../seiza.lock`](../seiza.lock), filtered to the lock's `include` paths (the `seiza`,
`seiza-calibration`, `seiza-imgproc` and `seiza-stats` crates), with the patches listed in
[`series`](series) applied in order. Never edit the vendored tree directly: change the patches with
`cargo xtask upstream --name seiza edit` and `export`, and verify with `cargo xtask upstream check`
([docs/upstream.md](../../docs/upstream.md)).

- Sync history: vendored at v0.19.2 on 2026-10-04
- Not vendored: the other workspace crates (CLI, downloads, stacking, ...), `docs/`, `packaging/`,
  `seiza/examples/`, `seiza-imgproc/tests/`

## 0001-cargo-flatten-workspace-inheritance

The four `Cargo.toml` files: every `*.workspace = true` field and dependency becomes the explicit
value from upstream's workspace root, which is not vendored; the `downloads` feature and its optional
`seiza-download` dependency are removed (with the matching re-export in `seiza/src/lib.rs`).

## 0002-solve-deadlines

Optional wall-clock deadlines, so a caller can bound a failing solve: NEW `Error::Timeout`,
`solve::solve_until` and `blind::solve_blind_until` (`solve` and `solve_blind` forward with no
deadline, unchanged). Checked before each hinted search window and rank-robust quad, before blind
hypothesis scoring, before each verification batch and at the start of every hypothesis
verification. Test: `blind::tests::solves_give_up_at_a_passed_deadline`.
