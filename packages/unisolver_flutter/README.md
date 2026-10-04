# unisolver_flutter

Flutter plugin for [unisolver](https://github.com/UMa-Technology/unisolver), an offline plate
solver: it finds where a night-sky photo points (RA/Dec, roll, field of view, WCS) and
labels the stars, deep-sky objects, planets and satellites in it. The Rust engine is
compiled for each platform by cargokit and called through flutter_rust_bridge; nothing
goes online.

Platforms: iOS, Android (arm64-v8a, x86_64), macOS, Windows (x64, arm64).

## Install

Not published to pub.dev; depend on it through git (a Rust toolchain is required, see
[docs/building.md](../../docs/building.md)):

```yaml
dependencies:
  unisolver_flutter:
    git:
      url: https://github.com/UMa-Technology/unisolver
      path: packages/unisolver_flutter
```

## Use

```dart
await RustLib.init();
final paths = await UnisolverAssets.ensureInstalled();
final solver = await UniSolver.newInstance(dbPath: paths.dbPath);
// solve, track, annotate, calibrate: see the integration guide
```

The full API, the options and the data license obligations are in
[docs/integration.md](../../docs/integration.md). `example/` is a working app with solve,
live tracking, calibration and database pages.

## License

The code is MIT OR Apache-2.0 (see `LICENSE`); desktop builds also compile in
`unisolver-starmatch`, derived from seiza, under Apache-2.0. The bundled star database (Gaia DR3,
CC BY-SA 3.0 IGO) and DSO catalog (OpenNGC, CC BY-SA 4.0) require attribution; the opt-in
names pack in `lib/optional/` is GPL-2.0-or-later. Details in
[THIRD_PARTY_LICENSES.md](../../THIRD_PARTY_LICENSES.md).
