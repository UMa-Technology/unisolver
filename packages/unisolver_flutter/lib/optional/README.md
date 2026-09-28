# Optional: multilingual names pack

`unisolver_names.bin` holds object names in 13 languages (411 named stars, 615 deep-sky
and solar-system objects, 88 constellations). It is **not bundled by default** because it is
licensed **GPL-2.0-or-later** (it derives from Stellarium's names and translations), while
the plugin itself is MIT OR Apache-2.0. Without it, annotation still works with English
names.

## Using it

Either bundle it with your app, by declaring it in **your app's** `pubspec.yaml`:

```yaml
flutter:
  assets:
    - packages/unisolver_flutter/optional/unisolver_names.bin
```

and installing it at startup:

```dart
final namesPath = await UnisolverAssets.installNames(); // null if not declared
```

or download it from a host that serves it in the manifest's `assets` list, with
`DbManager.installAsset(manifest.assetByName('unisolver_names')!)`.

## License obligations

When you distribute the file you distribute GPL-2.0-or-later data:

- keep this notice and `LICENSE-GPL-2.0` with it, and show the attribution
  "Object names and translations from Stellarium (GPL-2.0-or-later)";
- make the corresponding source available: `tools/namesgen/`,
  `tools/namesgen/data/objects-names.json` and `tools/namesgen/data/constellation-names.json`
  in the unisolver repository.

# Optional: western constellation illustrations

`unisolver_art_western.bin` holds a second set of constellation illustrations for the art
layer (`AnnotateOptionsDto.constellationArt`): low-poly drawings under the **Free Art
License 1.3** (see `unisolver_art_western.NOTICE.txt`). The painted set
(`ConstellationArtSet.westernNew`, CC BY-SA 4.0) is bundled; this one is not, to keep apps
small. Declare it in **your app's** `pubspec.yaml`:

```yaml
flutter:
  assets:
    - packages/unisolver_flutter/optional/unisolver_art_western.bin
```

and load images with `ConstellationArtSet.western.load(abbr)` (null when not declared). Show
the attribution from `dataAttributions()` (`constellation_art_western`) and keep the NOTICE
with the file when you distribute it.
