import 'dart:ui';

import 'package:flutter_test/flutter_test.dart';
import 'package:unisolver_flutter/unisolver_flutter.dart';
import 'package:unisolver_flutter_example/label_layout.dart';

void main() {
  const view = Size(400, 300);
  const text = Size(40, 12);

  test('a label that would overlap moves to its next spot', () {
    final a = besideMarker(const Offset(100, 100), 9, text);
    final spots = placeLabels(view, [LabelSlot(text, a), LabelSlot(text, a)]);
    expect(spots, [a[0], a[1]], reason: 'the second goes above');
  });

  test('a label with every spot taken is left out and takes no room', () {
    final a = besideMarker(const Offset(100, 100), 9, text);
    final spots = placeLabels(view, [
      for (var i = 0; i < 5; i++) LabelSlot(text, a),
      LabelSlot(text, [const Offset(300, 200)]),
    ]);
    expect(spots.take(4), a);
    expect(spots[4], isNull);
    expect(spots[5], const Offset(300, 200));
  });

  test('labels keep a gap', () {
    final spots = placeLabels(view, [
      LabelSlot(text, [const Offset(10, 10)]),
      LabelSlot(text, [const Offset(51, 10), const Offset(53, 10)]),
    ]);
    expect(spots[1], const Offset(53, 10));
  });

  test('a spot wholly on screen wins over one that sticks out', () {
    // A star at the bottom edge: its label goes above rather than half off screen
    final a = besideMarker(const Offset(200, 295), 9, text);
    expect(placeLabels(view, [LabelSlot(text, a)]).single, a[1]);
    // Near a corner with nothing wholly visible, a partly visible spot still counts
    final corner = [const Offset(390, 295)];
    expect(placeLabels(view, [LabelSlot(text, corner)]).single, corner.single);
  });

  test('a label steps around other markers when it can', () {
    final a = besideMarker(const Offset(100, 100), 9, text);
    final ring = Rect.fromCircle(center: const Offset(100, 118), radius: 8);
    expect(
      placeLabels(view, markers: [ring], [LabelSlot(text, a)]).single,
      a[1],
      reason: 'a ring just below: the label goes above',
    );
    final everywhere = [Offset.zero & view];
    expect(
      placeLabels(view, markers: everywhere, [LabelSlot(text, a)]).single,
      a[0],
      reason: 'covered markers are a fallback, not a reason to drop the label',
    );
  });

  test('labels off screen are dropped and block nothing', () {
    final off = [const Offset(-100, -100)];
    final on = [const Offset(10, 10)];
    expect(placeLabels(view, [LabelSlot(text, off), LabelSlot(text, on)]), [
      null,
      on.single,
    ]);
  });

  test('deep-sky labels go brightest first, unrated ones by size', () {
    DsoAnnotationDto o(String id, double? mag, double size) => DsoAnnotationDto(
      x: 0,
      y: 0,
      semiMajorPx: size,
      semiMinorPx: size,
      designation: id,
      kind: DsoKindDto.nebula,
      mag: mag,
      outlines: const [],
    );
    final sorted = [
      o('faint', 9, 50),
      o('small', null, 5),
      o('bright', 4, 1),
      o('large', null, 80),
    ]..sort(dsoLabelOrder);
    expect(sorted.map((d) => d.designation), [
      'bright',
      'faint',
      'large',
      'small',
    ]);
  });
}
