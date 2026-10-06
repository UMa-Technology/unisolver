# Integrating unisolver

unisolver is an embeddable plate-solving engine: star pattern recognition, attitude
solving, object annotation and on-device calibration. The core is pure Rust, needs no
network and runs entirely offline. This guide is for developers adding it to their app.

## Choose a surface

| Your host | Surface | Location |
|---|---|---|
| Flutter app (iOS / Android / macOS / Windows) | `unisolver_flutter` plugin (flutter_rust_bridge bindings, recommended) | `packages/unisolver_flutter/` |
| Native desktop, native iOS / Android, INDI, ASCOM, Python ctypes, … | C ABI library + `unisolver.h` | `crates/unisolver-cabi/` |
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
  and the DSO catalog (796 KB). The multilingual names pack (215 KB) is opt-in, see 1.4.
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
focal length and pixel size; EXIF 35 mm-equivalent focal length, or focal length and focal
plane resolution) goes first as a hint, the aspect-ratio ladder follows on failure, and
rungs are clamped to the database's range, exactly as the C ABI's `solve_image_json`. The
header's observation time (FITS `DATE-AVG`, or `DATE-OBS` plus half the exposure; EXIF
`DateTimeOriginal` with its zone), unless you pass one, comes back in
`outcome.observationUnixMs`, next to `outcome.observer` from EXIF GPS: hand both to the
annotator (§1.4). The time does not change the solution: the WCS and the reported centre are in
the J2000 catalog frame, so catalog positions projected through it land on the stars. To control the rungs yourself (per-model presets, a known lens) use
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
  and slow edge cases got faster too. When the first rung is a hint (EXIF focal length, FITS
  `FOCALLEN` + `XPIXSZ`, `focalLength35mm`) or your own FOV rather than the built-in ladder's
  guess, its probe checks four times as many patterns, so borderline frames with few stars
  solve with the hint too. The probe is sized in patterns, not milliseconds, so a phone
  reaches the same answer as a desktop, only later; the timeout still caps every pass. Set
  `thorough: true` to append the exhaustive search (every rung, every centroid, full timeout)
  when waiting beats missing: a frame without stars then takes about 20 s to fail on a
  desktop instead of 2.
- **Wide frames spread their centroids** (blind solves of 10° or wider): instead of the
  brightest 100 overall, the frame is cut into square cells, 8 along its long side, and each
  cell gives its brightest in turn, so a foreground lit by streetlights or windows cannot take
  every slot from the stars of a light-polluted sky. `outcome.centroids` is then in that order.
  Narrower fields and tracking keep the brightest. The plugin and the C library always do this;
  Rust callers can turn it off with `SolveOptions::spread_wide`.
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

**Photos the engine does not decode (HEIC).** The engine reads FITS, XISF, PNG, JPEG and
TIFF; iPhone photos default to HEIC, which it rejects with an error pointing here (the only
open decoders are LGPL, with HEVC patents). Decode on the platform (ImageIO on Apple,
`ImageDecoder` on Android), read the EXIF there too, and pass what the file would have
given:

```dart
final res = await pool.solveFrameAuto(
  frame: FrameDto(width: w, height: h, rowStrideBytes: null,
                  kind: PixelKindDto.rgba8, bytes: rgba),
  opts: SolveOptionsDto(
    // ...the fields of SolveOptionsDto.defaults(fovEstimateDeg: 70), plus:
    focalLength35Mm: 24,            // EXIF FocalLengthIn35mmFilm: tried first, ±15%
    observationUnixMs: takenAtUtcMs, // DateTimeOriginal + OffsetTimeOriginal, as UTC
  ),
);
```

The EXIF keys are `kCGImagePropertyExifFocalLenIn35mmFilm`,
`kCGImagePropertyExifDateTimeOriginal` and `kCGImagePropertyExifOffsetTimeOriginal` on Apple,
`TAG_FOCAL_LENGTH_IN_35MM_FILM`, `TAG_DATETIME_ORIGINAL` and `TAG_OFFSET_TIME_ORIGINAL` in
Android's `ExifInterface`. Without a zone, leave the time out rather than guess: a wrong hour
moves the moon by half a degree.

### 1.4 Annotation

```dart
final annotator = await pool.annotator(
  db: res.db, dsoPath: paths.dsoPath, namesPath: namesPath,
  constellationsPath: await UnisolverAssets.installConstellations(),
);
final ann = await annotator.annotate(
  wcs: solution.wcs,
  opts: AnnotateOptionsDto(
    starMaxMag: 6.5, maxStars: 300, includeStarNames: true,
    includeDso: true, includeSolarSystem: true,
    observationUnixMs: shotAtUnixMs,          // required by the solar-system and satellite layers
    observer: ObserverDto(latDeg: 31.2, lonDeg: 121.5, altM: 10), // see below
    satelliteTle: tleText,                    // see below; null when unused
    language: 'zh_cn',
    includeConstellations: true,              // figures and names (off by default)
    constellationBoundaries: true,            // IAU boundaries (off by default)
  ),
);
// ann.stars / ann.namedStars / ann.objects (DSO) / ann.solar / ann.satellites
// ann.constellations / ann.boundaries
// ann.layers: availability per layer plus **why** a layer is unavailable or degraded
```

For a photo, take the time and place from the solve: `outcome.observationUnixMs` (from the
options or the file's header) and `outcome.observer` (EXIF GPS). The example enables the
solar-system layer whenever the solve reports a time.

**The engine returns data; drawing is yours.** Every object comes with **pixel
coordinates** (top-left origin), a name, an apparent size (`semiMajorPx` / `semiMinorPx`)
and an orientation (`angleDeg`; `null` means unknown: draw a circle of the semi-major axis,
never guess an angle). What to draw and how is up to you; feed it to a `CustomPainter`
(see `example/lib/overlay_painter.dart`). `solvecli --annotate-dir` is a development tool,
not a product surface.

**Draw in screen space.** Screens differ in size and users zoom, so never draw the overlay
into the image at fixed sizes: map each point from image pixels to the screen with your
viewer's transform, and keep stroke widths, marker sizes and font sizes in screen pixels.
Sizes the sky gives (a nebula's `semiMajorPx`, the moon's `angularRadiusPx`) scale with the
image. Scaling the photo and its overlay together (a `FittedBox` round both) shrinks a 14 px
label to about 3 px when a 1920 px photo fits a phone. The example pairs an
`InteractiveViewer` for the photo with a painter on top that reads the viewer's
`TransformationController` (`solve_page.dart`, `overlay_painter.dart`); opaque line colors
render more evenly than translucent ones.

**Keep labels apart.** In a wide field of the Milky Way, star and deep-sky names pile up.
Whether two labels collide depends on their size on screen (font, language, zoom), which
only the app knows, so placing them is the app's job. The example (`label_layout.dart`) places
them greedily in screen space, in priority order: grid readings, the sun, moon and planets,
constellation names, named stars from the brightest, deep-sky objects from the brightest. Each
label tries below, above, right and left of its marker, and prefers spots on screen that cover
no other ring. A label with no free spot is left out until zooming in makes room. It runs on
every repaint.

**Showing FITS and XISF.** Flutter cannot decode them, and their linear data would look
black anyway. `imagePreview(path:, maxSide:)` returns an auto-stretched greyscale preview as
RGBA (the engine box-averages the frame down by a whole factor so neither side exceeds
`maxSide`, then lifts the sky to a quarter of full scale from its median and MAD, as
PixInsight's AutoSTF does): decode it with `ui.decodeImageFromPixels(..., PixelFormat.rgba8888,
...)` and draw it over the source's size (`sourceWidth` × `sourceHeight`) so it lines up with
the annotations. About 40 ms for a 26 Mpx frame. Rust: `imageio::preview` / `load_preview`.

**Tell the engine what you show.** Pass `viewport` (the visible image rectangle in image
pixels, and `scale`, screen pixels per image pixel) and annotate again when the user
finishes zooming or panning: it costs about a millisecond. Lines then follow the zoom:
grid spacing, curve sampling and simplification (to half a screen pixel), and labels on the
visible edges. Without it the engine assumes the whole image at scale 1. Point layers
(stars, deep-sky objects, planets) are the same either way.

**Coordinate grids.** `equatorialGrid` draws J2000 right ascension and declination;
`horizontalGrid` draws apparent altitude and azimuth with the horizon, and needs
`observationUnixMs` and `observer` (refraction is included; the solve reports both for
photos with EXIF). The step is the finest round value at least `gridSpacingPx` (default 150)
screen pixels apart: 10° and 30m on a wide field, down to arcseconds when zoomed in. Each
`GridLineDto` has its value (`valueDeg`), the value as charts print it (`text`: `16h30m`,
`−20°30′`, `180°`), a compass point for azimuths on a multiple of 45° (`cardinal`: `S`, `NW`;
localize it), its polylines (`lines`) and one `label` anchored on the visible edge it
crosses (`edge` says which, so nudge the text inward; `angleDeg` is the line's direction if
you want to align the text with it).

**Your own overlays.** For what the layers do not draw (a framing box, a crosshair, the sky
position under a tap), `wcsSkyToPixels(wcs:, radec:)` and `wcsPixelsToSky(wcs:, pixels:)`
convert interleaved batches through the solve's lens model in one call each (C
`unisolver_wcs_sky_to_pixels` / `unisolver_wcs_pixels_to_sky`, Rust `Wcs::sky_to_pixels` /
`pixels_to_sky`). Points the model cannot place (behind the camera, or far outside the frame
where the distortion polynomial folds back) come back as NaN.

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

**Constellations.** The constellation pack `unisolver_constellations.bin` (bundled with the
plugin; `UnisolverAssets.installConstellations()` returns its path) holds the 88 IAU
constellations: the line figures of the IAU charts (drawn by Sky & Telescope) and the
official boundaries (Delporte 1930, in J2000). With `includeConstellations`, each
`ConstellationAnnotationDto` in the frame has its IAU abbreviation, a name (from the names
pack in the requested language, else the IAU name such as "Orion"), a label position
(`labelX`/`labelY`, null when no star of the figure is in the frame) and its figure as
polylines (`lines`, interleaved pixels). With `constellationBoundaries`, each
`BoundaryAnnotationDto` is a boundary stretch (`points`) with the constellations on either
side (`between`). Polylines follow great circles through the lens model, may run past the
frame edge (let the canvas clip them) and break where the sky leaves the camera's view.
Both options are off by default. Without the pack the layers report themselves unavailable
in `layers.reasons`.

**Which constellation a point is in.** `annotator.constellationAt(raDeg:, decDeg:, language:)`
returns the constellation containing a J2000 position as a `ConstellationNameDto` (IAU
abbreviation and name, localized as above): pass the solve's `raDeg` / `decDeg` for the frame
centre, or a tapped point converted with `wcsPixelsToSky`. It is a lookup, not a layer, and
takes a few microseconds. The position is looked up in the boundaries' own frame (B1875), so
the answer agrees with the boundaries drawn. It needs the constellation pack (null without
it). C: `unisolver_annotator_constellation_at_json`; Rust: `Annotator::constellation_at`.

**Constellation art.** With `constellationArt` (and `includeConstellations`), each
constellation with an illustration carries `art`: a `cols × rows` mesh over the whole image,
row-major, the vertex in row r and column c at image position (c / (cols − 1), r / (rows − 1))
of its width and height, with its pixel position in `points` (NaN, NaN where the lens model
cannot place it). Draw it as textured triangles, two per cell, skipping any triangle with a
missing vertex: `drawVertices` with an `ImageShader` (see `artTriangles` in the example's
`overlay_painter.dart`). The illustrations are light on black, so blend them onto the photo
with `BlendMode.screen` at 40–60% opacity and the black disappears. The engine places each
one from three anchor stars, as Stellarium does, so the art follows the lens model and the
zoom. The illustrations are **not bundled**: declare the set you want in your app's assets,
`packages/unisolver_flutter/optional/unisolver_art_western_new.bin` (painted, CC BY-SA 4.0) or
`packages/unisolver_flutter/optional/unisolver_art_western.bin` (low-poly, Free Art License 1.3),
and `ConstellationArtSet.westernNew.load(abbr)` or `.western.load(abbr)` returns the image
bytes. An app that already carries the drawings reads its own copy with
`ConstellationArtFiles(directory)`: a Stellarium sky culture directory (`index.json` and its
images) or images named by IAU abbreviation; the anchors are those of Stellarium's western
sky cultures, so the drawings must be theirs. All return null for a constellation without an
illustration (Puppis, Vela and Serpens appear in their neighbours' art). Credit the set you
ship (`dataAttributions()`). C and Rust: the option is `constellation_art`; the images are the
`UART` packs in the plugin's `lib/optional/`, whose layout is given in
`constellation_art.dart`.

**Build the annotator once and keep it.** Construction reads and parses the DSO catalog
(796 KB), the names pack (215 KB) and, when given, the constellation pack (211 KB):

| Operation | Measured (Apple M2 Max, 73.2° phone frame, 734 annotated objects, 20 outlined) |
|---|---|
| Solve | 350–550 ms |
| Annotate a frame | **0.96 ms** |
| … plus constellation figures and boundaries | **1.2 ms** (73.7° field; under 0.1 ms at 8°) |
| Build an annotator | **2.12 ms**, plus 1.2 ms with the constellation pack |

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
`es` `it` `ru` `pl` `hu` `ro`) for **411 named stars, 615 deep-sky and solar-system
objects and the 88 constellations**. It is licensed **GPL-2.0-or-later**, so it is **not bundled by default**; opt in
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

### 1.5 Scale and lens: refined per frame, or calibrated

A solve without a known camera keeps the pixel scale its 4-star pattern measured. On wide
frames that scale can be 1–3% off, which leaves the stars at the edges 5–7 px from their
catalog positions: 73° frames of one phone solved anywhere between 72.7° and 75.6°. By
default (`SolveOptionsDto.refineScale`, C `refine_scale`, Rust `SolveOptions::refine_scale`)
such a solve of a wide field (20° or more) re-measures its scale: it finds the scale at which
the brightest detected stars land on catalog stars, re-solves at that scale and keeps the
result only when they land at least 10% closer (`solution.scaleRefined`). Narrower fields
measure their scale well and keep it, as do solves given a `camera` or an attitude hint.

A phone's wide lens also bends the edges of a frame by several pixels, and annotations drawn
through a pinhole solve inherit that error. By default (`SolveOptionsDto.fitLens`, C
`fit_lens`, Rust `SolveOptions::fit_lens`) a wide solve (20° or more, 30 or more matched
stars, no `camera` given) then fits the focal length and a radial distortion term (k1) to the
frame's own stars, adding a second term (k2) when it predicts left-out stars at least 5%
better and pins the corners down to within 3 px. It re-solves with that lens and keeps it only
when it fits those stars at least 5% better, then fits once more on the stars the new solve
matched, which reach further into the corners: `solution.lensFitted` says so, and
`wcs.camera` then carries the distortion, so every annotation and transform follows the lens.
With a fitted lens, `fov` is the lens's paraxial field of view, a few tenths of a degree away
from a pinhole's.

Together they add 10–20 ms to a wide solve. On 38 phone photos, the stars matched with and
without them sat 2.7 px from their catalog positions on average without either, 2.1 px with
the scale refined and 1.2 px with both (26 of the 32 fitted lenses took k2), and no photo got
worse; the scale of the 73° frames above now comes out between 73.3° and 73.4°. The lens fit
covers only distortion that is radial about the centre; for anything else (and for narrow
fields) calibrate the camera once from several frames:

### 1.5.1 On-device calibration (optional; for narrow fields and distorting lenses)

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
- Desktop builds can also register a narrow-field package (`pool.registerNarrow`, found by
  `openDir` too). It only takes frames whose known FOV is narrower than about 3°, before the
  tetra3 tiers when none covers the FOV and after them otherwise; frames of unknown FOV never
  reach it unless `narrowBlind` is set. Its attempts carry `kind == TierKindDto.narrow`.
  Below about 0.4° give it a pointing (`base.pointingHint`; see §5, item 10).

The plugin bundles one tier, covering 10–80°. The narrower tiers and the narrow-field package
are release assets of the data repository, and `DbManager` installs them from there or from any
static host serving a manifest in the format below (manifest → resumable download → sha256 check
→ decompress → register):

```dart
final mgr = DbManager(
  dir: dir,
  baseUrl: 'https://github.com/UMa-Technology/unisolver-data/releases/download/v3/',
  register: (path) => pool.register(dbPath: path).then((_) {}), // solvable at once, no restart
);
final manifest = await mgr.fetchManifest();          // cached; cachedManifest() works offline
final tier = manifest.byName('unisolver_5_10')!;
await mgr.install(tier, onProgress: (p) => print('${p.phase} ${p.fraction}'));
```

Desktop builds install the **narrow-field package** the same way. It is two files, a blind
index and its star tiles (for 0.18–3.1°: about 2.5 GB to download, 3.2 GB installed), listed
under `packages`, installed together and registered through a `registerPackage` callback:

```dart
final mgr = DbManager(
  dir: dir,
  baseUrl: 'https://github.com/UMa-Technology/unisolver-data/releases/download/v3/',
  register: (path) => pool.register(dbPath: path).then((_) {}),
  registerPackage: (index, stars) =>
      pool.registerNarrow(indexPath: index, starsPath: stars).then((_) {}),
);
final pkg = manifest.packageByName('unisolver_narrow')!;
if (!mgr.isPackageInstalled(pkg)) {
  await mgr.installPackage(pkg, onProgress: (p) => print('${p.name} ${p.phase}'));
}
```

- `installPackage` checks free space first (`pkg.diskBytesNeeded`, about 4.5 GB at the peak,
  less the files already in place), then downloads, verifies and decompresses one file at a
  time. A file already in place is kept, so an interrupted install continues where it stopped.
- Once both files are in place it calls `registerPackage`: the package solves at once, and the
  next `openDir` finds it by its file headers.
- After registering, the engine reads both files once in the background to warm the page
  cache, so the first narrow-field frames do not wait on a cold disk. Set
  `UNISOLVER_NO_PREFETCH=1` to skip it.
- On iOS and Android `installPackage` refuses: mobile builds have no narrow-field engine.
- `removePackage` deletes the files. A pool that registered the package keeps it until the pool
  is reopened (and on Windows the files cannot be deleted until then).

#### Proxies, downloaded files and tiers bundled with the app

- **Proxies.** On desktop, downloads take the proxy from the environment (`https_proxy`, as Dart
  does) and otherwise from the system settings (a manual HTTP or HTTPS proxy on macOS and
  Windows), read at each connection. Auto-configuration (PAC) scripts are not read: use the proxy
  tool's TUN mode or import the files. On iOS and Android, VPN-style proxies apply to the app as
  they are. To choose the proxy yourself, pass `httpClient`; `systemProxy()` returns the
  system's setting.
- **Files downloaded another way** (a browser, a file-sharing link, another computer):
  `importFile` recognises a tier, package file or asset by size and sha256, installs it as a
  download would be and registers it; the file's name does not matter and the file is left
  alone. A package is registered once both of its files are in place.

  ```dart
  final r = await mgr.importFile(pickedPath);   // DbImport: name, kind, path, missing
  if (!r.complete) print('still missing: ${r.missing}');   // the other file of a package
  ```

- **Tiers bundled with the app.** Ship the manifest and the archives you want offline from the
  start (for example `unisolver_5_10.db.zst`) in your app's assets, then on first launch:

  ```dart
  final manifestJson = await rootBundle.loadString('assets/manifest-v3.json');
  mgr.seedManifest(manifestJson);   // kept unless the cache holds a newer revision
  await UnisolverAssets.importBundled(mgr, 'assets/unisolver_5_10.db.zst',
      manifestJson: manifestJson);   // null when already installed
  ```

  The archive stays in the app package and the database is decompressed next to the others,
  so a bundled tier takes its archive plus its database on the device.

Manifest fields (`manifest-v3.json`, version 3; releases before 0.5.0 read `manifest.json`,
version 2):

| Field | Purpose |
|---|---|
| `key` | Download path relative to `base_url`, named after the content: the data release uses `<name>-<last 8 hex of sha256>.<ext>` (for example `unisolver_5_10-cbe3d569.db.zst`); URL = `base_url + key` |
| `sha256` | Digest of the archive; **verified after every download** (`DbManager` refuses a mismatch and removes the leftover) |
| `bytes` / `raw_bytes` | Download size / decompressed size; `tier.diskBytesNeeded` is the peak disk use during install |
| `raw_sha256` | Digest of the decompressed file (tiers and package files), **checked while decompressing**: a mismatch throws `DbChecksumException` and nothing is installed |
| `min_engine` | Oldest engine that reads the file; `DbManager` refuses an entry that needs a newer one and says to upgrade |
| `mobile` | Whether mobile devices should use it; mobile needs `allowNonMobile: true` otherwise |
| `bundled` | Ships with the plugin assets, not on the host; installed by `UnisolverAssets.ensureInstalled` |
| `license` / `attribution` | The tier's data license and the attribution it requires (Gaia DR3 for star databases) |
| `assets` | Optional files used as downloaded (the names pack): `name`, `kind`, `file`, `key`, `bytes`, `sha256`, `license`, `attribution`; install with `DbManager.installAsset` |
| `packages` | Multi-file downloads (the desktop narrow-field package): `name`, `kind` (`blind-index`), `min_fov_deg` / `max_fov_deg`, `mobile` (false), `min_engine`, `license`, `attribution`, informational counts, and `files`, each with `role` (`index` or `stars`), `file`, `key`, `bytes`, `sha256`, `raw_bytes`, `raw_sha256`; install with `DbManager.installPackage` |
| `revision` | Publication counter, raised whenever the content changes (0 when absent); `seedManifest` keeps a cached manifest at least as new as the bundled one |

Behaviour (all covered by tests): an interrupted download keeps its `.part` and resumes
with `Range`; a server ignoring `Range` gets a clean restart; a digest mismatch deletes
the bad content (otherwise every resume would continue from bad bytes); a failed
decompression leaves no partial database and reports the space needed; a decompressed file
that does not match `raw_sha256` is removed with its archive, so the next install downloads
again.

Databases are tetra3's format 2, memory-mapped: the pattern table stays on the file, so
**resident memory follows the pages touched, not the file size** (the bundled database plus
a phone frame peaks at about 57 MB). Each extra tier costs its star catalog plus the pages
it touches.

### 1.7 Logs and errors

```dart
setLogStream(sink: ...);   // bridges Rust tracing logs into Dart; connect your logger
```

Engine errors surface as `AnyhowException` with the root cause in the message (corrupt
file, unsupported format, database version mismatch, …).

---

## 2. C ABI (INDI / ASCOM / native desktop and mobile / Python)

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

For native mobile apps (Flutter apps use the plugin instead):

- **iOS** (`unisolver-cabi-vX.Y.Z-ios.zip`): add `unisolver.xcframework` to the app target
  (Frameworks, Libraries, and Embedded Content). It is a static library for devices (arm64) and
  simulators (arm64, x86_64), iOS 12 or later, so there is nothing to embed or sign. Its headers
  carry a module map: Swift can `import unisolver` and call the C functions directly (strings
  come back as `UnsafePointer<CChar>`: copy with `String(cString:)`, then free as documented
  below); Objective-C includes `unisolver.h`.
- **Android** (`unisolver-cabi-vX.Y.Z-android.zip`): `lib/<abi>/libunisolver_cabi.so` for
  `arm64-v8a` and `x86_64`, API 21 or later, 16 KB page aligned as Google Play requires. Copy
  them into `src/main/jniLibs/<abi>/`, or import them in CMake
  (`add_library(unisolver_cabi SHARED IMPORTED)` with `IMPORTED_LOCATION`), and call them from
  your JNI or NDK code with `include/unisolver.h`.

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
| `unisolver_pool_register_narrow(pool, index_path, stars_path, &err)` | Register the narrow-field package (desktop builds) → tier JSON with `"kind":"narrow"` |
| `unisolver_pool_tiers_json(pool, &err)` | Registered tiers plus files skipped at open |
| `unisolver_pool_solve_image_json(pool, path, &err)` | Solve **without naming a tier**; the JSON adds `db` |
| `unisolver_pool_solve_image_json_opts(pool, path, opts, &err)` | The same with options (a known FOV tries only tiers covering it) |
| `unisolver_pool_solve_frame_json_opts(pool, px, len, w, h, kind, stride, opts, &err)` | Direct frame input with pool routing |
| `unisolver_pool_close(pool)` | Release a pool |
| `unisolver_satellites_json(tle, unix_ms, lat, lon, alt, &err)` | TLE + SGP4 → topocentric positions (JSON array) |
| `unisolver_annotator_open(solver, dso, names, &err)` | Build an annotator (both paths may be NULL); **build once and keep it** |
| `unisolver_pool_annotator_open(pool, db_name, dso, names, &err)` | The same from one tier of a pool (`db_name` = the solve JSON's `db`) |
| `unisolver_annotator_load_constellations(annotator, path, &err)` | Load the constellation pack (figures and IAU boundaries) → true when loaded |
| `unisolver_annotator_constellation_at_json(annotator, ra, dec, language, &err)` | The constellation containing a J2000 position → `{"abbr","name"}`, or `null` without the pack |
| `unisolver_annotate_json(annotator, wcs_json, opts_json, &err)` | Annotate a frame from the solve JSON's `wcs` → annotation JSON |
| `unisolver_wcs_sky_to_pixels(wcs_json, in, n, out, &err)` | Batch `ra, dec` → `x, y` through the solve's lens model (NaN where it cannot place a point) |
| `unisolver_wcs_pixels_to_sky(wcs_json, in, n, out, &err)` | Batch `x, y` → `ra, dec` |
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
matches, RMSE, the **full WCS** and the plate scale), plus `observation_unix_ms` and
`observer` (the time the solve used and, for files with EXIF GPS, where the photo was taken;
null otherwise). This entry carries the same "header hints + ladder + range clamp" strategy
as the Flutter path, EXIF included.

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
parallax; required by satellites), `satellite_tle`, `include_constellations` /
`constellation_boundaries` (after `unisolver_annotator_load_constellations`; the JSON then
has `constellations` and `boundaries`), `equatorial_grid` / `horizontal_grid` /
`grid_spacing_px` (the JSON's `grid`), and `viewport` (`{"x", "y", "width", "height",
"scale"}`: what you show, see §1.4).

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
`match_threshold` / `timeout_ms` / `observation_unix_ms` / `observer_velocity_km_s` /
`focal_length_35mm` (ladders only: an EXIF 35 mm focal length read by the caller, tried
first). `observation_unix_ms` is only reported back. `observer_velocity_km_s` corrects stellar
aberration, which makes the solution the camera's physical pointing, up to 20″ from the catalog
frame the annotation uses: leave it unset when annotating or syncing a mount.
`attitude_hint_wxyz` without `fov_deg` or `camera` is an **error**: tracking needs the
scale, and silently falling back to a blind solve would hide that tracking is not working.

Pools with the narrow-field engine (desktop builds) also take `pointing_hint`
(`{"ra_deg", "dec_deg", "radius_deg"}`, the radius optional), `narrow_blind` and
`narrow_fallback_ms`; pool file entries fill the pointing in from FITS and XISF headers. Other
solves ignore them. Below about 0.4° pass a pointing whenever you have one (§5, item 10).

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
`fov_deg`/`camera` the fallback is the aspect ladder rather than header hints. A photo
decoded by the platform (HEIC) passes its EXIF as `focal_length_35mm` and
`observation_unix_ms`. Live and tracking use should pass the FOV and the previous attitude
anyway.

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
| `unisolver_dso.bin` (bundled asset) | 777 KiB (~481 KiB compressed in the APK) | DSO annotation catalog with outlines; omit it if you do not annotate |
| `unisolver_constellations.bin` (bundled asset) | 211 KiB (~173 KiB compressed) | constellation figures, IAU boundaries, the lookup table and the art anchors |
| `optional/unisolver_art_western_new.bin` (opt-in asset) | 3.2 MiB (85 webp) | constellation illustrations, painted set; `unisolver_art_western.bin` (low-poly) is 2.8 MiB |

**About 3.3 MiB installed / 3.0 MiB download per architecture** (Android arm64, without
databases), plus about 3 MiB for a set of constellation illustrations if you opt into one
(webp, which the store cannot compress further). Shipping both arm64-v8a and x86_64 doubles the native part (x86_64 is only for
emulators). Integrating through the **C ABI** is smaller: `libunisolver_cabi.dylib` (macOS
arm64, stripped) is **1.67 MiB**, without the flutter_rust_bridge / serde_json /
tracing-subscriber layer.

### Data files

| File | Purpose | Size (zstd / raw) | Distribution |
|---|---|---|---|
| `unisolver_10_80.db` | wide field 10–80° (phones) | 14 MB / 29 MB | bundled with the plugin |
| `unisolver_dso.bin` | DSO catalog (NGC / IC / Messier) with outlines | 796 KB | bundled with the plugin |
| `unisolver_constellations.bin` | 88 IAU constellation figures and boundaries | 200 KB | bundled with the plugin |
| `unisolver_names.bin` | names in 13 languages (GPL-2.0-or-later) | 229 KB | opt-in: declared by the app, or downloaded |
| `unisolver_5_10.db` | 5–10° | 23 MB / 41 MB | from the data release (`DbManager.install`) |
| `unisolver_2p5_5.db` | 2.5–5° | 108 MB / 165 MB | from the data release |
| `unisolver_1_2p5.db` | 1–2.5°, desktop builds | 287 MB / 400 MB | from the data release |
| narrow-field package (blind index + star tiles) | narrow fields 0.18–3.1°, desktop builds | 2.5 GB / 3.2 GB | not bundled; from the data release (`DbManager.installPackage`) |

- Databases are tetra3's format 2, memory-mapped. Files from releases before it (the
  `UNISOLV2` container) no longer load; download them again.
- Only the wide tier is bundled. The narrower tiers and the narrow-field package are release
  assets of the data repository, [unisolver-data](https://github.com/UMa-Technology/unisolver-data)
  (section 1.6); you can also generate and host your own (`unisolver-starmatch` has the package
  builders).
- Mobile devices should stay at ≥ 2.5° tiers: phones have no narrower fields, and deeper
  tiers are too large to keep resident on mobile. The narrow-field package is for desktop
  builds only.
- **Attribution is required**: the star databases and the narrow-field package derive from
  Gaia DR3 (ESA/Gaia/DPAC, CC BY-SA 3.0 IGO). Keep this in your app's About page, for example:
  *This work has made use of data from the European Space Agency (ESA) mission Gaia,
  processed by the Gaia Data Processing and Analysis Consortium (DPAC).*
  The DSO catalog derives from OpenNGC (CC BY-SA 4.0), the constellation pack from the IAU
  charts and boundaries via Stellarium's modern (IAU) sky culture (CC BY-SA 4.0), and the
  names pack from Stellarium (GPL-2.0-or-later); see `THIRD_PARTY_LICENSES.md`.

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
5. **Frames without stars** fail in under two seconds (measured 1.7–1.9 s on real frames;
   about 2.3 s with a FOV hint, which adds a rung and a longer probe). Use `attempts` to tell
   the user no stars were found; do not retry in a loop. With `thorough: true` failures take
   as long as the exhaustive search (10–16 s).
6. **Large frames**: frames above 16 Mpx are extracted in horizontal bands, so a 26 Mpx
   astro frame peaks around 290 MB including a 320 MB narrow tier (630 MB before banding);
   phone frames up to 4K take a single pass. On mobile, still prefer downsampling 48 Mpx
   originals to ≤ 4K: fewer pixels is less work, and the FOV does not change.
7. **Windows builds**: the DLL must be built on a Windows host (the dependency chain
   includes dart-sys); macOS can cross-check but not produce it.
8. **macOS sandbox needs network access**: to download with `DbManager`, the host's
   `*.entitlements` must include `com.apple.security.network.client` (the Flutter template
   does **not**). Without it you get a `SocketException`, not a permission error. Copy the
   example's `macos/Runner/{DebugProfile,Release}.entitlements`.
9. **Narrow frames without a hint sweep the ladder**: without a header FOV hint, a 3°
   frame routed through a multi-tier pool took 3 s (measured). Give a hint whenever you can
   (EXIF/FITS headers, `focalLength35Mm` for decoded photos, or a calibrated camera): one
   attempt instead of a dozen. Photos forwarded through chat apps usually lose their EXIF.
10. **Very narrow fields want a pointing** (desktop narrow-field package): below about 0.4° a
    blind solve needs more faint stars than fields away from the Milky Way usually hold. On
    simulated frames it solved about half the poses at 0.18°, three in four at 0.25° and nine
    in ten at 0.3°, the failures far from the Milky Way; with a pointing within a third of the
    field, 95% solved at 0.18° and all of them at 0.25–0.6°. Pass `pointingHint` (C:
    `pointing_hint`) with the mount's RA/Dec, or keep RA/Dec in the FITS/XISF header, which
    pool file entries read.
11. **A lit foreground**: trees, buildings or equipment lit by streetlights or windows can
    outshine the stars of a light-polluted sky. Blind wide-field solves spread their centroids
    over the frame (§1.3), which copes with a foreground in part of the frame; one that
    surrounds a narrow strip of sky, as between two lit buildings, still leaves too few stars.
    Point higher, or frame more sky.

## 6. Versions and compatibility

- Database files: tiers are tetra3's format 2 from 0.5.0, and files from earlier releases no
  longer load (download them again). Manifests record the oldest engine that reads each
  database file (`min_engine`); `DbManager` refuses one that needs a newer engine and says
  to upgrade.
- API: the Dart and C surfaces follow semantic versioning. Informational fields such as
  `attempts` and `layers.reasons` may gain entries; parse them tolerantly.
