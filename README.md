# unisolver

English | [简体中文](README.zh-CN.md)

An embeddable, offline plate solver: give it a photo of the night sky and it tells you
where the camera was pointing (RA/Dec, roll, field of view and a full WCS), then labels
the stars, deep-sky objects, planets and satellites in the frame. The core is pure Rust
with **no network dependency**, integrated through a Flutter plugin, a C ABI or the Rust
crate.

- Algorithm: [tetra3rs](https://github.com/ssmichael1/tetra3rs) (the Rust port of tetra3 /
  cedar-solve): 4-star geometric hashing, Wahba/SVD attitude, statistical verification and
  a 3-DOF WCS refinement, adapted for phone photos and several database tiers.
- Speed: p50 17 ms / p90 313 ms end to end over 53 real frames (Apple M2 Max).
- Platforms: iOS, Android (arm64-v8a, x86_64), macOS, Windows (x64, arm64).
- License: MIT OR Apache-2.0; third-party code and data in
  [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).

## Quick start

```bash
# Build and run the tests
cargo test --workspace --release --features "imageio satellites"

# Solve an image from the command line (FITS/XISF/PNG/JPEG/TIFF detected automatically)
zstd -d packages/unisolver_flutter/assets/unisolver_10_80.db.zst -o /tmp/unisolver_10_80.db
cargo run --release -p solvecli -- --db /tmp/unisolver_10_80.db \
  packages/unisolver_flutter/example/assets/sample_scorpius.jpg

# Run the Flutter example (macOS / iOS / Android / Windows)
cd packages/unisolver_flutter/example && flutter run
```

To integrate it into your app, read **[docs/integration.md](docs/integration.md)**: the full
API of all three surfaces, initialization, solving, tracking, annotation, calibration, data
license obligations and common pitfalls. Toolchains and release builds:
[docs/building.md](docs/building.md).

## Repository layout

```
unisolver/
├── crates/
│   ├── unisolver-core/         # the engine: all logic lives here
│   ├── unisolver-cabi/         # C ABI (INDI / ASCOM / native desktop / Python)
│   └── unisolver-synth/        # synthetic star fields for tests
├── packages/unisolver_flutter/ # Flutter plugin (flutter_rust_bridge bindings) and example app
├── third_party/tetra3/         # vendored upstream tetra3rs (with local patches)
├── tools/
│   ├── solvecli/               # solver CLI: batch solves, σ-grid statistics, tracking, calibration
│   └── namesgen/               # builds the multilingual names pack (13 languages)
├── scripts/
│   ├── ci/                     # checks: public text, Windows cross-check, local manifest host
│   └── verify/                 # accuracy checks against astropy
├── testdata/                   # how tests get their inputs
└── docs/                       # integration and build guides
```

### crates/unisolver-core

The **single source** of behaviour; the Flutter and C layers only marshal.

| Module | Responsibility |
|---|---|
| `solver.rs` | Entry points: `Solver`, `SolveOptions`, extraction profiles, FOV ladders (`aspect_ladder` / `presets_with_hints` / `solve_with_fov_presets`) clamped to the database range |
| `outcome.rs` | Results: `SolveOutcome` / `SolvedGeometry` / `Wcs` (the WCS is authoritative for pixel ↔ sky) |
| `imageio/` | Format dispatch by magic bytes: `fits.rs` (including NAXIS3=3 colour), `xisf.rs` (zlib/lz4/zstd + shuffle), `raster.rs` (PNG/JPEG/TIFF, 16-bit included) |
| `annotate.rs` | Annotation layers: stars, named stars, deep-sky objects, solar system, **satellites**, with availability and reasons per layer |
| `pool.rs` | **Multi-tier routing**: `SolverPool` registers several databases, dispatches by FOV and falls back across tiers, extracting once |
| `names_pack.rs` | Multilingual names pack (`UNAM`): data-driven languages, fallback requested → English |
| `ephemeris.rs`, `satellites.rs` | Planet and moon ephemeris (Standish + Meeus), satellite passes (TLE + SGP4) |
| `calibrate.rs`, `camera.rs` | On-device multi-frame calibration (radial / polynomial distortion) and the camera model |
| `names.rs` + `named_stars.csv` | The IAU's 411 named stars (HIP, position, magnitude, English name; localized names live in the names pack) |
| `dso.rs`, `coords.rs`, `quat.rs`, `frame.rs`, `aberration.rs` | DSO catalog, coordinate conversions, quaternions, frames (with row stride), aberration |

Tests live in `crates/unisolver-core/tests/`: `solve_test` (ladders, profiles, clamping),
`pool_test` (routing), `storage_test` (UNISOLV2 mmap equivalence and corruption),
`annotate_test`, `calibrate_test`, plus tests on a private corpus of real captures that
print `skipped` when it is absent (see [testdata/README.md](testdata/README.md)).

### Conventions (read before changing code)

1. **Coordinates**: always image **top-left origin**, +x right, +y down; tetra3's
   centre-origin coordinates are converted at the core boundary.
2. **FOV means horizontal** (along the width). In portrait the horizontal side is the short
   one: about 44–48° for a phone main camera, not 70°+.
3. **Attitude**: convert pixels ↔ sky through `solution.wcs`; `quat_icrs2cam_wxyz` is the
   SVD-stage attitude and can differ from the final WCS by a few arcminutes (upstream
   behaviour), so use it only as a tracking hint.
4. **Headers are hints**: FITS/EXIF headers may be missing, wrong (reducers, binning) or
   ambiguous; they only reorder the ladder, which must still run on failure.
5. **No hard-coded σ**: `PhoneJpeg` (σ10) / `CleanSensor` (σ5) / `Auto` (σ10 first, then
   the other) / `Custom`, each chosen from measurements on real frames.

### third_party/tetra3

A copy of upstream **v0.13.0**. **Every local patch is recorded in
[third_party/tetra3/PATCHES.md](third_party/tetra3/PATCHES.md)**: two Cargo.toml
adjustments and the `UNISOLV2` mmap storage (`src/solver/storage.rs`, which keeps the
pattern table on disk and pages it in on demand: a deep database's resident memory drops
from 1.1 GB to 124 MB). Read the rebase notes there before changing anything.

### packages/unisolver_flutter

| Path | Purpose |
|---|---|
| `lib/unisolver_flutter.dart` | Public exports |
| `lib/asset_installer.dart` | First-launch install of the bundled assets (idempotent; migrates old database formats) |
| `lib/src/db_manager.dart` | Tier acquisition: manifest → resumable download → sha256 → decompress → register |
| `lib/src/rust/` | **Generated by flutter_rust_bridge; do not edit** (regenerate after changing `rust/src/api/**`) |
| `rust/src/api/` | Bindings: `solver.rs` (single and pool handles), `types.rs` (DTOs), `install.rs` (decompression + sha256), `satellites.rs`, `logging.rs` |
| `assets/` | The bundled 10–80° database (16 MB zstd) and DSO catalog with outlines (771 KB) |
| `lib/optional/` | The multilingual names pack (215 KB, GPL-2.0-or-later), opt-in per app |
| `example/` | Example app: solve, live tracking, calibration and database pages, with a language switch |

### Databases

The plugin bundles a **10–80° wide-field database** (phones and wide lenses). For long
lenses and telescopes, generate narrower databases with the upstream tetra3rs tools: the
engine loads them as they are, routes across every tier you register (`SolverPool`), and
`DbManager` installs them from any static host serving a manifest. See
[docs/integration.md](docs/integration.md), section 1.6.

## Development

```bash
git config core.hooksPath .githooks     # once: commit-message and pre-push checks
cargo test --workspace --release --features "imageio satellites"
cargo clippy -p unisolver-core -p unisolver-synth -p unisolver-cabi -p namesgen -p solvecli \
  --all-targets --features "imageio satellites" -- -D warnings
python3 scripts/ci/check_public_text.py
bash scripts/ci/check_windows.sh
(cd packages/unisolver_flutter && flutter test && flutter analyze)
```

Commit messages are one line, `type(scope): summary` (types: feat fix perf refactor docs
test data ci chore revert), with no body or trailers; the hook enforces it. User-visible
changes get an entry in `CHANGELOG.md`.

Where to change what:

| Change | Place |
|---|---|
| Solve strategy, extraction profiles, FOV ladders | `crates/unisolver-core/src/solver.rs` |
| A new image format | `crates/unisolver-core/src/imageio/` + magic dispatch |
| A new annotation layer | `crates/unisolver-core/src/annotate.rs` |
| A new Flutter API | `packages/unisolver_flutter/rust/src/api/`, then regenerate the bindings |
| A new C symbol | `crates/unisolver-cabi/src/lib.rs` (cbindgen regenerates the header) |
| The upstream algorithm itself | `third_party/tetra3/`, **recorded in PATCHES.md** |

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.
Copyright (c) 2026 Suzhou UMa Technology Co., Ltd.

The bundled data carries its own terms: the star database derives from ESA Gaia DR3
(CC BY-SA 3.0 IGO, attribution required), the DSO catalog from OpenNGC (CC BY-SA 4.0) and
the names pack from Stellarium (GPL-2.0-or-later). See
[THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).
