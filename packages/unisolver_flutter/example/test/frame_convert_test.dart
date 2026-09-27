import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:unisolver_flutter/unisolver_flutter.dart';
import 'package:unisolver_flutter_example/frame_convert.dart';

void main() {
  test('y plane with padding keeps stride, no repack', () {
    final bytes = Uint8List.fromList(
      List.generate(2 * 4, (i) => i),
    ); // 2 rows, stride 4, width 3
    final f = yPlaneToFrameDto(
      width: 3,
      height: 2,
      bytesPerRow: 4,
      bytes: bytes,
    );
    expect(f.rowStrideBytes, 4);
    expect(f.kind, PixelKindDto.luma8);
    expect(f.bytes, same(bytes)); // not copied
  });
  test('tight rows omit stride', () {
    final f = yPlaneToFrameDto(
      width: 4,
      height: 1,
      bytesPerRow: 4,
      bytes: Uint8List(4),
    );
    expect(f.rowStrideBytes, isNull);
  });
}
