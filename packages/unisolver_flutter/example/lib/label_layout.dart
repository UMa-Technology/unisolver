import 'dart:ui';

import 'package:unisolver_flutter/unisolver_flutter.dart';

/// A label waiting for a place on screen: its size and where its top-left corner may go, in
/// order of preference.
class LabelSlot {
  const LabelSlot(this.size, this.candidates);

  final Size size;
  final List<Offset> candidates;
}

/// Greedy label placement in screen space, in the caller's priority order: each label takes
/// the first candidate that keeps [gap] pixels clear of every label placed before it,
/// preferring candidates wholly inside [view] to ones that stick out, and candidates that
/// cover none of the [markers] (other objects' rings) to ones that do. A label with no free
/// candidate, or entirely off screen, gets null and takes no room.
///
/// Placement needs the text's size on screen, which only the app knows (font, language,
/// zoom), so it happens here rather than in the engine. Run it on every repaint: as the view
/// zooms in, crowded labels spread apart and dropped ones come back.
List<Offset?> placeLabels(
  Size view,
  List<LabelSlot> labels, {
  List<Rect> markers = const [],
  double gap = 2,
}) {
  final screen = Offset.zero & view;
  bool inside(Rect r) =>
      r.left >= screen.left &&
      r.top >= screen.top &&
      r.right <= screen.right &&
      r.bottom <= screen.bottom;
  bool clear(Rect r) => !markers.any((m) => m.overlaps(r));
  final passes = <bool Function(Rect)>[
    (r) => inside(r) && clear(r),
    inside,
    (r) => screen.overlaps(r) && clear(r),
    screen.overlaps,
  ];
  final placed = <Rect>[];
  final out = <Offset?>[];
  for (final l in labels) {
    bool free(Rect r) => !placed.any((p) => p.inflate(gap).overlaps(r));
    Offset? pick;
    for (final pass in passes) {
      for (final c in l.candidates) {
        final r = c & l.size;
        if (pass(r) && free(r)) {
          pick = c;
          break;
        }
      }
      if (pick != null) break;
    }
    if (pick != null) placed.add(pick & l.size);
    out.add(pick);
  }
  return out;
}

/// Candidate top-left corners for a label beside a marker of radius [r] at [p]: below,
/// above, right, left.
List<Offset> besideMarker(Offset p, double r, Size text) => [
  p + Offset(-text.width / 2, r),
  p + Offset(-text.width / 2, -r - text.height),
  p + Offset(r, -text.height / 2),
  p + Offset(-r - text.width, -text.height / 2),
];

/// Deep-sky label priority: brighter first; objects without a magnitude (most nebulae) after
/// the rated ones, larger first.
int dsoLabelOrder(DsoAnnotationDto a, DsoAnnotationDto b) {
  final byMag = (a.mag ?? double.infinity).compareTo(b.mag ?? double.infinity);
  return byMag != 0 ? byMag : b.semiMajorPx.compareTo(a.semiMajorPx);
}
