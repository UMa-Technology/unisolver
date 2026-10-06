/// Tier acquisition: manifest → download (resumable) → sha256 check → install → register.
///
/// This layer **only acquires**; using installed tiers is `UniSolverPool`'s job. The contract
/// is plain static HTTP (`manifest-v3.json` + fixed paths + `Range` resume + sha256), so any
/// static server can host it, including `scripts/ci/serve_tiers.py` for local rehearsals.
library;

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'db_format.dart';
import 'db_manifest.dart';
import 'db_proxy.dart';
import 'rust/api/install.dart' as rust;
import 'rust/api/proxy.dart' as rust;

/// Decompress-and-install (defaults to Rust's streaming zstd), checking the decompressed
/// sha256 when the manifest gives one. Injectable for pure-Dart tests.
typedef DbInstaller = Future<BigInt> Function(
    {required String zstPath, required String outPath, String? rawSha256});

/// File sha256 (defaults to Rust's streaming implementation). Injectable for pure-Dart tests.
typedef DbDigest = Future<String> Function({required String path});

/// Called after a tier is installed (usually `pool.register`).
typedef DbRegister = Future<void> Function(String dbPath);

/// Called after a package is installed (usually `pool.registerNarrow`).
typedef DbRegisterPackage = Future<void> Function(
    String indexPath, String starsPath);

/// Free bytes on the volume holding a directory (defaults to Rust). Injectable for tests.
typedef DbFreeSpace = Future<int> Function(String dir);

/// This engine's version (defaults to Rust's `engineVersion`). Injectable for tests.
typedef DbEngineVersion = String Function();

enum DbPhase { downloading, verifying, decompressing, registering, done }

class DbProgress {
  const DbProgress({
    required this.name,
    required this.phase,
    required this.received,
    required this.total,
  });

  final String name;
  final DbPhase phase;

  /// Bytes downloaded (including what a resume already had locally)
  final int received;

  /// Total bytes of the archive
  final int total;

  /// 0–1; 0 when total is 0
  double get fraction => total <= 0 ? 0 : (received / total).clamp(0, 1);
}

/// Cancellation token: checked between chunks; cancelling keeps `.part` for the next resume.
class DbCancel {
  bool _cancelled = false;
  bool get isCancelled => _cancelled;
  void cancel() => _cancelled = true;
}

class DbException implements Exception {
  DbException(this.message, {this.cause});
  final String message;
  final Object? cause;
  @override
  String toString() =>
      'DbException: $message${cause == null ? '' : ' ($cause)'}';
}

/// The download does not match the manifest's sha256: **never install it** (a truncated or
/// altered database would only fail when mmap pages it in, far from the cause).
class DbChecksumException extends DbException {
  DbChecksumException({required this.expected, required this.actual})
      : super('sha256 mismatch: expected $expected, got $actual');
  final String expected;
  final String actual;
}

class DbHttpException extends DbException {
  DbHttpException(this.statusCode, String url)
      : super('HTTP $statusCode for $url');
  final int statusCode;
}

class DbCancelledException extends DbException {
  DbCancelledException(String name) : super('download cancelled: $name');
}

class DbManager {
  DbManager({
    required this.dir,
    String? baseUrl,
    DbInstaller? installer,
    DbDigest? digest,
    this.register,
    this.registerPackage,
    DbFreeSpace? freeSpace,
    DbEngineVersion? engineVersion,
    HttpClient Function()? httpClient,
    DbSystemProxy? systemProxy,
    bool? isMobile,
  })  : _baseUrlOverride = baseUrl,
        _install = installer ?? rust.installCompressedFile,
        _digest = digest ?? rust.sha256File,
        _freeSpace = freeSpace ??
            ((d) async => (await rust.availableDiskBytes(path: d)).toInt()),
        _engineVersion = engineVersion ?? rust.engineVersion,
        _newClient = httpClient,
        _systemProxy = systemProxy ??
            ((isMobile ?? (Platform.isAndroid || Platform.isIOS))
                ? _noProxy
                : rust.systemProxy),
        _isMobile = isMobile ?? (Platform.isAndroid || Platform.isIOS);

  /// Directory for installed databases (usually `getApplicationSupportDirectory()`)
  final String dir;

  final String? _baseUrlOverride;
  final DbInstaller _install;
  final DbDigest _digest;

  /// Called after a tier is installed (usually `pool.register`); omit to register yourself
  final DbRegister? register;

  /// Called after a package is installed (usually `pool.registerNarrow`); omit to register yourself
  final DbRegisterPackage? registerPackage;
  final DbFreeSpace _freeSpace;
  final DbEngineVersion _engineVersion;
  /// An injected client factory, used as it is (its proxy included)
  final HttpClient Function()? _newClient;
  final DbSystemProxy _systemProxy;

  static rust.SystemProxy? _noProxy() => null;

  /// A client for one request: the injected factory as it is; otherwise one that takes the
  /// environment's proxy, then the system's (read at each connection, so a change applies at once)
  HttpClient _client() {
    final make = _newClient;
    if (make != null) return make();
    final system = _systemProxy;
    return HttpClient()
      ..findProxy = (uri) =>
          proxyFor(uri, environment: Platform.environment, system: system);
  }
  final bool _isMobile;

  /// Version 3: databases in tetra3's format 2. Hosts keep version 2 (`manifest.json`) for
  /// earlier plugin releases, which cannot read format 2.
  static const _manifestFile = 'manifest-v3.json';

  /// Fetches the manifest and caches it at `dir/manifest-v3.json` ([cachedManifest] works
  /// offline).
  Future<DbManifest> fetchManifest({
    Duration timeout = const Duration(seconds: 15),
  }) async {
    final base = _baseUrlOverride;
    if (base == null || base.isEmpty) {
      throw DbException('no base URL: pass baseUrl or use a cached manifest');
    }
    final url = _join(base, _manifestFile);
    final client = _client()..connectionTimeout = timeout;
    try {
      final resp = await client.getUrl(Uri.parse(url)).then((r) => r.close());
      if (resp.statusCode != HttpStatus.ok) {
        throw DbHttpException(resp.statusCode, url);
      }
      final text = await resp.transform(utf8.decoder).join();
      final m = DbManifest.parse(text);
      // Parse before caching: never cache something unparsable
      Directory(dir).createSync(recursive: true);
      File('$dir/$_manifestFile').writeAsStringSync(text, flush: true);
      return m;
    } on SocketException catch (e) {
      throw DbException('cannot reach $url', cause: e);
    } finally {
      client.close(force: true);
    }
  }

  /// The last fetched manifest, or null.
  DbManifest? cachedManifest() {
    final f = File('$dir/$_manifestFile');
    if (!f.existsSync()) return null;
    try {
      return DbManifest.parse(f.readAsStringSync());
    } on Object {
      return null; // a broken cache counts as none; refetch when online
    }
  }

  String dbPath(DbTier t) => '$dir/${t.name}.db';

  String _partPath(DbItem t) => '$dir/${t.file}.part';

  /// Whether the tier's database is on disk in the format this engine reads. One left by an
  /// earlier release (the UNISOLV2 container) counts as not installed, so [install] replaces it.
  bool isInstalled(DbTier t) => isFormat2File(File(dbPath(t)));

  /// Paths of installed databases (`UniSolverPool.openDir` takes [dir] itself; this is for UIs)
  List<String> installedPaths() {
    final d = Directory(dir);
    if (!d.existsSync()) return const [];
    return d
        .listSync()
        .whereType<File>()
        .map((f) => f.path)
        .where((p) => p.endsWith('.db'))
        .toList()
      ..sort();
  }

  /// Bytes of a partial download (0 = none). UIs use it to offer "resume".
  int partialBytes(DbTier t) {
    final f = File(_partPath(t));
    return f.existsSync() ? f.lengthSync() : 0;
  }

  /// Installs a tier: download (resumable) → verify → decompress → register. Returns the database path.
  ///
  /// Returns at once when already installed (idempotent). `allowNonMobile` permits tiers the
  /// manifest marks as not for mobile (when the user knowingly asks for one).
  Future<String> install(
    DbTier t, {
    void Function(DbProgress)? onProgress,
    DbCancel? cancel,
    bool allowNonMobile = false,
  }) async {
    final out = dbPath(t);
    if (isInstalled(t)) {
      onProgress?.call(DbProgress(
          name: t.name, phase: DbPhase.done, received: t.bytes, total: t.bytes));
      return out;
    }
    if (t.bundled) {
      throw DbException(
        '${t.name} ships with the plugin: install it with '
        'UnisolverAssets.ensureInstalled(), not from the network',
      );
    }
    _requireEngine(t.name, t.minEngine);
    if (_isMobile && !t.mobile && !allowNonMobile) {
      throw DbException(
        '${t.name} is not marked mobile in the manifest '
        '(${t.fovLabel}, ${(t.diskBytesNeeded / 1e6).round()} MB peak) — '
        'pass allowNonMobile: true to override',
      );
    }
    final part = await _fetchVerified(t, onProgress: onProgress, cancel: cancel);

    // Decompress: streaming on the Rust side (buffering once peaked at 1.59 GB for N2)
    final zst = File('$dir/${t.file}');
    if (zst.existsSync()) zst.deleteSync();
    part.renameSync(zst.path);
    onProgress?.call(DbProgress(
        name: t.name,
        phase: DbPhase.decompressing,
        received: t.bytes,
        total: t.bytes));
    // The decompressor keeps an existing non-empty file: drop a database in an old format first
    final old = File(out);
    if (old.existsSync()) old.deleteSync();
    try {
      await _install(zstPath: zst.path, outPath: out, rawSha256: t.rawSha256);
    } on Object catch (e) {
      // Decompression failed (usually a full disk): keep the .zst to save a download, drop the database
      final f = File(out);
      if (f.existsSync()) f.deleteSync();
      final mismatch = _rawMismatch(e);
      if (mismatch != null) {
        // The archive passed its own check, so it is the published one: drop it and start over
        zst.deleteSync();
        throw mismatch;
      }
      throw DbException(
        '${t.name}: decompression failed — needs about '
        '${(t.diskBytesNeeded / 1e6).round()} MB free',
        cause: e,
      );
    }
    zst.deleteSync();

    final reg = register;
    if (reg != null) {
      onProgress?.call(DbProgress(
          name: t.name,
          phase: DbPhase.registering,
          received: t.bytes,
          total: t.bytes));
      await reg(out);
    }
    onProgress?.call(DbProgress(
        name: t.name, phase: DbPhase.done, received: t.bytes, total: t.bytes));
    return out;
  }

  /// Refuses an item that needs a newer engine than this one
  void _requireEngine(String name, String? minEngine) {
    if (minEngine == null) return;
    final engine = _engineVersion();
    if (!engineSatisfies(engine, minEngine)) {
      throw DbException('$name needs engine $minEngine or newer (this is $engine) — '
          'upgrade unisolver_flutter');
    }
  }

  static final RegExp _mismatch = RegExp(
      r'raw sha256 mismatch: expected ([0-9a-f]{64}), got ([0-9a-f]{64})');

  /// The installer's digest refusal as a [DbChecksumException] (Rust reports it as text)
  static DbChecksumException? _rawMismatch(Object e) {
    final m = _mismatch.firstMatch(e.toString());
    return m == null
        ? null
        : DbChecksumException(expected: m.group(1)!, actual: m.group(2)!);
  }

  /// Local path of a package file (the archive name without `.zst`)
  String packageFilePath(DbPackageFile f) => '$dir/${f.localFile}';

  static const Map<String, List<int>> _headers = {
    'index': [0x55, 0x4e, 0x49, 0x42, 0x4c, 0x49, 0x58, 0x31], // UNIBLIX1
    'stars': [0x55, 0x4e, 0x49, 0x53, 0x54, 0x41, 0x52, 0x31], // UNISTAR1
  };

  bool _fileInstalled(DbPackageFile f) {
    final file = File(packageFilePath(f));
    if (!file.existsSync() || file.lengthSync() != f.rawBytes) return false;
    final want = _headers[f.role];
    if (want == null) return true;
    final raf = file.openSync();
    try {
      final head = raf.readSync(8);
      if (head.length != 8) return false;
      for (var i = 0; i < 8; i++) {
        if (head[i] != want[i]) return false;
      }
      return true;
    } finally {
      raf.closeSync();
    }
  }

  /// Whether every file of [p] is on disk with the manifest's size and the right header (the
  /// digests are checked as they are installed)
  bool isPackageInstalled(DbPackage p) => p.files.every(_fileInstalled);

  /// Installs a package file by file: download (resumable) → verify → decompress → verify the
  /// decompressed digest. A file already in place is kept, so a failed install resumes where it
  /// stopped; once every file is in place the package is registered. Desktop only: mobile engines
  /// are built without the narrow-field engine. Returns the installed paths.
  Future<({String indexPath, String starsPath})> installPackage(
    DbPackage p, {
    void Function(DbProgress)? onProgress,
    DbCancel? cancel,
  }) async {
    final paths = (
      indexPath: packageFilePath(p.index),
      starsPath: packageFilePath(p.stars),
    );
    if (_isMobile) {
      throw DbException('${p.name} is for desktop builds only');
    }
    _requireEngine(p.name, p.minEngine);
    if (!isPackageInstalled(p)) {
      Directory(dir).createSync(recursive: true);
      final inPlace =
          p.files.where(_fileInstalled).fold(0, (s, f) => s + f.rawBytes);
      final need = p.diskBytesNeeded - inPlace;
      final free = await _freeSpace(dir);
      if (free < need) {
        throw DbException('${p.name} needs about ${(need / 1e6).round()} MB '
            'free, ${(free / 1e6).round()} MB available');
      }
      for (final f in p.files) {
        if (_fileInstalled(f)) continue;
        final part =
            await _fetchVerified(f, onProgress: onProgress, cancel: cancel);
        final zst = File('$dir/${f.file}');
        if (zst.existsSync()) zst.deleteSync();
        part.renameSync(zst.path);
        onProgress?.call(DbProgress(
            name: f.name,
            phase: DbPhase.decompressing,
            received: f.bytes,
            total: f.bytes));
        final out = File(packageFilePath(f));
        if (out.existsSync()) out.deleteSync();
        try {
          await _install(
              zstPath: zst.path, outPath: out.path, rawSha256: f.rawSha256);
        } on Object catch (e) {
          if (out.existsSync()) out.deleteSync();
          final mismatch = _rawMismatch(e);
          if (mismatch != null) {
            // The archive passed its own check, so it is the published one: start over
            zst.deleteSync();
            throw mismatch;
          }
          throw DbException(
            '${f.name}: decompression failed — needs about '
            '${(f.rawBytes / 1e6).round()} MB free',
            cause: e,
          );
        }
        zst.deleteSync();
      }
    }
    final reg = registerPackage;
    if (reg != null) {
      onProgress?.call(DbProgress(
          name: p.name,
          phase: DbPhase.registering,
          received: p.downloadBytes,
          total: p.downloadBytes));
      await reg(paths.indexPath, paths.starsPath);
    }
    onProgress?.call(DbProgress(
        name: p.name,
        phase: DbPhase.done,
        received: p.downloadBytes,
        total: p.downloadBytes));
    return paths;
  }

  /// Removes a package's files with any leftover download or decompression. A pool that
  /// registered it keeps it until the pool is reopened (and on Windows the mapped files cannot be
  /// deleted until then).
  Future<void> removePackage(DbPackage p) async {
    for (final f in p.files) {
      for (final path in [
        packageFilePath(f),
        '${packageFilePath(f)}.tmp',
        _partPath(f),
        '$dir/${f.file}',
      ]) {
        final file = File(path);
        if (file.existsSync()) file.deleteSync();
      }
    }
  }

  /// Installs an optional asset (the names pack): download (resumable) → verify. Assets
  /// are used as downloaded, without decompression. Returns the local path.
  Future<String> installAsset(
    DbAsset a, {
    void Function(DbProgress)? onProgress,
    DbCancel? cancel,
  }) async {
    final out = assetPath(a);
    if (isAssetInstalled(a)) {
      onProgress?.call(DbProgress(
          name: a.name, phase: DbPhase.done, received: a.bytes, total: a.bytes));
      return out;
    }
    final part = await _fetchVerified(a, onProgress: onProgress, cancel: cancel);
    final f = File(out);
    if (f.existsSync()) f.deleteSync();
    part.renameSync(out);
    onProgress?.call(DbProgress(
        name: a.name, phase: DbPhase.done, received: a.bytes, total: a.bytes));
    return out;
  }

  /// Local path of an asset.
  String assetPath(DbAsset a) => '$dir/${a.file}';

  bool isAssetInstalled(DbAsset a) {
    final f = File(assetPath(a));
    return f.existsSync() && f.lengthSync() == a.bytes;
  }

  /// Removes an asset (with any leftover `.part`).
  Future<void> removeAsset(DbAsset a) async {
    for (final p in [assetPath(a), _partPath(a)]) {
      final f = File(p);
      if (f.existsSync()) f.deleteSync();
    }
  }

  /// Downloads [t] to its `.part` file and verifies it. Returns the verified `.part`.
  Future<File> _fetchVerified(
    DbItem t, {
    void Function(DbProgress)? onProgress,
    DbCancel? cancel,
  }) async {
    if (t.key == null || t.key!.isEmpty) {
      throw DbException('${t.name} has no download key in the manifest');
    }
    Directory(dir).createSync(recursive: true);
    final part = File(_partPath(t));
    await _download(t, part, onProgress: onProgress, cancel: cancel);

    // Verify: length first (cheap), then sha256. A **short file keeps its `.part`**: a dropped
    // connection and an early close look the same over HTTP, and keeping it allows a resume;
    // an oversized one was handled before downloading.
    final len = part.lengthSync();
    if (len != t.bytes) {
      throw DbException(
        '${t.name}: download incomplete ($len of ${t.bytes} bytes) — '
        'install again to resume',
      );
    }
    onProgress?.call(DbProgress(
        name: t.name, phase: DbPhase.verifying, received: len, total: t.bytes));
    final got = (await _digest(path: part.path)).toLowerCase();
    if (got != t.sha256) {
      // Do not keep bad content, or every later resume would continue from bad bytes
      part.deleteSync();
      throw DbChecksumException(expected: t.sha256, actual: got);
    }
    return part;
  }

  /// Removes a tier (with any leftover `.part`/`.zst`). The bundled tier may be removed too;
  /// it reinstalls from the plugin assets.
  Future<void> remove(DbTier t) async {
    for (final p in [dbPath(t), _partPath(t), '$dir/${t.file}']) {
      final f = File(p);
      if (f.existsSync()) f.deleteSync();
    }
  }

  /// Downloads to `.part`, resuming when possible.
  Future<void> _download(
    DbItem t,
    File part, {
    void Function(DbProgress)? onProgress,
    DbCancel? cancel,
  }) async {
    var have = part.existsSync() ? part.lengthSync() : 0;
    if (have > t.bytes) {
      // Local file larger than the manifest = a stale leftover (the tier was regenerated); restart
      part.deleteSync();
      have = 0;
    }
    if (have == t.bytes) return; // finished last time but interrupted before verifying
    final base = _baseUrlOverride ?? cachedManifest()?.baseUrl ?? '';
    if (base.isEmpty) throw DbException('no base URL for ${t.name}');
    final url = _join(base, t.key!);

    final client = _client();
    try {
      final req = await client.getUrl(Uri.parse(url));
      if (have > 0) req.headers.set(HttpHeaders.rangeHeader, 'bytes=$have-');
      final resp = await req.close();
      var mode = FileMode.append;
      switch (resp.statusCode) {
        case HttpStatus.partialContent: // 206: resume
          break;
        case HttpStatus.ok: // 200: the server ignored Range; start over
          have = 0;
          mode = FileMode.write;
          break;
        case HttpStatus.requestedRangeNotSatisfiable: // 416: the leftover does not match; restart
          if (part.existsSync()) part.deleteSync();
          throw DbException(
            '${t.name}: server rejected the resume range — '
            'stale partial file removed, retry to download from scratch',
          );
        default:
          throw DbHttpException(resp.statusCode, url);
      }

      final sink = part.openSync(mode: mode);
      var received = have;
      try {
        onProgress?.call(DbProgress(
            name: t.name,
            phase: DbPhase.downloading,
            received: received,
            total: t.bytes));
        await for (final chunk in resp) {
          if (cancel?.isCancelled ?? false) {
            // Cancelling keeps .part: the next attempt resumes here
            throw DbCancelledException(t.name);
          }
          sink.writeFromSync(chunk);
          received += chunk.length;
          onProgress?.call(DbProgress(
              name: t.name,
              phase: DbPhase.downloading,
              received: received,
              total: t.bytes));
        }
      } finally {
        sink.flushSync();
        sink.closeSync();
      }
    } on HttpException catch (e) {
      // The connection dropped midway: keep .part for the next resume
      throw DbException('${t.name}: download interrupted', cause: e);
    } finally {
      client.close(force: true);
    }
  }

  static String _join(String base, String path) =>
      base.endsWith('/') ? '$base$path' : '$base/$path';
}
