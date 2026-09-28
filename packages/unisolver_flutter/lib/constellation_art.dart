import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// Illustration sets for the constellation art layer (`AnnotateOptionsDto.constellationArt`),
/// keyed by IAU abbreviation as `ConstellationAnnotationDto.abbr`. Each set's NOTICE and
/// `dataAttributions()` give its author and license.
enum ConstellationArtSet {
  /// Painted, CC BY-SA 4.0. Bundled with the plugin (one webp per constellation).
  westernNew(
    'packages/unisolver_flutter/assets/art/western_new',
    packed: false,
  ),

  /// Low-poly, Free Art License 1.3. Optional, as one file: declare
  /// `packages/unisolver_flutter/optional/unisolver_art_western.bin` in your app's assets.
  western(
    'packages/unisolver_flutter/optional/unisolver_art_western.bin',
    packed: true,
  );

  const ConstellationArtSet(this.asset, {required this.packed});

  /// Asset directory, or the asset key of the pack
  final String asset;

  /// Whether the set is one art pack file ([parseArtPack]) rather than a directory
  final bool packed;

  /// The illustration of [abbr] as encoded image bytes (webp; decode with
  /// `ui.instantiateImageCodec`), or null when the set has none for it or the app does not
  /// ship the set.
  Future<Uint8List?> load(String abbr) async {
    try {
      if (!packed) {
        final data = await rootBundle.load('$asset/$abbr.webp');
        return data.buffer.asUint8List(data.offsetInBytes, data.lengthInBytes);
      }
      final images = await (_packs[this] ??= rootBundle
          .load(asset)
          .then(parseArtPack));
      return images[abbr];
    } on FlutterError {
      return null;
    }
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
