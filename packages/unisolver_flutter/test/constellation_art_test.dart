import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:unisolver_flutter/constellation_art.dart';

/// An art pack as the generator writes it: `UART`, version 1, a u16 count, then per image a
/// u8 name length, the name, u32 offset and u32 length (little-endian), then the images.
Uint8List pack(Map<String, List<int>> images) {
  final names = images.keys.toList();
  final header = 4 + 1 + 2 + names.fold<int>(0, (n, k) => n + 1 + k.length + 8);
  final b = BytesBuilder()
    ..add('UART'.codeUnits)
    ..addByte(1)
    ..add(
      (ByteData(
        2,
      )..setUint16(0, names.length, Endian.little)).buffer.asUint8List(),
    );
  var offset = header;
  for (final k in names) {
    final entry = ByteData(8)
      ..setUint32(0, offset, Endian.little)
      ..setUint32(4, images[k]!.length, Endian.little);
    b
      ..addByte(k.length)
      ..add(k.codeUnits)
      ..add(entry.buffer.asUint8List());
    offset += images[k]!.length;
  }
  for (final k in names) {
    b.add(images[k]!);
  }
  return b.toBytes();
}

void main() {
  test('an art pack gives each illustration by abbreviation', () {
    final p = pack({
      'Ori': [1, 2, 3],
      'CVn': [9],
    });
    final images = parseArtPack(ByteData.sublistView(p));
    expect(images.keys, ['Ori', 'CVn']);
    expect(images['Ori'], [1, 2, 3]);
    expect(images['CVn'], [9]);
  });

  test('a file that is not an art pack is refused', () {
    expect(
      () => parseArtPack(
        ByteData.sublistView(Uint8List.fromList('NOPE'.codeUnits)),
      ),
      throwsFormatException,
    );
    final p = pack({
      'Ori': [1, 2, 3],
    });
    expect(
      () => parseArtPack(ByteData.sublistView(p, 0, p.length - 1)),
      throwsFormatException,
      reason: 'an image runs past the end',
    );
  });
}
