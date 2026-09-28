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

# Optional: constellation illustrations

Two sets of illustrations for the art layer (`AnnotateOptionsDto.constellationArt`), one
pack file each. **Neither is bundled**, to keep apps small:

| Pack | Set | License |
|---|---|---|
| `unisolver_art_western_new.bin` | `ConstellationArtSet.westernNew`, painted | CC BY-SA 4.0 (`unisolver_art_western_new.NOTICE.txt`) |
| `unisolver_art_western.bin` | `ConstellationArtSet.western`, low-poly | Free Art License 1.3 (`unisolver_art_western.NOTICE.txt`) |

Declare the one you want in **your app's** `pubspec.yaml`:

```yaml
flutter:
  assets:
    - packages/unisolver_flutter/optional/unisolver_art_western_new.bin
```

and load images with `ConstellationArtSet.westernNew.load(abbr)` (null when not declared).

An app that already carries the drawings, for example in a resource archive it unpacks, can
skip the pack and read its own copy with `ConstellationArtFiles(directory).load(abbr)`: a
Stellarium sky culture directory (`index.json` plus its images) or images named by IAU
abbreviation (`Ori.webp`). The anchors are those of Stellarium's western sky cultures, so
the drawings must be theirs (any resolution).

Show the attribution from `dataAttributions()` (`constellation_art` for the painted set,
`constellation_art_western` for the low-poly one) and keep the NOTICE with the file when you
distribute it.
