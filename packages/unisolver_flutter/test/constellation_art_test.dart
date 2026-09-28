import 'dart:io';
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
  TestWidgetsFlutterBinding.ensureInitialized();

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

  test('a set the app did not declare gives no images', () async {
    // Neither set is bundled with the plugin
    expect(await ConstellationArtSet.westernNew.load('Ori'), isNull);
    expect(await ConstellationArtSet.western.load('Ori'), isNull);
  });

  group('images on disk', () {
    late Directory dir;
    setUp(() => dir = Directory.systemTemp.createTempSync('art'));
    tearDown(() => dir.deleteSync(recursive: true));

    void write(String name, List<int> bytes) => (File(
      '${dir.path}/$name',
    )..createSync(recursive: true)).writeAsBytesSync(bytes);

    test('load by abbreviation from a directory of images', () async {
      write('Ori.webp', [1]);
      write('CVn.png', [2]);
      final art = ConstellationArtFiles(dir.path);
      expect(await art.load('Ori'), [1]);
      expect(await art.load('CVn'), [2]);
      expect(await art.load('And'), isNull);
    });

    test('a sky culture maps abbreviations through its index', () async {
      write(
        'index.json',
        '''{"constellations": [
        {"iau": "Ori", "image": {"file": "illustrations/orion.webp"}},
        {"iau": "Cvn", "image": {"file": "illustrations/canes_venatici.webp"}},
        {"iau": "Pup"}
      ]}'''
            .codeUnits,
      );
      write('illustrations/orion.webp', [1]);
      write('illustrations/canes_venatici.webp', [2]);
      write('Pup.webp', [3]);
      final art = ConstellationArtFiles(dir.path);
      expect(await art.load('Ori'), [1]);
      // The culture spells it Cvn; the annotations use the IAU's CVn
      expect(await art.load('CVn'), [2]);
      // Listed without an image: with an index the index alone decides, so a stray file
      // named by the abbreviation does not count
      expect(await art.load('Pup'), isNull);
    });
  });
}
