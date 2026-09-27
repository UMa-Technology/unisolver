import 'package:flutter/material.dart';
import 'package:unisolver_flutter/unisolver_flutter.dart';

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

/// Solve and annotation overlay. Coordinates are source-image pixels (top-left origin);
/// the outer FittedBox scales, so the painter does no coordinate conversion.
class SolveOverlayPainter extends CustomPainter {
  SolveOverlayPainter({required this.outcome, required this.annotations});

  final SolveOutcomeDto? outcome;
  final AnnotationsDto? annotations;

  static final _detected = Paint()
    ..style = PaintingStyle.stroke
    ..strokeWidth = 1.5
    ..color = const Color(0xFF40FF40);
  static final _matched = Paint()
    ..style = PaintingStyle.stroke
    ..strokeWidth = 1.5
    ..color = const Color(0xFFFF5050);
  static final _catalog = Paint()
    ..style = PaintingStyle.stroke
    ..strokeWidth = 1.0
    ..color = const Color(0xFFFFD23C);
  static final _named = Paint()
    ..style = PaintingStyle.stroke
    ..strokeWidth = 2.0
    ..color = const Color(0xFF50C8FF);
  static final _dso = Paint()
    ..style = PaintingStyle.stroke
    ..strokeWidth = 1.5
    ..color = const Color(0xFFC878FF);
  static final _solar = Paint()
    ..style = PaintingStyle.stroke
    ..strokeWidth = 2.0
    ..color = const Color(0xFFFFA040);
  // Outline levels 1..3: the faint outer edge is drawn faintest
  static final _outline = [0x70, 0xA8, 0xE0]
      .map(
        (a) => Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = 1.5
          ..color = Color.fromARGB(a, 0xC8, 0x78, 0xFF),
      )
      .toList();

  @override
  void paint(Canvas canvas, Size size) {
    final o = outcome;
    if (o != null) {
      for (final c in o.centroids) {
        canvas.drawCircle(Offset(c.x, c.y), 9, _detected);
      }
      final g = o.solution;
      if (g != null) {
        for (final m in g.matched) {
          canvas.drawCircle(Offset(m.x, m.y), 12, _matched);
        }
      }
    }
    final a = annotations;
    if (a != null) {
      for (final s in a.stars) {
        canvas.drawCircle(Offset(s.x, s.y), 5, _catalog);
      }
      for (final o in a.objects) {
        if (o.outlines.isNotEmpty) {
          for (final lv in o.outlines) {
            final paint = _outline[(lv.level.clamp(1, 3)) - 1];
            for (final c in lv.contours) {
              canvas.drawPath(outlinePath(c), paint);
            }
          }
          final at = outlineLabelAnchor(o.x, o.y, o.outlines, size);
          _label(
            canvas,
            o.commonName ?? o.designation,
            at.dx,
            at.dy + 4,
            const Color(0xFFC878FF),
          );
          continue;
        }
        final shape = dsoRenderShape(
          semiMajorPx: o.semiMajorPx,
          semiMinorPx: o.semiMinorPx,
          angleDeg: o.angleDeg,
        );
        final r = shape.radius.clamp(14.0, 4000.0);
        if (shape.isCircle) {
          canvas.drawCircle(Offset(o.x, o.y), r, _dso);
        } else {
          canvas.save();
          canvas.translate(o.x, o.y);
          // Image angles are counter-clockwise while screen y points down: rotate the canvas by the negative
          canvas.rotate(-shape.angleRad);
          canvas.drawOval(
            Rect.fromCenter(
              center: Offset.zero,
              width: 2 * r,
              height: 2 * shape.semiMinor.clamp(14.0, 4000.0),
            ),
            _dso,
          );
          canvas.restore();
        }
        _label(
          canvas,
          o.commonName ?? o.designation,
          o.x,
          o.y + r + 2,
          const Color(0xFFC878FF),
        );
      }
      for (final n in a.namedStars) {
        canvas.drawCircle(Offset(n.x, n.y), 16, _named);
        _label(canvas, n.name, n.x, n.y + 18, const Color(0xFF50C8FF));
      }
      // Planets, the moon and the sun (present only when the solve reported a time)
      for (final b in a.solar) {
        final r = (b.angularRadiusPx ?? 0).clamp(14.0, 4000.0);
        canvas.drawCircle(Offset(b.x, b.y), r, _solar);
        _label(canvas, b.name, b.x, b.y + r + 2, const Color(0xFFFFA040));
      }
    }
  }

  void _label(Canvas canvas, String text, double x, double y, Color color) {
    final tp = TextPainter(
      text: TextSpan(
        text: text,
        style: TextStyle(
          color: color,
          fontSize: 14,
          shadows: const [Shadow(blurRadius: 3, color: Colors.black)],
        ),
      ),
      textDirection: TextDirection.ltr,
    )..layout();
    tp.paint(canvas, Offset(x - tp.width / 2, y));
  }

  @override
  bool shouldRepaint(SolveOverlayPainter old) =>
      old.outcome != outcome || old.annotations != annotations;
}
