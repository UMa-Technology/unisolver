/// Unit tests of `DbManager`'s download path, all against a **local HTTP server** that mimics
/// the host contract (manifest-v3.json + fixed paths + `Range` resume + sha256): no native
/// library, no network.
///
/// Decompression and hashing are injected fakes here (the real ones are locked with real tier
/// files by the Rust tests `real_tier_archives_decode_byte_identically` and
/// `sha256_agrees_with_the_manifest_for_a_real_tier`); this file locks download, resume,
/// verification and refusal.
library;

import 'dart:convert';
import 'dart:io';

import 'package:crypto/crypto.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:unisolver_flutter/src/db_format.dart';
import 'package:unisolver_flutter/src/db_manager.dart';
import 'package:unisolver_flutter/src/db_manifest.dart';

/// A programmable static server: serves content by path, optionally dropping the connection
/// after N bytes (for resume cases).
class FakeCdn {
  FakeCdn(this._files);
  final Map<String, List<int>> _files;
  HttpServer? _server;

  /// The first request after this sends only this many bytes, then disconnects (null = send all)
  int? truncateAt;

  /// Ignore Range headers (a server without resume support)
  bool ignoreRange = false;

  /// Chunk size: real downloads arrive in chunks, which cancellation and progress rely on
  int chunkSize = 64 * 1024;

  /// Range headers received, in request order
  final List<String?> ranges = [];

  String get baseUrl => 'http://127.0.0.1:${_server!.port}/';

  Future<void> start() async {
    _server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    _server!.listen((req) async {
      final path = req.uri.path.substring(1);
      final body = _files[path];
      ranges.add(req.headers.value(HttpHeaders.rangeHeader));
      if (body == null) {
        req.response.statusCode = HttpStatus.notFound;
        await req.response.close();
        return;
      }
      var start = 0;
      final range = ignoreRange ? null : req.headers.value(HttpHeaders.rangeHeader);
      if (range != null && range.startsWith('bytes=')) {
        start = int.parse(range.substring(6).split('-').first);
        if (start >= body.length) {
          req.response.statusCode = HttpStatus.requestedRangeNotSatisfiable;
          await req.response.close();
          return;
        }
        req.response.statusCode = HttpStatus.partialContent;
        req.response.headers
            .set(HttpHeaders.contentRangeHeader, 'bytes $start-${body.length - 1}/${body.length}');
      } else {
        req.response.statusCode = HttpStatus.ok;
      }
      final slice = body.sublist(start);
      final cut = truncateAt;
      if (cut != null) {
        truncateAt = null;
        req.response.add(slice.sublist(0, cut.clamp(0, slice.length)));
        await req.response.flush();
        await req.response.close().catchError((_) {});
        // The client sees a short body; .part is kept for a resume
        return;
      }
      for (var i = 0; i < slice.length; i += chunkSize) {
        req.response.add(slice.sublist(i, (i + chunkSize).clamp(0, slice.length)));
        await req.response.flush();
      }
      await req.response.close();
    });
  }

  Future<void> stop() async => _server?.close(force: true);
}

void main() {
  late Directory tmp;
  late FakeCdn cdn;
  late List<int> tierBytes;
  late String tierSha;
  late List<int> namesBytes;
  late String namesSha;
  late String manifestText;
  late List<int> idxRaw;
  late List<int> starsRaw;
  late Map<String, List<int>> cdnFiles;

  /// Fake decompression: copies the "archive" to the database file as-is (this tests the path,
  /// not zstd), refusing a different decompressed digest the way the Rust installer does
  Future<BigInt> fakeInstall({required String zstPath, required String outPath, String? rawSha256}) async {
    final b = File(zstPath).readAsBytesSync();
    final got = sha256.convert(b).toString();
    if (rawSha256 != null && got != rawSha256) {
      throw Exception('raw sha256 mismatch: expected $rawSha256, got $got');
    }
    File(outPath).writeAsBytesSync(b, flush: true);
    return BigInt.from(b.length);
  }

  Future<String> realDigest({required String path}) async =>
      sha256.convert(File(path).readAsBytesSync()).toString();

  DbManager manager({
    bool isMobile = false,
    DbRegister? register,
    DbRegisterPackage? registerPackage,
    String? baseUrl,
    DbInstaller? installer,
    DbFreeSpace? freeSpace,
    DbEngineVersion? engineVersion,
  }) =>
      DbManager(
        dir: tmp.path,
        baseUrl: baseUrl ?? cdn.baseUrl,
        installer: installer ?? fakeInstall,
        digest: realDigest,
        register: register,
        registerPackage: registerPackage,
        freeSpace: freeSpace ?? (_) async => 1 << 40,
        engineVersion: engineVersion ?? () => '0.5.0',
        isMobile: isMobile,
      );

  setUp(() async {
    tmp = Directory.systemTemp.createTempSync('unisolver_dbmgr');
    // A 400 KB "tier" with verifiable content, headed like a format-2 database
    tierBytes = [
      ...format2Header,
      ...List<int>.generate(400 * 1024, (i) => (i * 31 + 7) & 0xFF),
    ];
    tierSha = sha256.convert(tierBytes).toString();
    namesBytes = List<int>.generate(210 * 1024, (i) => (i * 17 + 3) & 0xFF);
    namesSha = sha256.convert(namesBytes).toString();
    // A two-file package headed like a blind index (UNIBLIX1) and star tiles (UNISTAR1)
    idxRaw = [0x55, 0x4e, 0x49, 0x42, 0x4c, 0x49, 0x58, 0x31, ...List<int>.generate(300 * 1024, (i) => (i * 13 + 1) & 0xFF)];
    starsRaw = [0x55, 0x4e, 0x49, 0x53, 0x54, 0x41, 0x52, 0x31, ...List<int>.generate(200 * 1024, (i) => (i * 7 + 5) & 0xFF)];
    final idxSha = sha256.convert(idxRaw).toString(), starsSha = sha256.convert(starsRaw).toString();
    manifestText = json.encode({
      'version': 3,
      'base_url': 'https://example.invalid/unisolver/',
      'tiers': [
        {
          'name': 'unisolver_10_80',
          'file': 'unisolver_10_80.db.zst',
          'min_fov_deg': 10.0,
          'max_fov_deg': 80.0,
          'bytes': 16,
          'sha256': 'f' * 64,
          'mobile': true,
          'bundled': true,
        },
        {
          'name': 'tier_5_10',
          'file': 'tier_5_10.db.zst',
          'key': 'db/deadbeef/tier_5_10.db.zst',
          'min_fov_deg': 5.0,
          'max_fov_deg': 10.0,
          'bytes': tierBytes.length,
          'raw_bytes': tierBytes.length * 3,
          'raw_sha256': tierSha,
          'sha256': tierSha,
          'mobile': true,
          'license': 'CC-BY-SA-3.0-IGO',
          'attribution': 'This work has made use of data from the ESA mission Gaia.',
        },
        {
          'name': 'tier_0p5_1',
          'file': 'tier_0p5_1.db.zst',
          'key': 'db/cafef00d/tier_0p5_1.db.zst',
          'min_fov_deg': 0.5,
          'max_fov_deg': 1.0,
          'bytes': tierBytes.length,
          'sha256': tierSha,
          'mobile': false,
        },
      ],
      'packages': [
        {
          'name': 'np',
          'kind': 'blind-index',
          'min_fov_deg': 0.18,
          'max_fov_deg': 3.1,
          'mobile': false,
          'min_engine': '0.5.0',
          'files': [
            {'role': 'index', 'file': 'np.idx.zst', 'key': 'pkg/aaaa/np.idx.zst', 'bytes': idxRaw.length, 'sha256': idxSha, 'raw_bytes': idxRaw.length, 'raw_sha256': idxSha},
            {'role': 'stars', 'file': 'np.stars.zst', 'key': 'pkg/bbbb/np.stars.zst', 'bytes': starsRaw.length, 'sha256': starsSha, 'raw_bytes': starsRaw.length, 'raw_sha256': starsSha},
          ],
        },
      ],
      'assets': [
        {
          'name': 'unisolver_names',
          'kind': 'names',
          'file': 'unisolver_names.bin',
          'key': 'assets/0badf00d/unisolver_names.bin',
          'bytes': namesBytes.length,
          'sha256': namesSha,
          'license': 'GPL-2.0-or-later',
          'attribution': 'Object names and translations from Stellarium (GPL-2.0-or-later).',
        },
      ],
    });
    cdnFiles = {
      'manifest-v3.json': utf8.encode(manifestText),
      'db/deadbeef/tier_5_10.db.zst': tierBytes,
      'db/cafef00d/tier_0p5_1.db.zst': tierBytes,
      'assets/0badf00d/unisolver_names.bin': namesBytes,
      'pkg/aaaa/np.idx.zst': idxRaw,
      'pkg/bbbb/np.stars.zst': starsRaw,
    };
    cdn = FakeCdn(cdnFiles);
    await cdn.start();
  });

  tearDown(() async {
    await cdn.stop();
    tmp.deleteSync(recursive: true);
  });

  test('manifest is cached after fetching and readable offline; the bundled tier has no key', () async {
    final m = await manager().fetchManifest();
    expect(m.tiers.map((t) => t.name).toList(),
        ['unisolver_10_80', 'tier_5_10', 'tier_0p5_1']);
    expect(m.tiers.first.bundled, isTrue);
    expect(m.tiers.first.key, isNull);
    expect(m.byName('tier_5_10')!.rawBytes, tierBytes.length * 3);
    // Cached on disk → tiers can be listed offline
    expect(File('${tmp.path}/manifest-v3.json').existsSync(), isTrue);
    final offline = DbManager(dir: tmp.path, installer: fakeInstall, digest: realDigest);
    expect(offline.cachedManifest()!.tiers.length, 3);
  });

  test('a version 2 manifest lists databases this engine no longer reads', () {
    expect(
      () => DbManifest.parse('{"version": 2, "tiers": []}'),
      throwsA(isA<DbManifestException>()
          .having((e) => e.toString(), 'message', contains('manifest-v3.json'))),
    );
  });

  test('a database left by an earlier release counts as not installed and is replaced', () async {
    final m = manager();
    final tier = (await m.fetchManifest()).byName('tier_5_10')!;
    File(m.dbPath(tier))
      ..createSync(recursive: true)
      ..writeAsBytesSync([...'UNISOLV2'.codeUnits, ...List.filled(64, 0)]);
    expect(m.isInstalled(tier), isFalse);
    await m.install(tier);
    expect(m.isInstalled(tier), isTrue);
    expect(File(m.dbPath(tier)).readAsBytesSync(), tierBytes);
  });

  test('a manifest newer than the engine is an error, not a guess', () {
    expect(
      () => DbManifest.parse('{"version": 99, "tiers": []}'),
      throwsA(isA<DbManifestException>()),
    );
  });

  test('normal install: verify → decompress → register, byte-identical result', () async {
    final registered = <String>[];
    final m = manager(register: (p) async => registered.add(p));
    final tier = (await m.fetchManifest()).byName('tier_5_10')!;
    final phases = <DbPhase>[];
    final path = await m.install(tier, onProgress: (p) => phases.add(p.phase));

    expect(File(path).readAsBytesSync(), tierBytes);
    expect(registered, [path]);
    expect(phases, containsAllInOrder([
      DbPhase.downloading,
      DbPhase.verifying,
      DbPhase.decompressing,
      DbPhase.registering,
      DbPhase.done,
    ]));
    // Intermediate files are cleaned up
    expect(File('${tmp.path}/${tier.file}').existsSync(), isFalse);
    expect(File('${tmp.path}/${tier.file}.part').existsSync(), isFalse);
    expect(m.isInstalled(tier), isTrue);
    // Idempotent: installing again returns at once
    expect(await m.install(tier), path);
  });

  test('resume after a dropped connection: the second request sends Range for the rest only', () async {
    final m = manager();
    final tier = (await m.fetchManifest()).byName('tier_5_10')!;
    cdn.truncateAt = 100 * 1024; // drop the connection after 100 KB
    await expectLater(m.install(tier), throwsA(isA<DbException>()));
    expect(m.partialBytes(tier), 100 * 1024, reason: '.part must be kept');

    cdn.ranges.clear();
    final path = await m.install(tier); // resume
    expect(cdn.ranges.last, 'bytes=${100 * 1024}-');
    expect(File(path).readAsBytesSync(), tierBytes, reason: 'a resumed download must be complete and identical');
  });

  test('a server ignoring Range (200, full body) restarts cleanly instead of splicing a bad file', () async {
    final m = manager();
    final tier = (await m.fetchManifest()).byName('tier_5_10')!;
    cdn.truncateAt = 50 * 1024;
    await expectLater(m.install(tier), throwsA(isA<DbException>()));
    expect(m.partialBytes(tier), 50 * 1024);

    cdn.ignoreRange = true;
    final path = await m.install(tier);
    expect(File(path).readAsBytesSync(), tierBytes);
  });

  test('a sha256 mismatch is never installed and the bad .part is deleted', () async {
    final m = manager();
    final good = (await m.fetchManifest()).byName('tier_5_10')!;
    // The manifest says sha256 A; the server serves B (tampering or a mixed-up mirror)
    final tampered = DbTier(
      name: good.name,
      file: good.file,
      key: good.key,
      minFovDeg: good.minFovDeg,
      maxFovDeg: good.maxFovDeg,
      bytes: good.bytes,
      sha256: '0' * 64,
      mobile: true,
      bundled: false,
    );
    await expectLater(m.install(tampered), throwsA(isA<DbChecksumException>()));
    expect(m.partialBytes(tampered), 0, reason: 'bad content must not be kept for a "resume"');
    expect(m.isInstalled(tampered), isFalse);
  });

  test('a path missing on the host (404) reports the HTTP status instead of installing nothing', () async {
    final m = manager();
    final tier = DbTier(
      name: 'unisolver_ghost',
      file: 'unisolver_ghost.db.zst',
      key: 'db/00000000/unisolver_ghost.db.zst',
      minFovDeg: 1,
      maxFovDeg: 2,
      bytes: 10,
      sha256: '0' * 64,
      mobile: true,
      bundled: false,
    );
    await expectLater(
      m.install(tier),
      throwsA(isA<DbHttpException>().having((e) => e.statusCode, 'status', 404)),
    );
    expect(m.isInstalled(tier), isFalse);
  });

  test('mobile refuses non-mobile tiers unless explicitly allowed', () async {
    final m = manager(isMobile: true);
    final deep = (await m.fetchManifest()).byName('tier_0p5_1')!;
    await expectLater(m.install(deep), throwsA(isA<DbException>()));
    final path = await m.install(deep, allowNonMobile: true);
    expect(File(path).existsSync(), isTrue);
  });

  test('the bundled tier is not installed from the network (points to the asset installer)', () async {
    final m = manager();
    final w = (await m.fetchManifest()).byName('unisolver_10_80')!;
    await expectLater(
      m.install(w),
      throwsA(isA<DbException>().having(
          (e) => e.message, 'message', contains('ships with the plugin'))),
    );
  });

  test('a stale leftover (.part larger than the manifest) restarts instead of appending', () async {
    final m = manager();
    final tier = (await m.fetchManifest()).byName('tier_5_10')!;
    File('${tmp.path}/${tier.file}.part')
        .writeAsBytesSync(List<int>.filled(tier.bytes + 1234, 9));
    cdn.ranges.clear();
    final path = await m.install(tier);
    expect(cdn.ranges.last, isNull, reason: 'must restart without Range');
    expect(File(path).readAsBytesSync(), tierBytes);
  });

  test('a failed decompression (e.g. a full disk) leaves no partial database and states the space needed', () async {
    final m = manager(
      installer: ({required String zstPath, required String outPath, String? rawSha256}) async {
        File(outPath).writeAsBytesSync([1, 2, 3]); // partial
        throw const FileSystemException('No space left on device');
      },
    );
    final tier = (await m.fetchManifest()).byName('tier_5_10')!;
    await expectLater(
      m.install(tier),
      throwsA(isA<DbException>()
          .having((e) => e.message, 'message', contains('MB free'))),
    );
    expect(m.isInstalled(tier), isFalse, reason: 'a partial database must be deleted');
    // The archive is kept, saving the next download
    expect(File('${tmp.path}/${tier.file}').existsSync(), isTrue);
  });

  test('cancelling keeps progress for the next resume', () async {
    final m = manager();
    final tier = (await m.fetchManifest()).byName('tier_5_10')!;
    final cancel = DbCancel();
    var seen = 0;
    await expectLater(
      m.install(tier, cancel: cancel, onProgress: (p) {
        seen = p.received;
        if (p.received > 0) cancel.cancel();
      }),
      throwsA(isA<DbCancelledException>()),
    );
    expect(seen, greaterThan(0));
    expect(m.partialBytes(tier), greaterThan(0));
    final path = await m.install(tier);
    expect(File(path).readAsBytesSync(), tierBytes);
  });

  test('assets parse; a manifest without assets has none', () async {
    final m = await manager().fetchManifest();
    final names = m.assetByName('unisolver_names')!;
    expect(names.kind, 'names');
    expect(names.license, 'GPL-2.0-or-later');
    expect(names.attribution, contains('Stellarium'));
    expect(DbManifest.parse('{"version": 3, "tiers": []}').assets, isEmpty);
  });

  test('installAsset downloads and verifies, with no decompression', () async {
    final m = manager();
    final asset = (await m.fetchManifest()).assetByName('unisolver_names')!;
    final phases = <DbPhase>[];
    final path = await m.installAsset(asset, onProgress: (p) => phases.add(p.phase));
    expect(File(path).readAsBytesSync(), namesBytes);
    expect(path, m.assetPath(asset));
    expect(m.isAssetInstalled(asset), isTrue);
    expect(phases, isNot(contains(DbPhase.decompressing)));
    expect(phases.last, DbPhase.done);
    expect(File('${tmp.path}/${asset.file}.part').existsSync(), isFalse);
    expect(await m.installAsset(asset), path); // idempotent
    await m.removeAsset(asset);
    expect(m.isAssetInstalled(asset), isFalse);
  });

  test('installAsset resumes and refuses a sha256 mismatch', () async {
    final m = manager();
    final asset = (await m.fetchManifest()).assetByName('unisolver_names')!;
    cdn.truncateAt = 64 * 1024;
    await expectLater(m.installAsset(asset), throwsA(isA<DbException>()));
    cdn.ranges.clear();
    final path = await m.installAsset(asset);
    expect(cdn.ranges.last, 'bytes=${64 * 1024}-');
    expect(File(path).readAsBytesSync(), namesBytes);

    final tampered = DbAsset(
      name: 'other',
      kind: 'names',
      file: 'other.bin',
      key: asset.key,
      bytes: asset.bytes,
      sha256: '0' * 64,
    );
    await expectLater(m.installAsset(tampered), throwsA(isA<DbChecksumException>()));
    expect(m.isAssetInstalled(tampered), isFalse);
  });

  test('tiers carry their license and attribution when the manifest has them', () async {
    final m = await manager().fetchManifest();
    final n1 = m.byName('tier_5_10')!;
    expect(n1.license, 'CC-BY-SA-3.0-IGO');
    expect(n1.attribution, contains('Gaia'));
    expect(m.byName('tier_0p5_1')!.license, isNull); // older entries have none
  });

  group('packages', () {
    Future<DbPackage> pkg() async => (await manager().fetchManifest()).packageByName('np')!;

    test('both files install, verify twice and register once with both paths', () async {
      final p = await pkg();
      final calls = <(String, String)>[];
      final progress = <DbProgress>[];
      final m = manager(registerPackage: (i, s) async => calls.add((i, s)));
      final paths = await m.installPackage(p, onProgress: progress.add);
      expect(File(paths.indexPath).readAsBytesSync(), idxRaw);
      expect(File(paths.starsPath).readAsBytesSync(), starsRaw);
      expect(paths.indexPath, '${tmp.path}/np.idx');
      expect(calls, [(paths.indexPath, paths.starsPath)]);
      expect(m.isPackageInstalled(p), isTrue);
      expect(progress.last.phase, DbPhase.done);
      expect(tmp.listSync().where((e) => e.path.endsWith('.zst') || e.path.endsWith('.part')), isEmpty);
      // Installed already: nothing is downloaded again
      final requests = cdn.ranges.length;
      await m.installPackage(p);
      expect(cdn.ranges.length, requests);
    });

    test('mobile refuses a package outright', () async {
      final p = await pkg();
      await expectLater(manager(isMobile: true).installPackage(p), throwsA(isA<DbException>()));
    });

    test('an engine older than min_engine is told to upgrade', () async {
      final p = await pkg();
      await expectLater(
        manager(engineVersion: () => '0.4.4').installPackage(p),
        throwsA(isA<DbException>().having((e) => e.message, 'message', contains('upgrade'))),
      );
    });

    test('too little free disk is refused before downloading', () async {
      final p = await pkg();
      final requests = cdn.ranges.length;
      await expectLater(manager(freeSpace: (_) async => 10).installPackage(p), throwsA(isA<DbException>()));
      expect(cdn.ranges.length, requests);
    });

    test('a decompressed file with the wrong digest is refused and removed', () async {
      final p = await pkg();
      final s = p.stars;
      final bad = DbPackage(
        name: p.name,
        kind: p.kind,
        minFovDeg: p.minFovDeg,
        maxFovDeg: p.maxFovDeg,
        mobile: p.mobile,
        minEngine: p.minEngine,
        files: [
          p.index,
          DbPackageFile(package: p.name, role: 'stars', file: s.file, key: s.key, bytes: s.bytes, sha256: s.sha256, rawBytes: s.rawBytes, rawSha256: '0' * 64),
        ],
      );
      await expectLater(manager().installPackage(bad), throwsA(isA<DbChecksumException>()));
      expect(File('${tmp.path}/np.stars').existsSync(), isFalse);
      expect(File('${tmp.path}/np.stars.zst').existsSync(), isFalse);
      expect(manager().isPackageInstalled(bad), isFalse);
    });

    test('a failed second file keeps the first, and the next install fetches only the second', () async {
      final p = await pkg();
      final stars = cdnFiles.remove('pkg/bbbb/np.stars.zst')!;
      await expectLater(manager().installPackage(p), throwsA(isA<DbHttpException>()));
      expect(File('${tmp.path}/np.idx').existsSync(), isTrue);
      expect(manager().isPackageInstalled(p), isFalse);
      cdnFiles['pkg/bbbb/np.stars.zst'] = stars;
      final requests = cdn.ranges.length;
      await manager().installPackage(p);
      expect(cdn.ranges.length, requests + 1, reason: 'only the star tiles are fetched');
      expect(manager().isPackageInstalled(p), isTrue);
    });

    test('a dropped download resumes', () async {
      final p = await pkg();
      cdn.truncateAt = 100;
      await expectLater(manager().installPackage(p), throwsA(isA<DbException>()));
      await manager().installPackage(p);
      expect(cdn.ranges, contains('bytes=100-'));
      expect(manager().isPackageInstalled(p), isTrue);
    });

    test('a wrong header or size is not installed; remove cleans up', () async {
      final p = await pkg();
      await manager().installPackage(p);
      File('${tmp.path}/np.stars').writeAsBytesSync([0, 1, 2]);
      expect(manager().isPackageInstalled(p), isFalse);
      await manager().removePackage(p);
      expect(tmp.listSync().where((e) => e.path.contains('np.')), isEmpty);
    });
  });

  test('tiers hand their decompressed digest to the installer and refuse a newer min_engine', () async {
    final m0 = await manager().fetchManifest();
    final t = m0.byName('tier_5_10')!;
    String? seen;
    final m = manager(installer: ({required zstPath, required outPath, rawSha256}) async {
      seen = rawSha256;
      return fakeInstall(zstPath: zstPath, outPath: outPath, rawSha256: rawSha256);
    });
    await m.install(t);
    expect(seen, t.rawSha256);
    final newer = DbTier(
      name: 'tier_new',
      file: t.file,
      key: t.key,
      minFovDeg: t.minFovDeg,
      maxFovDeg: t.maxFovDeg,
      bytes: t.bytes,
      sha256: t.sha256,
      mobile: true,
      bundled: false,
      minEngine: '9.0.0',
    );
    await expectLater(manager().install(newer), throwsA(isA<DbException>().having((e) => e.message, 'message', contains('upgrade'))));
  });
}
