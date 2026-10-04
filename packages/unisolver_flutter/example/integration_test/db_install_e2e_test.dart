/// End-to-end acceptance of tier acquisition: real HTTP (with `Range`) + real sha256 (Rust,
/// streaming) + real zstd decompression + real registration + a real solve.
///
/// The server runs inside the app process and serves the **real archive of the bundled wide tier**
/// (15.9 MB → 60 MB database), so the only difference from a real host is the domain: manifest,
/// content-addressed paths, resume and refusal all follow the host contract.
library;

import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:path_provider/path_provider.dart';
import 'package:unisolver_flutter/unisolver_flutter.dart';

/// In-process static server: supports Range and can drop the connection midway.
class LocalCdn {
  LocalCdn(this.files);
  final Map<String, List<int>> files;
  HttpServer? _s;
  int? truncateAt;
  final List<String?> ranges = [];

  String get baseUrl => 'http://127.0.0.1:${_s!.port}/';

  Future<void> start() async {
    _s = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    _s!.listen((req) async {
      final body = files[req.uri.path.substring(1)];
      ranges.add(req.headers.value(HttpHeaders.rangeHeader));
      if (body == null) {
        req.response.statusCode = HttpStatus.notFound;
        await req.response.close();
        return;
      }
      var start = 0;
      final r = req.headers.value(HttpHeaders.rangeHeader);
      if (r != null && r.startsWith('bytes=')) {
        start = int.parse(r.substring(6).split('-').first);
        req.response.statusCode = HttpStatus.partialContent;
        req.response.headers.set(
          HttpHeaders.contentRangeHeader,
          'bytes $start-${body.length - 1}/${body.length}',
        );
      }
      final slice = body.sublist(start);
      final cut = truncateAt;
      truncateAt = null;
      const chunk = 1 << 18;
      final end = cut == null ? slice.length : cut.clamp(0, slice.length);
      for (var i = 0; i < end; i += chunk) {
        req.response.add(slice.sublist(i, (i + chunk).clamp(0, end)));
        await req.response.flush();
      }
      await req.response.close().catchError((_) {});
    });
  }

  Future<void> stop() async => _s?.close(force: true);
}

/// Optional: repeat against an **external** static server with a **real manifest** and **real tier files**.
///     python3 scripts/ci/serve_tiers.py --manifest M --data DIR --only TIER
///     flutter test integration_test/db_install_e2e_test.dart -d macos \
///       --dart-define=UNISOLVER_CDN_BASE=http://127.0.0.1:8099/ \
///       --dart-define=UNISOLVER_CDN_TIER=TIER   # default: the manifest's first downloadable tier
const externalBase = String.fromEnvironment('UNISOLVER_CDN_BASE');
const externalTier = String.fromEnvironment('UNISOLVER_CDN_TIER');

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  setUpAll(() async => RustLib.init());

  testWidgets(
    'manifest → download (resume and refusal) → decompress → register → solve, end to end',
    (t) async {
      final support = await getApplicationSupportDirectory();
      final dir = Directory('${support.path}/dbmgr_e2e');
      if (dir.existsSync()) dir.deleteSync(recursive: true);
      dir.createSync(recursive: true);

      // The real archive (the bundled wide tier) plays "a tier on the host"
      final asset = await rootBundle.load(
        'packages/unisolver_flutter/assets/unisolver_10_80.db.zst',
      );
      final zstBytes = asset.buffer.asUint8List();
      final staged = File('${dir.path}/staged.zst')
        ..writeAsBytesSync(zstBytes, flush: true);
      final sha = await sha256File(path: staged.path);
      staged.deleteSync();

      // The names pack plays "an optional asset on the host"
      final namesBytes = (await rootBundle.load(
        'packages/unisolver_flutter/optional/unisolver_names.bin',
      )).buffer.asUint8List();
      final namesStaged = File('${dir.path}/staged_names.bin')
        ..writeAsBytesSync(namesBytes, flush: true);
      final namesSha = await sha256File(path: namesStaged.path);
      namesStaged.deleteSync();
      const namesKey = 'assets/0badf00d/unisolver_names.bin';

      const name = 'unisolver_10_80_e2e';
      const key = 'db/abcd1234/$name.db.zst';
      Map<String, Object?> tier({String? shaOverride}) => {
        'name': name,
        'file': '$name.db.zst',
        'key': key,
        'min_fov_deg': 10.0,
        'max_fov_deg': 80.0,
        'bytes': zstBytes.length,
        'sha256': shaOverride ?? sha,
        'mobile': true,
      };
      final cdn = LocalCdn({
        'manifest-v3.json': utf8.encode(
          json.encode({
            'version': 3,
            'base_url': '',
            'tiers': [tier()],
            'assets': [
              {
                'name': 'unisolver_names',
                'kind': 'names',
                'file': 'unisolver_names.bin',
                'key': namesKey,
                'bytes': namesBytes.length,
                'sha256': namesSha,
                'license': 'GPL-2.0-or-later',
              },
            ],
          }),
        ),
        'bad/manifest-v3.json': utf8.encode(
          json.encode({
            'version': 3,
            'base_url': '',
            'tiers': [tier(shaOverride: '0' * 64)],
          }),
        ),
        key: zstBytes,
        namesKey: namesBytes,
        // The tampered manifest gets its own prefix (base URL + key decide the download URL)
        'bad/$key': zstBytes,
      });
      await cdn.start();
      addTearDown(cdn.stop);

      final pool = await UniSolverPool.empty();
      final mgr = DbManager(
        dir: dir.path,
        baseUrl: cdn.baseUrl,
        register: (p) => pool.register(dbPath: p).then((_) {}),
      );

      // (1) Manifest
      final m = await mgr.fetchManifest();
      expect(m.tiers.single.name, name);
      final tierInfo = m.tiers.single;

      // (2) The tampered manifest must be refused (sha256 mismatch) and leave nothing behind
      final badMgr = DbManager(dir: dir.path, baseUrl: '${cdn.baseUrl}bad/');
      final badTier = (await badMgr.fetchManifest()).tiers.single;
      await expectLater(
        badMgr.install(badTier),
        throwsA(isA<DbChecksumException>()),
      );
      expect(mgr.partialBytes(tierInfo), 0);
      expect(mgr.isInstalled(tierInfo), isFalse);

      // (3) Drop → resume (the second request must send Range and fetch only the gap)
      cdn.truncateAt = 4 << 20; // drop after 4 MB
      await expectLater(mgr.install(tierInfo), throwsA(isA<DbException>()));
      final partial = mgr.partialBytes(tierInfo);
      expect(partial, 4 << 20);
      cdn.ranges.clear();

      final phases = <DbPhase>[];
      final dbPath = await mgr.install(
        tierInfo,
        onProgress: (p) => phases.add(p.phase),
      );
      expect(
        cdn.ranges.first,
        'bytes=$partial-',
        reason: 'must resume, not restart',
      );
      expect(phases, contains(DbPhase.registering));

      // (4) The result is a real database (format 2 header, plausible size) and is registered
      final f = File(dbPath);
      expect(f.lengthSync(), greaterThan(20 * 1000 * 1000));
      expect(await f.open().then((r) => r.read(6)), [
        ...'T3DB'.codeUnits,
        2,
        0,
      ]);
      final tiers = await pool.tiers();
      expect(tiers.map((e) => e.name), contains(name));

      // (5) Solvable right after install: the sample solves without naming a tier (Scorpius)
      final bytes = await rootBundle.load('assets/sample_scorpius.jpg');
      final img = File('${dir.path}/sample.jpg')
        ..writeAsBytesSync(bytes.buffer.asUint8List(), flush: true);
      final res = await pool.solveImageFileAuto(
        path: img.path,
        base: SolveOptionsDto.defaults(fovEstimateDeg: 70),
      );
      expect(
        res.outcome.status,
        SolveStatusDto.ok,
        reason: res.attempts.map((a) => '${a.db}@${a.fovDeg}').join(' '),
      );
      expect(res.db, name);
      final g = res.outcome.solution!;
      expect(g.raDeg, closeTo(250.07, 1.0));
      expect(g.decDeg, closeTo(-19.19, 1.0));

      // (5b) The optional names pack installs from the host too, and annotates
      final namesAsset = m.assetByName('unisolver_names')!;
      final namesPath = await mgr.installAsset(namesAsset);
      expect(File(namesPath).lengthSync(), namesBytes.length);
      final probe = await pool.annotator(namesPath: namesPath);
      expect(probe.languages(), contains('zh_cn'));
      probe.dispose();

      // (6) Idempotent: installing again returns at once, no download
      cdn.ranges.clear();
      expect(await mgr.install(tierInfo), dbPath);
      expect(cdn.ranges, isEmpty);

      dir.deleteSync(recursive: true);
    },
    timeout: const Timeout(Duration(minutes: 5)),
  );

  testWidgets(
    'external static server + real manifest + real tier (only with --dart-define)',
    (t) async {
      final support = await getApplicationSupportDirectory();
      final dir = Directory('${support.path}/dbmgr_cdn');
      if (dir.existsSync()) dir.deleteSync(recursive: true);
      dir.createSync(recursive: true);
      addTearDown(() => dir.deleteSync(recursive: true));

      final pool = await UniSolverPool.empty();
      final mgr = DbManager(
        dir: dir.path,
        baseUrl: externalBase,
        register: (p) => pool.register(dbPath: p).then((_) {}),
      );
      final m = await mgr.fetchManifest();
      final tier = externalTier.isEmpty
          ? m.tiers.firstWhere((t) => t.key != null)
          : m.byName(externalTier)!;
      expect(
        tier.key,
        isNotNull,
        reason: 'a downloadable tier must have a key',
      );

      // Retry after an interruption (what real clients do). With the server's --truncate-first
      // this resumes once; otherwise it succeeds at the first attempt.
      var lastPhase = DbPhase.downloading;
      String? dbPath;
      var tries = 0;
      while (dbPath == null) {
        tries++;
        try {
          dbPath = await mgr.install(
            tier,
            allowNonMobile: true,
            onProgress: (p) => lastPhase = p.phase,
          );
        } on DbException catch (_) {
          if (tries >= 3) rethrow;
          expect(
            mgr.partialBytes(tier),
            greaterThan(0),
            reason: 'an interruption must leave progress to resume from',
          );
        }
      }
      expect(lastPhase, DbPhase.done);
      debugPrint('external host: installed ${tier.name} in $tries attempt(s)');

      // The decompressed size must equal the manifest's raw_bytes (manifest and archive agree)
      final f = File(dbPath);
      if (tier.rawBytes != null) expect(f.lengthSync(), tier.rawBytes);
      expect(await f.open().then((r) => r.read(6)), [
        ...'T3DB'.codeUnits,
        2,
        0,
      ]);

      // The engine accepts it, and its range matches the manifest
      final tiers = await pool.tiers();
      final reg = tiers.firstWhere((e) => e.name == tier.name);
      expect(reg.minFovDeg, closeTo(tier.minFovDeg, 0.01));
      expect(reg.maxFovDeg, closeTo(tier.maxFovDeg, 0.01));
      if (tier.numStars != null) expect(reg.numStars.toInt(), tier.numStars);

      // Idempotent
      expect(await mgr.install(tier, allowNonMobile: true), dbPath);
    },
    skip: externalBase.isEmpty,
    timeout: const Timeout(Duration(minutes: 10)),
  );

  // The fixtures come from `cargo run -p unisolver-starmatch --example mini_package` (a synthetic sky)
  testWidgets(
    'narrow package: manifest → two downloads → both digests → registered, and found by openDir',
    (t) async {
      Future<List<int>> load(String f) async => (await rootBundle.load(
        'integration_test/fixtures/$f',
      )).buffer.asUint8List();
      final spec =
          json.decode(utf8.decode(await load('narrow_mini.json')))
              as Map<String, dynamic>;
      final files = (spec['files'] as List).cast<Map<String, dynamic>>();
      String keyOf(Map<String, dynamic> f) =>
          'pkg/${(f['sha256'] as String).substring(56)}/${f['file']}';
      Map<String, dynamic> manifest({String? starsRawSha}) => {
        'version': 3,
        'tiers': [],
        'packages': [
          {
            'name': 'narrow_mini',
            'kind': 'blind-index',
            'min_fov_deg': 0.2,
            'max_fov_deg': 3.1,
            'mobile': false,
            'min_engine': engineVersion(),
            'files': [
              for (final f in files)
                {
                  ...f,
                  'key': keyOf(f),
                  if (f['role'] == 'stars' && starsRawSha != null)
                    'raw_sha256': starsRawSha,
                },
            ],
          },
        ],
      };
      final cdn = LocalCdn({
        'manifest-v3.json': utf8.encode(json.encode(manifest())),
        'bad/manifest-v3.json': utf8.encode(
          json.encode(manifest(starsRawSha: '0' * 64)),
        ),
        for (final f in files) keyOf(f): await load(f['file'] as String),
        for (final f in files)
          'bad/${keyOf(f)}': await load(f['file'] as String),
      });
      await cdn.start();
      final dir = Directory.systemTemp.createTempSync('narrow_e2e').path;
      try {
        // A wrong decompressed digest: refused, nothing left behind
        final bad = DbManager(dir: dir, baseUrl: '${cdn.baseUrl}bad/');
        final pb = (await bad.fetchManifest()).packageByName('narrow_mini')!;
        await expectLater(
          bad.installPackage(pb),
          throwsA(isA<DbChecksumException>()),
        );
        expect(File('$dir/narrow_mini.stars').existsSync(), isFalse);

        final pool = await UniSolverPool.empty();
        final m = DbManager(
          dir: dir,
          baseUrl: cdn.baseUrl,
          registerPackage: (i, s) async {
            await pool.registerNarrow(indexPath: i, starsPath: s);
          },
        );
        final p = (await m.fetchManifest()).packageByName('narrow_mini')!;
        await m.installPackage(p);
        expect(m.isPackageInstalled(p), isTrue);
        final narrow = (await pool.tiers())
            .where((x) => x.kind == TierKindDto.narrow)
            .toList();
        expect(narrow.single.name, 'narrow_mini');
        // A directory open finds it by its headers (the leftover manifest cache is not a database)
        final opened = await UniSolverPool.openDir(dir: dir);
        expect(
          (await opened.tiers()).any((x) => x.kind == TierKindDto.narrow),
          isTrue,
        );
      } finally {
        await cdn.stop();
        Directory(dir).deleteSync(recursive: true);
      }
    },
  );
}
