import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// Where the illustrations of the constellation art layer (`AnnotateOptionsDto.constellationArt`)
/// come from: a set the plugin provides ([ConstellationArtSet]) or a copy the app keeps on
/// disk ([ConstellationArtFiles]). Images are keyed by IAU abbreviation, as
/// `ConstellationAnnotationDto.abbr`.
abstract interface class ConstellationArtSource {
  /// The illustration of [abbr] as encoded image bytes (decode with
  /// `ui.instantiateImageCodec`), or null when there is none for it.
  Future<Uint8List?> load(String abbr);
}

/// Illustration sets the plugin provides, one pack file each. **Neither is bundled**: declare
/// a set's pack in your app's `pubspec.yaml` to ship it; [load] returns null otherwise. Each
/// set's NOTICE and `dataAttributions()` give its author and license.
enum ConstellationArtSet implements ConstellationArtSource {
  /// Painted, CC BY-SA 4.0: declare
  /// `packages/unisolver_flutter/optional/unisolver_art_western_new.bin`.
  westernNew(
    'packages/unisolver_flutter/optional/unisolver_art_western_new.bin',
  ),

  /// Low-poly, Free Art License 1.3: declare
  /// `packages/unisolver_flutter/optional/unisolver_art_western.bin`.
  western('packages/unisolver_flutter/optional/unisolver_art_western.bin');

  const ConstellationArtSet(this.asset);

  /// Asset key of the set's pack ([parseArtPack])
  final String asset;

  @override
  Future<Uint8List?> load(String abbr) async {
    try {
      final images = await (_packs[this] ??= rootBundle
          .load(asset)
          .then(parseArtPack));
      return images[abbr];
    } on FlutterError {
      return null;
    }
  }
}

/// Illustrations the app keeps on disk, for example unpacked from its own resource archive:
/// a Stellarium sky culture directory (an `index.json` whose constellations give their `iau`
/// abbreviation and `image.file`), or a directory of images named by IAU abbreviation
/// (`Ori.webp`, `Ori.png`, `Ori.jpg`). The layer places the drawings of Stellarium's western
/// sky cultures, whose anchors are in 0–1 image coordinates, so any resolution fits; other
/// drawings would not line up with their stars. Throws [FormatException] when `index.json`
/// is not JSON.
class ConstellationArtFiles implements ConstellationArtSource {
  ConstellationArtFiles(this.directory);

  /// The sky culture's directory, or the directory of images named by abbreviation
  final String directory;

  static const _extensions = ['webp', 'png', 'jpg'];

  /// `index.json`'s image files by lower-case abbreviation (sky cultures spell some
  /// abbreviations differently, as `Cvn` for `CVn`); null without an index
  late final Future<Map<String, String>?> _index = _readIndex();

  Future<Map<String, String>?> _readIndex() async {
    final f = File('$directory/index.json');
    if (!await f.exists()) return null;
    final json = jsonDecode(await f.readAsString());
    final list = json is Map ? json['constellations'] : null;
    return {
      for (final c in list is List ? list : const [])
        if (c is Map &&
            c['iau'] is String &&
            c['image'] is Map &&
            c['image']['file'] is String)
          (c['iau'] as String).toLowerCase(): c['image']['file'] as String,
    };
  }

  @override
  Future<Uint8List?> load(String abbr) async {
    // With an index, the index alone decides
    final index = await _index;
    final names = index == null
        ? [for (final e in _extensions) '$abbr.$e']
        : [?index[abbr.toLowerCase()]];
    for (final name in names) {
      final f = File('$directory/$name');
      if (await f.exists()) return f.readAsBytes();
    }
    return null;
  }
}

final Map<ConstellationArtSet, Future<Map<String, Uint8List>>> _packs = {};

/// Reads an art pack: `UART`, version 1, a u16 image count, then per image a u8 name length,
/// the name (IAU abbreviation, ASCII), a u32 offset and a u32 length into the file
/// (little-endian), then the images. Throws [FormatException] on anything else.
Map<String, Uint8List> parseArtPack(ByteData data) {
  Never bad(String why) => throw FormatException('not an art pack: $why');
  if (data.lengthInBytes < 7 ||
      ascii.decode(data.buffer.asUint8List(data.offsetInBytes, 4)) != 'UART') {
    bad('magic');
  }
  if (data.getUint8(4) != 1) bad('version ${data.getUint8(4)}');
  final count = data.getUint16(5, Endian.little);
  final out = <String, Uint8List>{};
  var p = 7;
  for (var i = 0; i < count; i++) {
    if (p + 1 > data.lengthInBytes) bad('index truncated');
    final n = data.getUint8(p);
    if (p + 1 + n + 8 > data.lengthInBytes) bad('index truncated');
    final name = ascii.decode(
      data.buffer.asUint8List(data.offsetInBytes + p + 1, n),
    );
    final offset = data.getUint32(p + 1 + n, Endian.little);
    final length = data.getUint32(p + 1 + n + 4, Endian.little);
    if (offset + length > data.lengthInBytes) bad('$name runs past the end');
    out[name] = Uint8List.sublistView(data, offset, offset + length);
    p += 1 + n + 8;
  }
  return out;
}
