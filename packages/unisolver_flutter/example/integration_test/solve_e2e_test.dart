import 'dart:io';
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:path_provider/path_provider.dart';
import 'package:unisolver_flutter/asset_installer.dart';
import 'package:unisolver_flutter/src/rust/api/logging.dart' as rust_log;
import 'package:unisolver_flutter/src/rust/api/solver.dart';
import 'package:unisolver_flutter/src/rust/api/preview.dart';
import 'package:unisolver_flutter/src/rust/api/types.dart';
import 'package:unisolver_flutter/src/rust/api/wcs.dart';
import 'package:unisolver_flutter/src/rust/frb_generated.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  setUpAll(() async {
    await RustLib.init();
  });

  testWidgets('assets install, the sample solves to Scorpius, zh names, logs', (
    t,
  ) async {
    final logs = <String>[];
    rust_log.setLogStream().listen((e) => logs.add('${e.level} ${e.message}'));

    final paths = await UnisolverAssets.ensureInstalled();
    expect(await File(paths.dbPath).length(), greaterThan(30 * 1000 * 1000));

    final solver = await UniSolver.newInstance(dbPath: paths.dbPath);
    final props = solver.properties();
    expect(props.numStars, greaterThan(BigInt.from(50000)));

    // Save the sample to disk and solve the file (unknown FOV → ladder)
    final bytes = await rootBundle.load('assets/sample_scorpius.jpg');
    final dir = await getApplicationSupportDirectory();
    final img = File('${dir.path}/sample.jpg');
    await img.writeAsBytes(bytes.buffer.asUint8List(), flush: true);

    final res = await solver.solveImageFileWithPresets(
      path: img.path,
      base: SolveOptionsDto.defaults(fovEstimateDeg: 70),
      presets: [
        FovPresetDto(fovDeg: 70, maxErrorDeg: 9),
        FovPresetDto(fovDeg: 46, maxErrorDeg: 7),
      ],
    );
    expect(res.outcome.status, SolveStatusDto.ok);
    final g = res.outcome.solution!;
    expect(g.raDeg, closeTo(250.07, 0.5));
    expect(g.decDeg, closeTo(-19.19, 0.5));

    // Annotation (Chinese names): the example opts in to the names pack in its pubspec
    final namesPath = await UnisolverAssets.installNames();
    expect(
      namesPath,
      isNotNull,
      reason: 'the example declares the optional asset',
    );
    final ann = await solver.annotator(
      dsoPath: paths.dsoPath,
      namesPath: namesPath,
      constellationsPath: await UnisolverAssets.installConstellations(),
    );
    expect(ann.languages().length, greaterThanOrEqualTo(10));
    final base = AnnotateOptionsDto.defaults();
    final a = await ann.annotate(
      wcs: g.wcs,
      opts: AnnotateOptionsDto(
        starMaxMag: base.starMaxMag,
        maxStars: base.maxStars,
        includeStarNames: true,
        includeDso: true,
        dsoMaxMag: base.dsoMaxMag,
        dsoOutlines: true,
        maxOutlineLevel: 3,
        language: 'zh_cn',
        includeSolarSystem: false,
        includeConstellations: true,
        constellationBoundaries: true,
      ),
    );
    expect(a.layers.dso, isTrue);
    // The sample is Scorpius: its figure, its Chinese name and the boundaries around it
    expect(a.layers.constellations, isTrue);
    final sco = a.constellations.where((c) => c.abbr == 'Sco').toList();
    expect(
      sco,
      hasLength(1),
      reason: a.constellations.map((c) => c.abbr).join(' '),
    );
    expect(sco.single.name, '天蝎座');
    expect(sco.single.lines, isNotEmpty);
    expect(a.boundaries.any((b) => b.between.contains('Sco')), isTrue);

    // A grid for a zoomed view: finer lines, labels on the view's edges; and the batch
    // transform agrees with the solution's boresight
    final w = g.wcs;
    final grid = await ann.annotate(
      wcs: w,
      opts: AnnotateOptionsDto(
        starMaxMag: base.starMaxMag,
        maxStars: base.maxStars,
        includeStarNames: false,
        includeDso: false,
        dsoOutlines: false,
        maxOutlineLevel: 3,
        language: 'en',
        includeSolarSystem: false,
        equatorialGrid: true,
        viewport: ViewportDto(
          x: w.width / 2 - 200,
          y: w.height / 2 - 100,
          width: 400,
          height: 200,
          scale: 4,
        ),
      ),
    );
    expect(grid.layers.grid, isTrue);
    expect(grid.grid.where((l) => l.kind == GridKindDto.dec), isNotEmpty);
    expect(grid.grid.every((l) => l.label?.edge != null), isTrue);
    // A display preview (the path FITS takes; any format works): box-averaged, stretched
    final pv = await imagePreview(path: img.path, maxSide: 800);
    // Integer box factor, never above the limit: 1920 / 800 rounds up to 3
    expect((pv.width, pv.height), (640, 360));
    expect((pv.sourceWidth, pv.sourceHeight), (w.width, w.height));
    expect(pv.rgba.length, 640 * 360 * 4);
    final px = wcsSkyToPixels(wcs: w, radec: [g.raDeg, g.decDeg]);
    expect(px[0], closeTo((w.width - 1) / 2, 0.5));
    expect(px[1], closeTo((w.height - 1) / 2, 0.5));
    // In the authoritative name data, Chinese star names carry the Latin name ("心宿二 Antares")
    expect(
      a.namedStars.any((n) => n.name.contains('心宿二')),
      isTrue,
      reason: a.namedStars.map((n) => n.name).join(' '),
    );

    // The log bridge received at least the attach event
    expect(logs, isNotEmpty);
  });

  // The HEIC path: the app decodes the photo itself and passes its EXIF focal length and
  // time; the pool tries that FOV first and echoes the time for the annotator
  testWidgets('decoded photo with EXIF values solves on the first rung', (
    t,
  ) async {
    final paths = await UnisolverAssets.ensureInstalled();
    final pool = await UniSolverPool.openDir(
      dir: File(paths.dbPath).parent.path,
    );
    final bytes = await rootBundle.load('assets/sample_scorpius.jpg');
    final codec = await ui.instantiateImageCodec(bytes.buffer.asUint8List());
    final image = (await codec.getNextFrame()).image;
    final rgba = await image.toByteData(format: ui.ImageByteFormat.rawRgba);
    final (w, h) = (image.width, image.height);
    // The sample spans 73.3° across its width; its 35 mm equivalent (CIPA diagonal) is 24 mm
    final diag = math.sqrt(w * w + h * h);
    final focal = (21.633 / (math.tan(73.3 / 2 * math.pi / 180) * diag / w))
        .roundToDouble();
    const takenAt = 1781018520519;
    final d = SolveOptionsDto.defaults(fovEstimateDeg: 70);
    final res = await pool.solveFrameAuto(
      frame: FrameDto(
        width: w,
        height: h,
        rowStrideBytes: null,
        kind: PixelKindDto.rgba8,
        bytes: rgba!.buffer.asUint8List(),
      ),
      opts: SolveOptionsDto(
        fovEstimateDeg: d.fovEstimateDeg,
        hintUncertaintyDeg: d.hintUncertaintyDeg,
        strictHint: d.strictHint,
        profile: d.profile,
        retryAlternateProfile: d.retryAlternateProfile,
        thorough: d.thorough,
        matchThreshold: d.matchThreshold,
        timeoutMs: d.timeoutMs,
        observationUnixMs: takenAt,
        focalLength35Mm: focal,
      ),
    );
    expect(res.outcome.status, SolveStatusDto.ok);
    expect(
      res.attempts.length,
      1,
      reason: res.attempts.map((a) => a.fovDeg).join(' '),
    );
    expect(res.outcome.solution!.raDeg, closeTo(250.07, 0.5));
    expect(res.outcome.observationUnixMs, takenAt);
    expect(res.outcome.observer, isNull);

    // A HEIC file is refused with directions to the platform decoder
    final dir = await getApplicationSupportDirectory();
    final heic = File('${dir.path}/photo.heic');
    await heic.writeAsBytes([
      0,
      0,
      0,
      24,
      ...'ftypheic'.codeUnits,
      ...List.filled(16, 0),
    ]);
    await expectLater(
      pool.solveImageFileAuto(path: heic.path, base: d),
      throwsA(
        predicate((e) => '$e'.contains('HEIC') && '$e'.contains('frame entry')),
      ),
    );
  });

  testWidgets('rust panic surfaces as Dart exception, not crash', (t) async {
    await expectLater(debugTriggerPanic(), throwsA(anything));
  });
}
