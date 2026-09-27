/// Unit tests of `DbManager`'s download path, all against a **local HTTP server** that mimics
/// the host contract (manifest.json + fixed paths + `Range` resume + sha256): no native
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

  /// Fake decompression: copies the "archive" to the database file as-is (this tests the path, not zstd)
  Future<BigInt> fakeInstall({required String zstPath, required String outPath}) async {
    final b = File(zstPath).readAsBytesSync();
    File(outPath).writeAsBytesSync(b, flush: true);
    return BigInt.from(b.length);
  }

  Future<String> realDigest({required String path}) async =>
      sha256.convert(File(path).readAsBytesSync()).toString();

  DbManager manager({
    bool isMobile = false,
    DbRegister? register,
    String? baseUrl,
    DbInstaller? installer,
  }) =>
      DbManager(
        dir: tmp.path,
        baseUrl: baseUrl ?? cdn.baseUrl,
        installer: installer ?? fakeInstall,
        digest: realDigest,
        register: register,
        isMobile: isMobile,
      );

  setUp(() async {
    tmp = Directory.systemTemp.createTempSync('unisolver_dbmgr');
    // A 400 KB "tier" with verifiable content
    tierBytes = List<int>.generate(400 * 1024, (i) => (i * 31 + 7) & 0xFF);
    tierSha = sha256.convert(tierBytes).toString();
    namesBytes = List<int>.generate(210 * 1024, (i) => (i * 17 + 3) & 0xFF);
    namesSha = sha256.convert(namesBytes).toString();
    manifestText = json.encode({
      'version': 2,
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
    cdn = FakeCdn({
      'manifest.json': utf8.encode(manifestText),
      'db/deadbeef/tier_5_10.db.zst': tierBytes,
      'db/cafef00d/tier_0p5_1.db.zst': tierBytes,
      'assets/0badf00d/unisolver_names.bin': namesBytes,
    });
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
    expect(File('${tmp.path}/manifest.json').existsSync(), isTrue);
    final offline = DbManager(dir: tmp.path, installer: fakeInstall, digest: realDigest);
    expect(offline.cachedManifest()!.tiers.length, 3);
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
      installer: ({required String zstPath, required String outPath}) async {
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
    expect(DbManifest.parse('{"version": 2, "tiers": []}').assets, isEmpty);
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
}
