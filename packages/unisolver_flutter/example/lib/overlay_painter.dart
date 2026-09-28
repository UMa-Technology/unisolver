import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:unisolver_flutter/unisolver_flutter.dart';

import 'label_layout.dart';

/// DSO shape decision (a testable pure function): with unknown orientation
/// (angleDeg == null) draw a circle of the semi-major axis, never guess an angle.
({bool isCircle, double radius, double semiMinor, double angleRad})
dsoRenderShape({
  required double semiMajorPx,
  required double semiMinorPx,
  required double? angleDeg,
}) {
  if (angleDeg == null) {
    return (
      isCircle: true,
      radius: semiMajorPx,
      semiMinor: semiMajorPx,
      angleRad: 0,
    );
  }
  return (
    isCircle: false,
    radius: semiMajorPx,
    semiMinor: semiMinorPx,
    angleRad: angleDeg * 3.141592653589793 / 180.0,
  );
}

/// Outline contour as a path; points are interleaved image pixels (x0, y0, x1, y1, ...).
Path outlinePath(OutlineContourDto c) {
  final p = c.points;
  final path = Path();
  for (var i = 0; i + 1 < p.length; i += 2) {
    if (i == 0) {
      path.moveTo(p[i], p[i + 1]);
    } else {
      path.lineTo(p[i], p[i + 1]);
    }
  }
  if (c.closed) path.close();
  return path;
}

/// An open polyline from interleaved image pixels (x0, y0, x1, y1, ...): constellation
/// figures and boundaries.
Path polylinePath(List<double> p) {
  final path = Path();
  for (var i = 0; i + 1 < p.length; i += 2) {
    if (i == 0) {
      path.moveTo(p[i], p[i + 1]);
    } else {
      path.lineTo(p[i], p[i + 1]);
    }
  }
  return path;
}

/// Label anchor for an outlined object: its centre when inside the image, otherwise the
/// first in-image vertex of its outermost outline (the centre may lie off-frame).
Offset outlineLabelAnchor(
  double x,
  double y,
  List<DsoOutlineDto> outlines,
  Size size,
) {
  bool inside(double px, double py) =>
      px >= 0 && py >= 0 && px < size.width && py < size.height;
  if (inside(x, y)) return Offset(x, y);
  for (final lv in outlines) {
    for (final c in lv.contours) {
      final p = c.points;
      for (var i = 0; i + 1 < p.length; i += 2) {
        if (inside(p[i], p[i + 1])) return Offset(p[i], p[i + 1]);
      }
    }
  }
  return Offset(x, y);
}

/// The viewer's zoom, read from the x axis: `getMaxScaleOnAxis` also counts z, which a 2D
/// fit may leave at 1.
double viewerScale(Matrix4 m) =>
    math.sqrt(m.storage[0] * m.storage[0] + m.storage[1] * m.storage[1]);

/// Grid label text: the compass point first when there is one (`S 180°`); the horizon by name.
String gridLabelText(GridLineDto g) => switch (g.kind) {
  GridKindDto.horizon => 'Horizon',
  _ => g.cardinal == null ? g.text : '${g.cardinal} ${g.text}',
};

/// Where to put a label's top-left corner so the text sits just inside the visible edge the
/// engine anchored it on.
Offset gridLabelOffset(GridEdgeDto edge, Size text) => switch (edge) {
  GridEdgeDto.left => Offset(4, -text.height / 2),
  GridEdgeDto.right => Offset(-text.width - 4, -text.height / 2),
  GridEdgeDto.top => Offset(-text.width / 2, 4),
  GridEdgeDto.bottom => Offset(-text.width / 2, -text.height - 4),
  GridEdgeDto.inside => Offset(-text.width / 2, -text.height / 2),
};

/// A constellation illustration's mesh as textured triangles: two per grid cell, skipping any
/// triangle with a vertex the lens model could not place. Positions go through [at] (image →
/// screen); texture coordinates are in the illustration's pixels.
({List<Offset> positions, List<Offset> textureCoordinates}) artTriangles(
  ConstellationArtDto art,
  Size image,
  Offset Function(double x, double y) at,
) {
  final (cols, rows, p) = (art.cols, art.rows, art.points);
  final positions = <Offset>[];
  final tex = <Offset>[];
  Offset? vertex(int r, int c) {
    final i = (r * cols + c) * 2;
    return p[i].isNaN || p[i + 1].isNaN ? null : at(p[i], p[i + 1]);
  }

  Offset uv(int r, int c) =>
      Offset(c / (cols - 1) * image.width, r / (rows - 1) * image.height);
  void triangle(List<(int, int)> t) {
    final v = [for (final (r, c) in t) vertex(r, c)];
    if (v.contains(null)) return;
    for (var k = 0; k < 3; k++) {
      positions.add(v[k]!);
      tex.add(uv(t[k].$1, t[k].$2));
    }
  }

  for (var r = 0; r + 1 < rows; r++) {
    for (var c = 0; c + 1 < cols; c++) {
      triangle([(r, c), (r, c + 1), (r + 1, c)]);
      triangle([(r, c + 1), (r + 1, c + 1), (r + 1, c)]);
    }
  }
  return (positions: positions, textureCoordinates: tex);
}

/// Solve and annotation overlay, drawn in **screen** space. The engine returns image-pixel
/// geometry; [transform] (the viewer's image → screen matrix) moves the points, while strokes,
/// markers and text keep their screen size at any zoom. Sizes the sky gives (a nebula's
/// extent, the moon's disc) scale with the image.
///
/// The detection and match rings are diagnostics, off unless [diagnostics]: with them on, a
/// bright named star wears four rings (detected, matched, catalogued, named).
class SolveOverlayPainter extends CustomPainter {
  SolveOverlayPainter({
    required this.transform,
    required this.outcome,
    required this.annotations,
    this.diagnostics = false,
    this.art = const {},
  }) : super(repaint: transform);

  final TransformationController transform;
  final SolveOutcomeDto? outcome;
  final AnnotationsDto? annotations;
  final bool diagnostics;

  /// Decoded constellation illustrations by IAU abbreviation
  final Map<String, ui.Image> art;

  static Paint _stroke(Color c, double w) => Paint()
    ..style = PaintingStyle.stroke
    ..strokeWidth = w
    ..color = c;
  static final _detected = _stroke(const Color(0xFF40FF40), 1.2);
  static final _matched = _stroke(const Color(0xFFFF5050), 1.2);
  static final _catalog = Paint()..color = const Color(0xCCFFD23C);
  static final _named = _stroke(const Color(0xFF50C8FF), 1.5);
  static final _dso = _stroke(const Color(0xFFC878FF), 1.2);
  // Lines are opaque mid-tones rather than translucent: thin translucent strokes blend
  // unevenly on some renderers (up to fully opaque where a line runs along a pixel row)
  static final _outline = [
    const Color(0xFF7A5A99),
    const Color(0xFF9E68CC),
    const Color(0xFFC878FF),
  ].map((c) => _stroke(c, 1.2)).toList();
  static final _figure = _stroke(const Color(0xFF5E7FB8), 1.2);
  static final _boundary = _stroke(const Color(0xFF7C7C86), 1.0);
  static final _eqGrid = _stroke(const Color(0xFF3FA392), 1.0);
  static final _hzGrid = _stroke(const Color(0xFFB57A3E), 1.0);
  static final _horizon = _stroke(const Color(0xFFFF6E3C), 2.0);
  static final _solar = _stroke(const Color(0xFFFFA040), 1.5);
  static final _satellite = _stroke(const Color(0xFF78FFFF), 1.2);

  /// Rings up to this radius (screen pixels) mark a point; labels avoid covering them
  static const _markerMaxRadius = 16.0;

  @override
  void paint(Canvas canvas, Size size) {
    final m = transform.value;
    final scale = viewerScale(m);
    Offset at(double x, double y) =>
        MatrixUtils.transformPoint(m, Offset(x, y));
    void path(Path p, Paint paint) =>
        canvas.drawPath(p.transform(m.storage), paint);

    final a = annotations;
    if (a != null) {
      // Illustrations first, under every line: black backgrounds vanish in a screen blend
      for (final c in a.constellations) {
        final mesh = c.art;
        final image = art[c.abbr];
        if (mesh == null || image == null) continue;
        final t = artTriangles(
          mesh,
          Size(image.width.toDouble(), image.height.toDouble()),
          at,
        );
        if (t.positions.isEmpty) continue;
        canvas.saveLayer(
          Offset.zero & size,
          Paint()
            ..blendMode = BlendMode.screen
            ..color = const Color.fromRGBO(0, 0, 0, 0.55),
        );
        canvas.drawVertices(
          ui.Vertices(
            ui.VertexMode.triangles,
            t.positions,
            textureCoordinates: t.textureCoordinates,
          ),
          BlendMode.srcOver,
          Paint()
            ..filterQuality = FilterQuality.medium
            ..shader = ImageShader(
              image,
              TileMode.clamp,
              TileMode.clamp,
              Matrix4.identity().storage,
            ),
        );
        canvas.restore();
      }
      for (final g in a.grid) {
        final paint = g.kind == GridKindDto.horizon
            ? _horizon
            : g.system == GridSystemDto.equatorial
            ? _eqGrid
            : _hzGrid;
        for (final l in g.lines) {
          path(polylinePath(l), paint);
        }
      }
      for (final b in a.boundaries) {
        path(polylinePath(b.points), _boundary);
      }
      for (final c in a.constellations) {
        for (final l in c.lines) {
          path(polylinePath(l), _figure);
        }
      }
      for (final o in a.objects) {
        if (o.outlines.isNotEmpty) {
          for (final lv in o.outlines) {
            for (final c in lv.contours) {
              path(outlinePath(c), _outline[(lv.level.clamp(1, 3)) - 1]);
            }
          }
        } else {
          final shape = dsoRenderShape(
            semiMajorPx: o.semiMajorPx,
            semiMinorPx: o.semiMinorPx,
            angleDeg: o.angleDeg,
          );
          final r = (shape.radius * scale).clamp(8.0, 4000.0);
          final c = at(o.x, o.y);
          if (shape.isCircle) {
            canvas.drawCircle(c, r, _dso);
          } else {
            canvas.save();
            canvas.translate(c.dx, c.dy);
            // Image angles are counter-clockwise while screen y points down
            canvas.rotate(-shape.angleRad);
            canvas.drawOval(
              Rect.fromCenter(
                center: Offset.zero,
                width: 2 * r,
                height: 2 * (shape.semiMinor * scale).clamp(8.0, 4000.0),
              ),
              _dso,
            );
            canvas.restore();
          }
        }
      }
      // A catalogued star that is also named gets only the named marker
      final named = [for (final n in a.namedStars) at(n.x, n.y)];
      for (final s in a.stars) {
        final p = at(s.x, s.y);
        if (named.any((q) => (q - p).distance < 4)) continue;
        canvas.drawCircle(p, 2.5, _catalog);
      }
      for (final b in a.solar) {
        final r = ((b.angularRadiusPx ?? 0) * scale).clamp(7.0, 4000.0);
        canvas.drawCircle(at(b.x, b.y), r, _solar);
      }
      for (final s in a.satellites) {
        canvas.drawCircle(at(s.x, s.y), 5, _satellite);
      }
      for (final p in named) {
        canvas.drawCircle(p, 7, _named);
      }
    }
    final o = outcome;
    if (diagnostics && o != null) {
      for (final c in o.centroids) {
        canvas.drawCircle(at(c.x, c.y), 6, _detected);
      }
      for (final mt in o.solution?.matched ?? const <MatchDto>[]) {
        canvas.drawCircle(at(mt.x, mt.y), 9, _matched);
      }
    }
    // Text last, above every line, placed so no two labels overlap. Priority: the grid's
    // readings (fixed to the view's edges), the sun, moon and planets, constellation names,
    // named stars from the brightest, deep-sky objects from the brightest. A label with no
    // free spot beside its marker waits until zooming in makes room.
    if (a != null) {
      final labels = <(TextPainter, List<Offset>)>[];
      for (final g in a.grid) {
        final l = g.label;
        if (l == null) continue;
        final color = g.system == GridSystemDto.equatorial
            ? const Color(0xFF50E6C8)
            : const Color(0xFFFFAA50);
        final tp = _text(gridLabelText(g), color, 11);
        labels.add((tp, [at(l.x, l.y) + gridLabelOffset(l.edge, tp.size)]));
      }
      for (final b in a.solar) {
        final tp = _text(b.name, const Color(0xFFFFA040), 12);
        final r = ((b.angularRadiusPx ?? 0) * scale).clamp(7.0, 4000.0);
        labels.add((tp, besideMarker(at(b.x, b.y), r + 2, tp.size)));
      }
      for (final c in a.constellations) {
        if (c.labelX == null || c.labelY == null) continue;
        final tp = _text(c.name, const Color(0xC080B4FF), 13);
        final p = at(c.labelX!, c.labelY!) - tp.size.center(Offset.zero);
        labels.add((
          tp,
          [p, p.translate(0, -tp.height), p.translate(0, tp.height)],
        ));
      }
      final stars = [...a.namedStars]..sort((x, y) => x.mag.compareTo(y.mag));
      for (final n in stars) {
        final tp = _text(n.name, const Color(0xFF50C8FF), 12);
        labels.add((tp, besideMarker(at(n.x, n.y), 9, tp.size)));
      }
      final wcs = outcome?.solution?.wcs;
      final image = wcs == null
          ? Size.infinite
          : Size(wcs.width.toDouble(), wcs.height.toDouble());
      for (final o in [...a.objects]..sort(dsoLabelOrder)) {
        final tp = _text(
          o.commonName ?? o.designation,
          const Color(0xFFC878FF),
          11,
        );
        // An outlined object's centre may be off the image: label its first visible vertex
        final anchor = o.outlines.isEmpty
            ? Offset(o.x, o.y)
            : outlineLabelAnchor(o.x, o.y, o.outlines, image);
        labels.add((tp, besideMarker(at(anchor.dx, anchor.dy), 10, tp.size)));
      }
      // Rings of point-like objects: labels step around them when they can (a large
      // object's ring is an area, not a marker)
      final markers = [
        for (final n in a.namedStars)
          Rect.fromCircle(center: at(n.x, n.y), radius: 7),
        for (final b in a.solar)
          if ((b.angularRadiusPx ?? 0) * scale <= _markerMaxRadius)
            Rect.fromCircle(center: at(b.x, b.y), radius: 7),
        for (final o in a.objects)
          if (o.outlines.isEmpty && o.semiMajorPx * scale <= _markerMaxRadius)
            Rect.fromCircle(center: at(o.x, o.y), radius: 8),
      ];
      final spots = placeLabels(size, markers: markers, [
        for (final (tp, candidates) in labels) LabelSlot(tp.size, candidates),
      ]);
      for (var i = 0; i < labels.length; i++) {
        final spot = spots[i];
        if (spot != null) labels[i].$1.paint(canvas, spot);
      }
    }
  }

  TextPainter _text(String text, Color color, double size) => TextPainter(
    text: TextSpan(
      text: text,
      style: TextStyle(
        color: color,
        fontSize: size,
        shadows: const [Shadow(blurRadius: 3, color: Colors.black)],
      ),
    ),
    textDirection: TextDirection.ltr,
  )..layout();

  @override
  bool shouldRepaint(SolveOverlayPainter old) =>
      old.outcome != outcome ||
      old.annotations != annotations ||
      old.diagnostics != diagnostics ||
      old.art != art ||
      old.transform != transform;
}
