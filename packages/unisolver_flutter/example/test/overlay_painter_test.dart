import 'dart:typed_data';

import 'package:flutter/painting.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:unisolver_flutter/unisolver_flutter.dart';
import 'package:unisolver_flutter_example/fov_presets.dart';
import 'package:unisolver_flutter_example/overlay_painter.dart';

void main() {
  test('grid labels read like a chart and sit inside the visible edge', () {
    GridLineDto line(GridKindDto kind, String text, [String? cardinal]) =>
        GridLineDto(
          system: kind == GridKindDto.ra || kind == GridKindDto.dec
              ? GridSystemDto.equatorial
              : GridSystemDto.horizontal,
          kind: kind,
          valueDeg: 0,
          text: text,
          cardinal: cardinal,
          lines: const [],
        );
    expect(gridLabelText(line(GridKindDto.ra, '16h30m')), '16h30m');
    expect(gridLabelText(line(GridKindDto.az, '180°', 'S')), 'S 180°');
    expect(gridLabelText(line(GridKindDto.horizon, '0°')), 'Horizon');
    const text = Size(40, 12);
    expect(gridLabelOffset(GridEdgeDto.left, text), const Offset(4, -6));
    expect(gridLabelOffset(GridEdgeDto.bottom, text), const Offset(-20, -16));
    expect(gridLabelOffset(GridEdgeDto.right, text).dx, -44);
  });
  test('constellation polylines stay open', () {
    final path = polylinePath([0, 0, 10, 0, 10, 10]);
    final b = path.getBounds();
    expect(b, const Rect.fromLTRB(0, 0, 10, 10));
    // Not closed: the point diagonally inside the L is outside the stroke's area
    expect(path.contains(const Offset(2, 8)), isFalse);
  });
  test('ladder is aspect aware', () {
    expect(ladderFor(1920, 1080).first.fovDeg, 70);
    expect(ladderFor(720, 1280).first.fovDeg, 46);
  });
  test('unknown device model falls back to null', () {
    expect(presetForDeviceModel('UnknownPhone1,0'), isNull);
  });
  test('dso without angle renders as circle of semi-major', () {
    final r = dsoRenderShape(semiMajorPx: 40, semiMinorPx: 10, angleDeg: null);
    expect(r.isCircle, isTrue);
    expect(r.radius, 40);
  });
  test('dso with angle keeps ellipse axes', () {
    final r = dsoRenderShape(semiMajorPx: 40, semiMinorPx: 10, angleDeg: 30);
    expect(r.isCircle, isFalse);
    expect(r.semiMinor, 10);
  });
  test('off-frame outlined object labels at its first in-image vertex', () {
    final outlines = [
      DsoOutlineDto(
        level: 1,
        contours: [
          OutlineContourDto(
            closed: true,
            points: Float64List.fromList([-50, -50, 20, 30, 40, 60]),
          ),
        ],
      ),
    ];
    const size = Size(100, 100);
    expect(outlineLabelAnchor(-80, 10, outlines, size), const Offset(20, 30));
    expect(outlineLabelAnchor(50, 50, outlines, size), const Offset(50, 50));
  });

  test('art mesh becomes textured triangles, skipping unplaced vertices', () {
    // 3 × 3 grid on a 100 × 100 image; the lower-right vertex cannot be placed
    final pts = <double>[];
    for (var r = 0; r < 3; r++) {
      for (var c = 0; c < 3; c++) {
        final missing = r == 2 && c == 2;
        pts.addAll(missing ? [double.nan, double.nan] : [c * 10.0, r * 10.0]);
      }
    }
    final art = ConstellationArtDto(
      cols: 3,
      rows: 3,
      points: Float64List.fromList(pts),
    );
    final tri = artTriangles(art, const Size(100, 100), (x, y) => Offset(x, y));
    // 4 cells × 2 triangles, less the one triangle using the missing vertex
    expect(tri.positions.length, 7 * 3);
    expect(tri.textureCoordinates.length, tri.positions.length);
    expect(tri.textureCoordinates.first, Offset.zero);
    expect(tri.textureCoordinates, contains(const Offset(100, 50)));
    expect(tri.positions, contains(const Offset(20, 10)));
    final none = artTriangles(
      ConstellationArtDto(
        cols: 2,
        rows: 2,
        points: Float64List.fromList(List.filled(8, double.nan)),
      ),
      const Size(1, 1),
      (x, y) => Offset(x, y),
    );
    expect(none.positions, isEmpty);
  });
}
