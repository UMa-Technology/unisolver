import 'dart:typed_data';

import 'package:unisolver_flutter/unisolver_flutter.dart';

/// Camera YUV420 Y plane → FrameDto without copying rows: bytesPerRow passes straight through
/// and Rust reads the first `width` bytes of each row by row_stride.
FrameDto yPlaneToFrameDto({
  required int width,
  required int height,
  required int bytesPerRow,
  required Uint8List bytes,
}) {
  return FrameDto(
    width: width,
    height: height,
    rowStrideBytes: bytesPerRow == width ? null : bytesPerRow,
    kind: PixelKindDto.luma8,
    bytes: bytes,
  );
}
