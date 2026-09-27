import 'package:unisolver_flutter/unisolver_flutter.dart';

/// FOV preset ladder (the one solvecli validated on 47 phone frames): landscape main cameras
/// span 60–76° horizontally; portrait horizontal = the short-side angle, about 44–48°.
List<FovPresetDto> ladderFor(int width, int height) {
  final landscape = width >= height;
  return [
    if (landscape) ...[
      FovPresetDto(fovDeg: 70, maxErrorDeg: 9),
      FovPresetDto(fovDeg: 55, maxErrorDeg: 8),
      FovPresetDto(fovDeg: 42, maxErrorDeg: 7),
    ] else ...[
      FovPresetDto(fovDeg: 46, maxErrorDeg: 7),
      FovPresetDto(fovDeg: 60, maxErrorDeg: 8),
      FovPresetDto(fovDeg: 33, maxErrorDeg: 6),
    ],
    FovPresetDto(fovDeg: 20, maxErrorDeg: 5),
    FovPresetDto(fovDeg: 13, maxErrorDeg: 3.5),
  ];
}

/// Per-model presets: a match gives the horizontal FOV directly, skipping the ladder.
/// Filled in as devices are measured; no match returns null and the ladder runs.
const Map<String, double> _deviceHorizontalFov = {
  // 'iPhone17,1': 73.7,   // example: iPhone 16 Pro main camera, landscape
  // 'SM-S9280': 74.2,     // example: Galaxy S24 Ultra main camera, landscape
};

double? presetForDeviceModel(String model) => _deviceHorizontalFov[model];
