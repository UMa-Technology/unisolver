# Changelog

## Unreleased

## 2026-09-29 — v0.4.2

1. **Solves no longer depend on how fast the CPU is.** The staged search's all-centroid
   probe of the likeliest rung ran for 100 ms (400 ms for a hinted rung). A phone three to
   five times slower than a desktop searched a third as far in that time, so a borderline
   photo the desktop solved at 85 000 patterns failed on the phone. The probe now checks a
   fixed 120 000 patterns (480 000 for a hinted rung), what 100 ms covered on an M2 Max: every
   machine searches the same patterns and gets the same result. On a Snapdragon 865 phone the
   47 test photos now solve as on the desktop (44), and a frame without stars fails about
   0.3 s later. The timeout still caps every pass; `thorough` stays off by default.
2. **Constellation illustrations are no longer bundled.** The painted set shipped with every
   app (3.2 MiB); it is now an opt-in pack like the low-poly one. **Apps that draw the art
   layer must declare** `packages/unisolver_flutter/optional/unisolver_art_western_new.bin` in
   their assets (`ConstellationArtSet.westernNew.load()` returns null otherwise), or read a copy
   they already carry with the new `ConstellationArtFiles(directory)`: a Stellarium sky culture
   directory (`index.json` and its images) or images named by IAU abbreviation. Both implement
   `ConstellationArtSource`.

## 2026-09-29 — v0.4.1

1. **FOV hints no longer lose borderline frames.** The staged search gives the likeliest
   rung one all-centroid probe of 100 ms. A borderline phone frame needed 65–120 ms there
   depending on the FOV estimate, so the EXIF focal length, 0.02° off the ladder's rung, tipped
   it out: the same photo solved without EXIF and failed with it. When the first rung is a hint
   (EXIF, FITS `FOCALLEN` + `XPIXSZ`, `focalLength35mm`) or the caller's own FOV, that probe
   now gets 400 ms; all 44 phone photos that solve plainly solve with EXIF too. A frame without
   stars that carries a hint fails in about 2.3 s instead of 2.0 s.
2. **Prebuilt C libraries for iOS and Android.** Releases now carry
   `unisolver-cabi-vX.Y.Z-ios.zip` (`unisolver.xcframework`: the static library for devices and
   simulators, iOS 12 or later, with a module map so Swift can `import unisolver`) and
   `unisolver-cabi-vX.Y.Z-android.zip` (`libunisolver_cabi.so` for arm64-v8a and x86_64, API 21 or
   later, 16 KB page aligned), for native apps that do not use the Flutter plugin.

## 2026-09-28 — v0.4.0

1. **Constellation art.** Annotation can lay mythology illustrations over the constellation
   figures (`constellationArt`; C `constellation_art`; Rust `AnnotateOptions::constellation_art`):
   each illustration comes back as a mesh of pixel positions to texture-map, placed from three
   anchor stars as Stellarium does, so it follows the lens model and the zoom. Two sets ship
   with the plugin, named by IAU abbreviation: `western_new` (painted, CC BY-SA 4.0, bundled)
   and `western` (low-poly, Free Art License 1.3, one optional file an app declares);
   `ConstellationArtSet.load()` returns their images and `dataAttributions()` their credits.
   The constellation pack moves to format v3 (it carries the anchors). The example draws the
   art behind an "Art" toggle.
2. **Wide frames fit their lens.** A wide solve (20° or more, 30 or more matched stars, no
   camera given) now fits the focal length and a radial distortion term to the frame's own
   stars, with a second term when it predicts left-out stars better and pins the corners
   down, and keeps them when they fit at least 5% better (`solution.lensFitted`; the
   distortion comes back in `wcs.camera`), so annotations follow the lens. On 38 phone photos,
   after the scale refinement below, it was kept for 32 (26 with the second term) and lowered
   the mean residual from 2.1 to 1.2 px without making any worse, at about 5 ms per solve.
   Turn it off with `fitLens: false` (C `fit_lens`, Rust `SolveOptions::fit_lens`);
   calibration sessions keep solving pinhole.
3. **Wide solves re-measure their scale.** A solve without a known camera kept the pixel
   scale its 4-star pattern measured, 1–3% off on wide frames: 73° frames of one phone solved
   anywhere between 72.7° and 75.6°, with the stars at the edges 5–7 px off. Such a solve of a
   wide field (20° or more) now finds the scale at which its brightest stars land on catalog
   stars, re-solves there and keeps the result when they land at least 10% closer
   (`solution.scaleRefined`); narrower fields keep theirs. The same
   frames' scale now comes out between 73.3° and 73.4°, and on 38 phone photos the mean
   residual fell from 2.7 to 2.1 px (1.2 px with the lens fit) without making any worse;
   together with the lens fit it adds 10–20 ms to a wide solve. Turn it off with
   `refineScale: false` (C `refine_scale`, Rust `SolveOptions::refine_scale`); solves given a
   camera or an attitude hint keep theirs.

## 2026-09-28 — v0.3.0

1. **Photos bring their own FOV, time and place.** JPEG, PNG and TIFF files now have their
   EXIF read: the 35 mm-equivalent focal length (or focal length with focal plane
   resolution) becomes the first ladder rung, and the capture time is read when EXIF gives its
   zone. FITS/XISF files contribute `DATE-AVG`, or `DATE-OBS` plus
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
   `UniAnnotator.constellationAt()` (C `unisolver_annotator_constellation_at_json`, Rust
   `Annotator::constellation_at`) names the constellation a position is in, such as the frame
   centre or a tapped point; the example shows the centre's.
6. **Coordinate grids, and annotation that follows the zoom.** Annotation can draw an
   equatorial grid (J2000) and a horizontal grid (apparent altitude and azimuth with the
   horizon, refraction included) as pixel polylines with chart-style labels anchored on the
   visible edges (`equatorialGrid`, `horizontalGrid`, `gridSpacingPx`; C `equatorial_grid`,
   `horizontal_grid`, `grid_spacing_px`). A new `viewport` option tells the engine what the app
   shows (visible region and zoom), so grid spacing, curve sampling, simplification and label
   positions follow it. `wcsSkyToPixels` / `wcsPixelsToSky` (C `unisolver_wcs_sky_to_pixels` /
   `unisolver_wcs_pixels_to_sky`, Rust `Wcs::sky_to_pixels` / `pixels_to_sky`) convert batches
   through the solve's lens model. The example now draws its overlay in screen space over a
   pinch-zoomable photo, keeps the detection rings behind a toggle and no longer rings a named
   star twice. Its labels are placed by priority so they never overlap; more appear as you
   zoom in.
7. **Previews for FITS and XISF.** `imagePreview(path:, maxSide:)` (Rust
   `imageio::preview` / `load_preview`) turns any supported image into an auto-stretched 8-bit
   preview no larger than `maxSide`, so apps can show astronomical frames that are otherwise
   black. `solvecli --annotate-dir` uses it for FITS/XISF backgrounds.
8. **Annotations stay on the stars when the time is known (behaviour change).** An
   observation time (`observationUnixMs`, and since item 1 the file's header) no longer feeds
   the stellar aberration correction. The solution and its WCS stay in the J2000 catalog
   frame, as other plate solvers report them. With the correction, every annotation layer,
   the grids and the batch transforms landed up to 20″ off the stars (invisible on phone
   photos, 10 px at 2″/px), and the reported centre was as far off the catalog frame, which
   mount sync expects. The time is still reported for the solar-system layer. An explicit
   `observer_velocity_km_s` (Rust `SolveOptions`, C) still asks for the physical pointing.

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
