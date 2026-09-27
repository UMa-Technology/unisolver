# Changelog

## Unreleased

1. **Photos bring their own FOV, time and place.** JPEG, PNG and TIFF files now have their
   EXIF read: the 35 mm-equivalent focal length (or focal length with focal plane
   resolution) becomes the first ladder rung, and the capture time (when EXIF gives its zone)
   feeds the aberration correction. FITS/XISF files contribute `DATE-AVG`, or `DATE-OBS` plus
   half the exposure. Outcomes report `observationUnixMs` and, for photos with GPS,
   `observer` (Rust `SolveOutcome`, Flutter `SolveOutcomeDto`, C JSON), so the solar-system
   layer needs no input from the app; the example now draws it.
2. **Decoded photos can pass their EXIF.** `SolveOptionsDto.focalLength35Mm` (Rust
   `SolveOptions::focal_length_35mm`, C `focal_length_35mm`) puts that focal length's FOV
   first in any ladder, for formats the engine does not decode.
3. **HEIC is rejected with directions.** A HEIC/HEIF file now fails with an error that says to
   decode it on the platform and use a frame entry; the integration guide shows how.
4. **Large frames take less than half the memory.** Frames above 16 Mpx are extracted in
   horizontal bands converted straight from the frame's pixels, without a full-frame f32 copy:
   solving a 26 Mpx astro frame peaks at 290 MB instead of 630 MB (with a narrow tier loaded)
   and gives the same solutions. Smaller frames are extracted as before.
5. **Constellations.** A new constellation pack, `unisolver_constellations.bin` (bundled,
   CC BY-SA 4.0), carries the 88 IAU constellations' figures from the IAU charts and their
   official boundaries. Annotation draws them on request (`includeConstellations`,
   `constellationBoundaries`; C `include_constellations`, `constellation_boundaries`) as
   pixel polylines with names and label positions; load the pack with
   `annotator(constellationsPath: ...)` and `UnisolverAssets.installConstellations()`
   (C `unisolver_annotator_load_constellations`, Rust `Annotator::with_constellations`). The
   names pack now names the constellations in its 13 languages, and `dataAttributions()`
   lists the new source. `annotator()` gains the optional `constellationsPath` parameter.

## 2026-09-28 — v0.2.0

1. **Data attributions from the engine.** `dataAttributions()` (Flutter),
   `unisolver_attributions_json()` (C) and `unisolver_core::data_attributions()` (Rust) return
   the attribution each data source requires (Gaia DR3, Hipparcos, IAU WGSN, OpenNGC,
   Stellarium), so apps no longer hard-code them. The example app shows them under
   "Data sources". The Flutter package now also exports `satellitePositions`.
2. **The names pack is opt-in (breaking).** `unisolver_names.bin` is GPL-2.0-or-later, so the
   plugin no longer bundles it into every app. Declare
   `packages/unisolver_flutter/optional/unisolver_names.bin` in your app's assets and call
   `UnisolverAssets.installNames()`, or download it with `DbManager.installAsset()` from a
   manifest's new `assets` list. `UnisolverAssets.ensureInstalled()` no longer returns `namesPath`.
3. **Manifest entries carry their license.** Tiers and assets may list `license` and
   `attribution`; `DbTier` and `DbAsset` expose them.
4. **Failures are fast.** FOV ladders now search in stages: the likeliest rung first with
   the brightest 28 centroids, the next rungs with the brightest 24, a 100 ms all-centroid
   probe of the first rung, then the rest. On 47 real phone frames the same 44 solve, the
   three without stars fail in 1.7 s instead of 10–16 s, p90 drops from 272 ms to 184 ms,
   and edge cases between rungs went from 3–11 s to 0.5–1.4 s. The new `thorough` option
   (Rust `SolveOptions`, Flutter `SolveOptionsDto`, C `opts_json`, `solvecli --thorough`)
   appends the previous exhaustive search. `SolveOptionsDto` gains the `thorough` field.
5. **Outlines for extended objects.** The DSO catalog now carries OpenNGC's hand-drawn
   outlines (up to three brightness levels) for about 190 nebulae, clusters and cloud
   complexes, including twelve regions new to the catalog such as the Orion, Rho Ophiuchi
   and Cygnus X complexes, the Vela SNR and the LMC. Annotations return them projected to
   pixels (`DsoAnnotationDto.outlines`, C/Rust `outlines`), and outlined objects are kept
   regardless of `dsoMaxMag`. New options `dsoOutlines` (default on) and `maxOutlineLevel`
   (default 3); `AnnotateOptionsDto` gains both fields. The example app and
   `solvecli --annotate-dir` draw them. The catalog grows from 528 KB to 771 KB; files
   written by older versions still load.
6. **The tetra3rs copy is a patch queue.** `third_party/tetra3` is now generated from the
   pinned upstream release (`third_party/tetra3.lock`) plus `git format-patch` files in
   `third_party/tetra3-patches/`. `cargo xtask upstream check` verifies it byte for byte (it
   runs in the local gate and CI), and `edit` / `export` / `sync` change the patches or rebase
   them onto a new upstream release (docs/upstream.md). Upstream's CLAUDE.md, CONTRIBUTING.md
   and .gitignore are no longer vendored.
7. **Prebuilt C libraries.** GitHub releases carry `unisolver-cabi-vX.Y.Z-<platform>.zip` for
   macOS (universal), Linux x86_64 and Windows x86_64 and aarch64: the header, the dynamic and
   static libraries and the licenses, plus the names pack with its notice and `SHA256SUMS`
   (docs/releasing.md).
8. **The macOS dylib's install name is `@rpath/libunisolver_cabi.dylib`.** It used to be the
   absolute path the library was built at, so a copied dylib did not load without
   `install_name_tool`.

## 2026-09-27 — Repository baseline

The state of the engine when this repository was opened, summarized.

1. **Blind plate solving on phone photos and astro frames.** tetra3rs pattern matching with
   aspect-aware FOV ladders, header hints (FITS/XISF) that only reorder the ladder, rungs
   clamped to the database range, and extraction profiles for compressed phone images (σ10),
   clean sensors (σ5) or both. 44 of 47 real phone frames solve (the three failures have no
   stars), p50 17 ms end to end.
2. **Image input**: FITS (including planar colour), XISF (zlib/lz4/zstd, byte shuffling),
   PNG, JPEG and TIFF, 8/16-bit and float, with headers treated as hints.
3. **Several database tiers**: `SolverPool` routes a frame to the tier that can solve it and
   falls back across tiers, extracting once. Databases use the UNISOLV2 container, which
   memory-maps the pattern table (a deep database stays around 124 MB resident instead of
   1.1 GB).
4. **Database acquisition**: `DbManager` fetches a manifest, downloads with resume, verifies
   sha256, decompresses as a stream and registers the tier with the pool.
5. **Annotation as data**: catalog stars, IAU named stars, deep-sky objects (OpenNGC, with
   position angles), the sun, moon and planets (topocentric moon within 0.04° of astropy)
   and satellites (TLE + SGP4), each with pixel coordinates and per-layer availability.
6. **Tracking and calibration**: a previous attitude as a hint takes the fast matching path;
   multi-frame calibration fits radial or polynomial distortion (11.3 px → 1.4 px RMSE on
   real data).
7. **Three surfaces**: the Flutter plugin (iOS, Android, macOS, Windows), a C ABI with a
   generated header, including direct pixel-buffer input, and the Rust crate.
8. **Upstream**: tetra3rs v0.13.0 vendored with three recorded patches and a drift check.
