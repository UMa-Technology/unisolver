import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:path_provider/path_provider.dart';
import 'package:unisolver_flutter/asset_installer.dart';
import 'package:unisolver_flutter/src/rust/api/logging.dart' as rust_log;
import 'package:unisolver_flutter/src/rust/api/solver.dart';
import 'package:unisolver_flutter/src/rust/api/types.dart';
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
      ),
    );
    expect(a.layers.dso, isTrue);
    // In the authoritative name data, Chinese star names carry the Latin name ("心宿二 Antares")
    expect(
      a.namedStars.any((n) => n.name.contains('心宿二')),
      isTrue,
      reason: a.namedStars.map((n) => n.name).join(' '),
    );

    // The log bridge received at least the attach event
    expect(logs, isNotEmpty);
  });

  testWidgets('rust panic surfaces as Dart exception, not crash', (t) async {
    await expectLater(debugTriggerPanic(), throwsA(anything));
  });
}
