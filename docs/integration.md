# Integrating unisolver

unisolver is an embeddable plate-solving engine: star pattern recognition, attitude
solving, object annotation and on-device calibration. The core is pure Rust, needs no
network and runs entirely offline. This guide is for developers adding it to their app.

## Choose a surface

| Your host | Surface | Location |
|---|---|---|
| Flutter app (iOS / Android / macOS / Windows) | `unisolver_flutter` plugin (flutter_rust_bridge bindings, recommended) | `packages/unisolver_flutter/` |
| Native desktop, INDI, ASCOM, Python ctypes, … | C ABI library + `unisolver.h` | `crates/unisolver-cabi/` |
| Rust | the `unisolver-core` crate | `crates/unisolver-core/` |

Verified platforms: iOS devices and arm64 simulators; Android arm64-v8a and x86_64 (no
armeabi-v7a); macOS arm64 and x86_64; Windows x64 and arm64 (the DLL must be built on a
Windows host; the cross-check passes on macOS). Toolchains and commands are in
[building.md](building.md).

---

## 1. Flutter

### 1.1 Dependency

The plugin is not published to pub.dev; depend on it through git (or a local path):

```yaml
dependencies:
  unisolver_flutter:
    git:
      url: https://github.com/UMa-Technology/unisolver
      path: packages/unisolver_flutter
```

With a Rust toolchain installed (see building.md), `flutter build` compiles the native
library for each platform through cargokit; there are no prebuilt artifacts to manage.

### 1.2 Initialize

```dart
import 'package:unisolver_flutter/unisolver_flutter.dart';

await RustLib.init();                                    // (1) FRB runtime, once per app start
final paths = await UnisolverAssets.ensureInstalled();   // (2) install bundled assets (idempotent)
final solver = await UniSolver.newInstance(dbPath: paths.dbPath); // (3) a long-lived handle
final namesPath = await UnisolverAssets.installNames();  // optional names pack, see 1.4 (null if absent)
```

- Bundled assets: the 10–80° database (≈16 MB zstd, 61 MB decompressed, loaded with mmap)
  and the DSO catalog (771 KB). The multilingual names pack (215 KB) is opt-in, see 1.4.
- `ensureInstalled` sniffs installed files by magic bytes; missing, empty or old v1 files
  are reinstalled, so it is safe to call on every launch.
- `solver.properties()` (synchronous) reports the database's FOV range, star and pattern
  counts, for UI hints.

### 1.3 Blind solving

**Photos and files** (FITS / XISF / PNG / JPEG / TIFF detected automatically; the
recommended entry):

```dart
final res = await solver.solveImageFileAuto(
  path: imagePath,
  base: SolveOptionsDto.defaults(fovEstimateDeg: 70),
);
if (res.outcome.status == SolveStatusDto.ok) {
  final s = res.outcome.solution!;
  // s.raDeg / s.decDeg / s.rollDeg / s.fovDeg / s.rmseArcsec / s.wcs
}
// res.attempts: status and timing per FOV rung, for progress or diagnostics
```

`solveImageFileAuto` carries the whole FOV strategy: a FOV computed from the header (FITS
focal length and pixel size) goes first as a hint, the aspect-ratio ladder follows on
failure, and rungs are clamped to the database's range, exactly as the C ABI's
`solve_image_json`. To control the rungs yourself (per-model presets, a known lens) use
`solveImageFileWithPresets(path, base, presets)`; `example/lib/fov_presets.dart` shows a
ladder.

- **Unknown FOV: use the ladder**; do not guess one value for `solveImageFile`.
- **Known FOV (fixed optics): skip the ladder**. Pass `fovEstimateDeg` and a tight
  `fovMaxErrorDeg` (for example 1.5° with 0.25°). The difference is an order of
  magnitude: 35 ms with a known FOV versus 425 ms walking a ladder on the same narrow-field
  camera.
- **Ladders search in stages**: the likeliest rung first with the brightest 28 centroids,
  the next rungs with the brightest 24, a short all-centroid probe of the first rung, then
  the remaining rungs. A frame without stars fails in under two seconds instead of 10–16,
  and slow edge cases got faster too. Set `thorough: true` to append the exhaustive search
  (every rung, every centroid, full timeout) when waiting beats missing.
- `ExtractionProfileDto`: the default `.phoneJpeg()` (σ10) suits compressed phone images;
  astro cameras and clean sensors want `.cleanSensor()` (σ5); for files of any origin use
  `.auto()` (σ10 first, the other profile on failure).

**Camera frames**:

```dart
final frame = FrameDto(
  width: w, height: h,
  rowStrideBytes: null,          // pass the real stride for padded rows
  kind: PixelKindDto.luma8,      // luma8 / luma16 / lumaF32 (little-endian) / rgba8
  bytes: lumaBytes,
);
final out = await solver.solveFrame(frame: frame, opts: opts);
```

**Burst tracking**: after a blind solve, pass the previous attitude as the next frame's
hint to take the fast path (the DTOs are plain generated classes without copyWith, so
construct every field):

```dart
final opts = SolveOptionsDto(
  fovEstimateDeg: prevFov,
  attitudeHintWxyz: prev.quatIcrs2CamWxyz,   // from the previous SolvedGeometryDto
  hintUncertaintyDeg: 3,
  strictHint: false,
  profile: const ExtractionProfileDto.customFast(sigma: 10, maxCentroids: 60),
  // ...the remaining fields as in SolveOptionsDto.defaults()
);
```

When tracking is lost (consecutive failures) fall back to a blind solve with the ladder;
on dark frames retry with `phoneJpeg()` and the hint before giving up. See
`example/lib/live_page.dart` for the full cadence.

### 1.4 Annotation

```dart
final annotator = await pool.annotator(db: res.db, dsoPath: paths.dsoPath, namesPath: namesPath);
final ann = await annotator.annotate(
  wcs: solution.wcs,
  opts: AnnotateOptionsDto(
    starMaxMag: 6.5, maxStars: 300, includeStarNames: true,
    includeDso: true, includeSolarSystem: true,
    observationUnixMs: shotAtUnixMs,          // required by the solar-system and satellite layers
    observer: ObserverDto(latDeg: 31.2, lonDeg: 121.5, altM: 10), // see below
    satelliteTle: tleText,                    // see below; null when unused
    language: 'zh_cn',
  ),
);
// ann.stars / ann.namedStars / ann.objects (DSO) / ann.solar / ann.satellites
// ann.layers: availability per layer plus **why** a layer is unavailable or degraded
```

**The engine returns data; drawing is yours.** Every object comes with **pixel
coordinates** (top-left origin), a name, an apparent size (`semiMajorPx` / `semiMinorPx`)
and an orientation (`angleDeg`; `null` means unknown: draw a circle of the semi-major axis,
never guess an angle). What to draw and how is up to you; feed it to a `CustomPainter`
(see `example/lib/overlay_painter.dart`). `solvecli --annotate-dir` is a development tool,
not a product surface.

**Outlines of extended objects.** About 190 nebulae, clusters and cloud complexes whose
shape an ellipse cannot describe (M42, the North America Nebula, the Veil, the Rosette, the
Orion, Rho Ophiuchi and Cygnus X complexes, the LMC, …) come with hand-drawn outlines from
OpenNGC. `DsoAnnotationDto.outlines` lists them per brightness level, outermost first:
level 1 traces the faint outer edge, level 3 only the bright core. Each `OutlineContourDto`
holds **interleaved** pixel coordinates (`points = [x0, y0, x1, y1, …]`) and whether it is
`closed`; contours are already simplified to about half a pixel. Draw the outline instead
of the ellipse when it is present. An outlined object's centre (`x`, `y`) may lie outside
the frame while part of its outline is inside, and outlined objects are kept regardless of
`dsoMaxMag` (extended nebulae rarely have a magnitude). Turn outlines off with
`dsoOutlines: false`, or cap the levels with `maxOutlineLevel` (1 keeps only the outer
edge). The C and Rust surfaces carry the same data, with `points` as `[x, y]` pairs.

**Build the annotator once and keep it.** Construction reads and parses the DSO catalog
(771 KB) and the names pack (215 KB):

| Operation | Measured (Apple M2 Max, 73.2° phone frame, 734 annotated objects, 20 outlined) |
|---|---|
| Solve | 350–550 ms |
| Annotate a frame | **0.96 ms** |
| Build an annotator | **2.12 ms** |

Rebuilding it per solve triples the cost of annotation (more on phones). With a pool keep
one per **tier that solved the frame** (narrow tiers have denser catalogs). The language
is an `annotate()` argument, so one annotator serves every language; see `_annotatorFor`
in the example's `solve_page.dart`.

**`observer` matters.** Without it the moon is **geocentric** and can be off by **up to 1°
(two lunar diameters)** from what the observer sees. With it the topocentric parallax is
applied, within **0.04°** of astropy. Planet and sun parallax is ≤ 0.003° either way.
Without an observer `layers.reasons` says so; the layer still works.

**Satellite layer** (`satelliteTle`): fetch TLEs yourself (e.g. CelesTrak); **the engine
never goes online**. It needs `observationUnixMs` and `observer`, and returns only
satellites **above the horizon**, since those below are hidden by the Earth even when
their RA/Dec falls in the field. For everything (pass lists, visibility), use
`satellitePositions(tleText: ..., unixMs: ..., observer: ...)`. A wrong TLE (say, an HTML
page) is an **error**, not zero satellites, which would read as "no passes today".

**Multilingual names**: `language` is a language code. The names pack
`unisolver_names.bin` provides **13 languages** (`en` `zh_cn` `zh_tw` `ja` `ko` `fr` `de`
`es` `it` `ru` `pl` `hu` `ro`) for **411 named stars and 615 deep-sky and solar-system
objects**. It is licensed **GPL-2.0-or-later**, so it is **not bundled by default**; opt in
one of two ways:

- **Bundle it with your app**: declare
  `packages/unisolver_flutter/optional/unisolver_names.bin` under `flutter: assets:` in your
  app's pubspec, then `await UnisolverAssets.installNames()` returns its path (null when the
  asset is not declared).
- **Download it**: when your manifest host lists it under `assets`, install it with
  `DbManager.installAsset(manifest.assetByName('unisolver_names')!)`.

Either way you distribute GPL data: keep the notice and license from
`packages/unisolver_flutter/lib/optional/` and show the Stellarium attribution.

```dart
annotator.languages();   // the languages in the pack (data-driven; do not hard-code them in UI)
// opts.language: 'zh_cn' / 'ja' / 'fr' …; 'zh-CN', 'zh_Hans' and 'zh' map to Simplified Chinese
```

- Fallback: **requested language → English → the catalog's curated Chinese name (Chinese
  requests only) → the designation**. A language missing from the pack falls back to
  English and says so in `layers.reasons`.
- Chinese star names carry the Latin name (`心宿二 Antares`); that is how the name data
  is written, and the engine passes it through.
- Without the names pack annotation still works: English names everywhere, plus Chinese
  names for deep-sky objects from the catalog (star names stay English).

### 1.5 On-device calibration (optional; for narrow fields and distorting lenses)

```dart
final cal = await solver.newCalibration();
for (final p in imagePaths) { await cal.addImageFile(path: p, opts: opts); }
final report = await cal.fit(model: const CalibModelDto.radial());  // or polynomial(...)
// persist report.camera (cameraParamsToJson / FromJson), then pass it as SolveOptionsDto.camera
```

On real data the radial model reduced the RMSE from 11.3 px to 1.4 px.

### 1.6 Several databases: `UniSolverPool` and `DbManager`

Narrow fields (long lenses and telescopes, under 10°) need narrow-field databases. **Do
not pick a database by FOV yourself**: register what is installed in a pool and let the
engine route.

```dart
final dir = File(paths.dbPath).parent.path;          // where the bundled tier was installed
final pool = await UniSolverPool.openDir(dir: dir);  // registers every database in the directory

final res = await pool.solveImageFileAuto(           // no database named
  path: imagePath,
  base: SolveOptionsDto.defaults(fovEstimateDeg: 70),
);
res.db;                                              // the tier that solved it (null if none)
res.attempts;                                        // tier, FOV, status and timing per attempt
final ann = await pool.annotator(db: res.db, dsoPath: paths.dsoPath); // annotate with that tier
```

Routing (no configuration needed; the same rules as the single-database ladder):

- A header FOV hint (FITS/XISF `FOCALLEN` + `XPIXSZ`) goes **straight to the tier covering
  it**; if the hint is wrong the ladder still runs (headers are hints, not truth).
- Without a hint: the aspect ladder on the wide tier, then each narrow tier sweeps its own
  range. Tolerance is [0.8×min, 1.25×max], so adjacent tiers meet and edge frames try both.
- With a calibrated camera (`base.camera`) or a tracking hint the FOV is known: **no
  ladder**, only tiers covering it.
- Extraction runs once and is reused across tiers (`res.extractCount`): a 26 Mpx frame
  takes seconds to extract, and re-extracting per rung would be an order of magnitude slower.

The plugin bundles one tier, covering 10–80°. For narrower fields, generate databases with
the upstream tetra3rs tools (the engine loads them as they are) and register them, or let
`DbManager` install them from any static host serving a manifest in the format below
(manifest → resumable download → sha256 check → decompress → register):

```dart
final mgr = DbManager(
  dir: dir,
  baseUrl: 'https://<your host>/unisolver/',
  register: (path) => pool.register(dbPath: path).then((_) {}), // solvable at once, no restart
);
final manifest = await mgr.fetchManifest();          // cached; cachedManifest() works offline
final tier = manifest.byName('my_narrow_tier')!;
await mgr.install(tier, onProgress: (p) => print('${p.phase} ${p.fraction}'));
```

Manifest fields (`manifest.json`, version 2):

| Field | Purpose |
|---|---|
| `key` | Content-addressed path `db/<last 8 hex of sha256>/<file>`; URL = `base_url + key` |
| `sha256` | Digest of the archive; **verified after every download** (`DbManager` refuses a mismatch and removes the leftover) |
| `bytes` / `raw_bytes` | Download size / decompressed size; `tier.diskBytesNeeded` is the peak disk use during install |
| `mobile` | Whether mobile devices should use it; mobile needs `allowNonMobile: true` otherwise |
| `bundled` | Ships with the plugin assets, not on the host; installed by `UnisolverAssets.ensureInstalled` |
| `license` / `attribution` | The tier's data license and the attribution it requires (Gaia DR3 for star databases) |
| `assets` | Optional files used as downloaded (the names pack): `name`, `kind`, `file`, `key`, `bytes`, `sha256`, `license`, `attribution`; install with `DbManager.installAsset` |

Behaviour (all covered by tests): an interrupted download keeps its `.part` and resumes
with `Range`; a server ignoring `Range` gets a clean restart; a digest mismatch deletes
the bad content (otherwise every resume would continue from bad bytes); a failed
decompression leaves no partial database and reports the space needed.

Databases use the `UNISOLV2` mmap format, so **resident memory follows the pages touched,
not the file size**: a deep database handle measured about 124 MB, and the bundled
database plus a phone frame about 130 MB end to end. Each extra tier costs its small
header plus the pages it touches.

### 1.7 Logs and errors

```dart
setLogStream(sink: ...);   // bridges Rust tracing logs into Dart; connect your logger
```

Engine errors surface as `AnyhowException` with the root cause in the message (corrupt
file, unsupported format, database version mismatch, …).

---

## 2. C ABI (INDI / ASCOM / native desktop / Python)

### Build

```bash
cargo build --release -p unisolver-cabi
# dynamic: target/release/libunisolver_cabi.{dylib|so} or unisolver_cabi.dll
# static:  libunisolver_cabi.a
# header:  crates/unisolver-cabi/include/unisolver.h (generated by cbindgen; do not edit)
```

### Prebuilt libraries

Each [GitHub release](https://github.com/UMa-Technology/unisolver/releases) carries
`unisolver-cabi-vX.Y.Z-<platform>.zip` for `macos-universal`, `linux-x86_64`, `windows-x86_64`
and `windows-aarch64`: `include/unisolver.h`, the dynamic and static libraries under `lib/`, and
the license files. Check downloads against the release's `SHA256SUMS`. The macOS dylib's install
name is `@rpath/libunisolver_cabi.dylib`: ship it beside your binary or on its rpath. Star
databases are not attached (section 4).

### Symbols

| Symbol | Purpose |
|---|---|
| `unisolver_version()` | Version string (static; do not free) |
| `unisolver_open(db_path, &err)` | Open a database → handle; NULL and `err` on failure |
| `unisolver_solve_image_json(solver, path, &err)` | Solve an image file with default options → JSON |
| `unisolver_solve_image_json_opts(solver, path, opts, &err)` | The same **with options**: known FOV / calibrated camera / tracking hint / profile / timeout |
| `unisolver_solve_frame_json_opts(solver, px, len, w, h, kind, stride, opts, &err)` | **Direct frame input** from memory, no disk |
| `unisolver_pool_open(dir, &err)` | Open a pool: register every `*.db` in `dir` |
| `unisolver_pool_register(pool, db_path, &err)` | Register one more tier (after an install) → tier JSON |
| `unisolver_pool_tiers_json(pool, &err)` | Registered tiers plus files skipped at open |
| `unisolver_pool_solve_image_json(pool, path, &err)` | Solve **without naming a tier**; the JSON adds `db` |
| `unisolver_pool_solve_image_json_opts(pool, path, opts, &err)` | The same with options (a known FOV tries only tiers covering it) |
| `unisolver_pool_solve_frame_json_opts(pool, px, len, w, h, kind, stride, opts, &err)` | Direct frame input with pool routing |
| `unisolver_pool_close(pool)` | Release a pool |
| `unisolver_satellites_json(tle, unix_ms, lat, lon, alt, &err)` | TLE + SGP4 → topocentric positions (JSON array) |
| `unisolver_annotator_open(solver, dso, names, &err)` | Build an annotator (both paths may be NULL); **build once and keep it** |
| `unisolver_pool_annotator_open(pool, db_name, dso, names, &err)` | The same from one tier of a pool (`db_name` = the solve JSON's `db`) |
| `unisolver_annotate_json(annotator, wcs_json, opts_json, &err)` | Annotate a frame from the solve JSON's `wcs` → annotation JSON |
| `unisolver_annotator_languages_json(annotator, &err)` | Languages in the names pack (JSON array) |
| `unisolver_annotator_close(annotator)` | Release an annotator |
| `unisolver_calibration_open(solver, &err)` | Open a calibration session (same-size images → camera with distortion) |
| `unisolver_calibration_add_image_json(cal, path, opts, &err)` | Add a frame → its solve JSON (**only solved frames count**) |
| `unisolver_calibration_count(cal)` | Frames accepted (-1 for NULL) |
| `unisolver_calibration_fit_json(cal, model, &err)` | Fit → `camera`, RMSE before/after, inlier counts |
| `unisolver_calibration_close(cal)` | Release a session |
| `unisolver_string_free(s)` | Release any string this library returned (including `err`) |
| `unisolver_close(solver)` | Release a solver |

Lifetimes (details at the top of the header): pointers from this library are released only
with the matching `unisolver_*_close` / `unisolver_string_free`; every release accepts
NULL; **one solver may run solves concurrently (shared read-only) but no call may be in
flight when it is closed**. For pools, `unisolver_pool_register` changes the pool, so no
other call may overlap it (solves are read-only and may run concurrently).

### Minimal example

```c
#include "unisolver.h"

char *err = NULL;
UnisolverSolver *s = unisolver_open("unisolver_10_80.db", &err);
if (!s) { fprintf(stderr, "%s\n", err); unisolver_string_free(err); return 1; }

char *json = unisolver_solve_image_json(s, "sky.jpg", &err);
if (json) {
    printf("%s\n", json);   // {"status":"Ok","solution":{"ra_deg":...,"wcs":{...}},"attempts":[...]}
    unisolver_string_free(json);
} else {
    fprintf(stderr, "%s\n", err); unisolver_string_free(err);
}
unisolver_close(s);
```

The JSON holds `status`, per-rung `attempts` (with timing) and `solution` (ra/dec/roll/fov,
matches, RMSE, the **full WCS** and the plate scale). This entry carries the same "header
hints + ladder + range clamp" strategy as the Flutter path.

**Solving and annotating are separate calls** (annotation depends only on the WCS);
continuing from `json` above:

```c
// Build once and keep it; both paths may be NULL (the layer degrades, it does not fail)
UnisolverAnnotator *ann = unisolver_annotator_open(
    s, "unisolver_dso.bin", "unisolver_names.bin", &err);

// wcs_json is the "wcs" object from the solve JSON, passed back unchanged
char *a = unisolver_annotate_json(ann, wcs_json, "{\"language\":\"en\"}", &err);
// {"stars":[...],"named_stars":[{"x":..,"y":..,"name":"Vega",..}],
//  "objects":[...],"solar":[...],"satellites":[...],
//  "layers":{"dso":true,...,"reasons":[["solar_system","..."]]}}
unisolver_string_free(a);
unisolver_annotator_close(ann);
```

`opts_json` may be NULL or `{}` for all defaults. Common fields: `language`,
`observation_unix_ms` (required by the solar-system and satellite layers), `observer` (moon
parallax; required by satellites) and `satellite_tle`.

### Known FOV and tracking (the telescope-driver case)

A driver usually knows its focal length and pixel size, hence the FOV. **Pass it and the
ladder is skipped**:

```c
// Known FOV: one attempt (35 ms versus 425 ms walking the ladder on the same camera)
char *j = unisolver_solve_image_json_opts(s, "frame.fits",
        "{\"fov_deg\":1.5,\"profile\":\"clean\"}", &err);

// Tracking (guiding, centring loops): the previous attitude as the hint. The solve skips
// the 4-star hash search and matches catalog stars projected around the hint; 3 stars
// suffice (a blind solve needs 4)
char *j2 = unisolver_solve_image_json_opts(s, "frame2.fits",
        "{\"fov_deg\":1.5,\"attitude_hint_wxyz\":[0.71,0.0,0.70,0.0],"
        "\"hint_uncertainty_deg\":3.0}", &err);
```

Every `opts_json` field is optional: `fov_deg` / `fov_max_error_deg` / `camera` /
`attitude_hint_wxyz` / `hint_uncertainty_deg` / `strict_hint` / `profile`
(`auto` · `phone` · `clean`) / `sigma` / `max_centroids` / `retry_alternate_profile` /
`thorough` /
`match_threshold` / `timeout_ms` / `observation_unix_ms` / `observer_velocity_km_s`.
`attitude_hint_wxyz` without `fov_deg` or `camera` is an **error**: tracking needs the
scale, and silently falling back to a blind solve would hide that tracking is not working.

### Direct frame input (camera callbacks, video)

Pixels straight from memory, without disk:

```c
// kind: "luma8" | "luma16" | "luma_f32" | "rgba8" (multi-byte samples are native-endian)
// stride: bytes per source row, for padded buffers (Android YUV_420_888 Y-plane rowStride,
//         iOS bytesPerRow); 0 when tightly packed
char *j = unisolver_solve_frame_json_opts(
    s, y_plane, y_len, 1920, 1080, "luma8", row_stride,
    "{\"fov_deg\":1.5,\"attitude_hint_wxyz\":[0.71,0.0,0.70,0.0]}", &err);
```

Two contracts: **the pixels are copied** (a camera callback's buffer is reclaimed at once
and cannot be assumed to outlive the solve), and a raw frame **has no header**, so without
`fov_deg`/`camera` the fallback is the aspect ladder rather than header hints. Live and
tracking use should pass the FOV and the previous attitude anyway.

### On-device calibration (optional)

```c
UnisolverCalibration *cal = unisolver_calibration_open(s, &err);
for (int i = 0; i < n; i++) {
    char *r = unisolver_calibration_add_image_json(cal, paths[i], "{\"fov_deg\":1.5}", &err);
    unisolver_string_free(r);          // each frame returns its solve JSON; unsolved ones do not count
}
char *rep = unisolver_calibration_fit_json(cal, "{\"model\":\"radial\"}", &err);
// {"camera":{...},"rmse_before_px":11.3,"rmse_after_px":1.4,"n_inliers":..,"frames_used":..}
```

Store the report's `camera` and pass it back as the `camera` field of `opts_json` to skip
the ladder and get the distortion correction.

---

## 3. Rust

```toml
[dependencies]
unisolver-core = { git = "https://github.com/UMa-Technology/unisolver", features = ["imageio", "satellites"] }
```

`Solver::from_file` → `solve` / `solve_with_fov_presets` / `annotator` /
`CalibrationSession`, and `SolverPool` for several tiers. This API is the common base of
the Flutter and C surfaces; see the rustdoc of each module.

---

## 4. Size, data files and license obligations

### Integration size (release + strip, measured)

The engine is a **library**; the host already carries the Flutter engine and the Dart
runtime, so only these count (**databases excluded**, see the next table):

| Item | Size | Notes |
|---|---|---|
| Android arm64 `libunisolver_frb.so` | **2.42 MiB** | stored uncompressed in the APK, so download = install size |
| iOS arm64 native code | **2.28 MiB** | statically linked into the host; the equivalent stripped cdylib |
| macOS arm64 `libunisolver_frb.dylib` | **2.30 MiB** | |
| Windows x64 / arm64 DLL | not measured | needs a Windows host; expected to be similar |
| Dart AOT | ~150 KB | `unisolver_flutter` + `flutter_rust_bridge` |
| `unisolver_dso.bin` (bundled asset) | 753 KiB (~457 KiB compressed in the APK) | DSO annotation catalog with outlines; omit it if you do not annotate |

**About 3.3 MiB installed / 3.0 MiB download per architecture** (Android arm64, without
databases). Shipping both arm64-v8a and x86_64 doubles the native part (x86_64 is only for
emulators). Integrating through the **C ABI** is smaller: `libunisolver_cabi.dylib` (macOS
arm64, stripped) is **1.67 MiB**, without the flutter_rust_bridge / serde_json /
tracing-subscriber layer.

### Data files

| File | Purpose | Size (zstd / raw) | Distribution |
|---|---|---|---|
| `unisolver_10_80.db` | wide field 10–80° (phones) | 16 MB / 61 MB | bundled with the plugin |
| `unisolver_dso.bin` | DSO catalog (NGC / IC / Messier) with outlines | 771 KB | bundled with the plugin |
| `unisolver_names.bin` | names in 13 languages (GPL-2.0-or-later) | 210 KB | opt-in: declared by the app, or downloaded |

- Databases use the `UNISOLV2` mmap container; the engine still reads old postcard v1 files.
- Only the wide tier is bundled; narrower tiers are yours to generate and host (see
  section 1.6).
- Mobile devices should stay at ≥ 2.5° tiers: phones have no narrower fields, and deeper
  tiers are too large to keep resident on mobile.
- **Attribution is required**: the star database derives from Gaia DR3 (ESA/Gaia/DPAC,
  CC BY-SA 3.0 IGO). Keep this in your app's About page, for example:
  *This work has made use of data from the European Space Agency (ESA) mission Gaia,
  processed by the Gaia Data Processing and Analysis Consortium (DPAC).*
  The DSO catalog derives from OpenNGC (CC BY-SA 4.0) and the names pack from Stellarium
  (GPL-2.0-or-later); see `THIRD_PARTY_LICENSES.md`.

### Showing attributions

Read the texts from the engine rather than hard-coding them, so they stay right when the
data changes. Each entry has `id`, `name`, `applies_to`, `license`, `text` and `url`:

| Surface | Call |
|---|---|
| Flutter | `dataAttributions()` (synchronous) → `List<DataAttributionDto>` |
| C | `unisolver_attributions_json()` → OWNED JSON array (free with `unisolver_string_free`) |
| Rust | `unisolver_core::data_attributions()` |

Show at least `gaia` (every star database), `openngc` when you annotate deep-sky objects
and `stellarium` when you ship the names pack. The example app lists them all under
"Data sources" in the app bar.

---

## 5. Common pitfalls

1. **FOV means horizontal** (along the width). In portrait the horizontal side is the short
   one: a main camera held upright spans about 44–48°, not 70°+. The ladder handles aspect
   ratios; keep that in mind for your own presets.
2. **Coordinates** are always top-left origin, +x right, +y down (centroids, annotations,
   WCS reference points).
3. **Headers are never assumed**: FITS/EXIF headers may be missing, wrong (a reducer or
   binning not reflected) or ambiguous. The engine uses them only as hints; do not show them
   to users as truth either.
4. **Attitude**: convert pixels ↔ sky through `solution.wcs`. `quatIcrs2CamWxyz` is the
   SVD-stage attitude and can differ from the final WCS by a few arcminutes; use it only as
   a tracking hint.
5. **Frames without stars** fail in under two seconds (measured 1.7 s on real frames). Use
   `attempts` to tell the user no stars were found; do not retry in a loop. With
   `thorough: true` failures take as long as the exhaustive search (10–16 s).
6. **Large frames**: extracting a 26 Mpx astro frame peaks around 700 MB (full-frame f32
   buffers, unrelated to the database). On mobile downsample to ≤ 4K first, or process
   originals on desktop.
7. **Windows builds**: the DLL must be built on a Windows host (the dependency chain
   includes dart-sys); macOS can cross-check but not produce it.
8. **macOS sandbox needs network access**: to download with `DbManager`, the host's
   `*.entitlements` must include `com.apple.security.network.client` (the Flutter template
   does **not**). Without it you get a `SocketException`, not a permission error. Copy the
   example's `macos/Runner/{DebugProfile,Release}.entitlements`.
9. **Narrow frames without a hint sweep the ladder**: without a header FOV hint, a 3°
   frame routed through a multi-tier pool took 3 s (measured). Give a hint whenever you can
   (EXIF/FITS headers or a calibrated camera): one attempt instead of a dozen.

## 6. Versions and compatibility

- Database files: the engine detects v2/v1 by magic. New engines read old files; old engines
  cannot read v2, so record the minimum engine version when distributing databases.
- API: the Dart and C surfaces follow semantic versioning. Informational fields such as
  `attempts` and `layers.reasons` may gain entries; parse them tolerantly.
