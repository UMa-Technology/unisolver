import 'dart:io';

import 'package:flutter/foundation.dart' show FlutterError;
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';

import 'src/db_format.dart';
import 'src/db_manager.dart';
import 'src/db_manifest.dart';
import 'src/rust/api/install.dart' as rust;

/// First launch: install the bundled assets into the app support directory (database
/// decompressed by Rust's zstd, DSO catalog copied). Idempotent. The constellation pack is
/// bundled too; [installConstellations] copies it out.
///
/// The multilingual names pack is **opt-in** because it is GPL-2.0-or-later: either
/// declare `packages/unisolver_flutter/optional/unisolver_names.bin` in your app's
/// pubspec assets and call [installNames], or download it with `DbManager.installAsset`.
class UnisolverAssets {
  static const _dbAsset =
      'packages/unisolver_flutter/assets/unisolver_10_80.db.zst';
  static const _dsoAsset =
      'packages/unisolver_flutter/assets/unisolver_dso.bin';
  static const _namesAsset =
      'packages/unisolver_flutter/optional/unisolver_names.bin';
  static const _constellationsAsset =
      'packages/unisolver_flutter/assets/unisolver_constellations.bin';

  static Future<({String dbPath, String dsoPath})> ensureInstalled() async {
    final dir = await getApplicationSupportDirectory();
    final dbPath = '${dir.path}/unisolver_10_80.db';
    final dsoPath = '${dir.path}/unisolver_dso.bin';

    final dbFile = File(dbPath);
    // Missing, empty or another format (a database left by an earlier release): install. The
    // decompressor keeps an existing non-empty file, so an old one must go first.
    if (!isFormat2File(dbFile)) {
      if (await dbFile.exists()) await dbFile.delete();
      final zstBytes = await rootBundle.load(_dbAsset);
      final zstFile = File('${dir.path}/unisolver_10_80.db.zst');
      await zstFile.writeAsBytes(zstBytes.buffer.asUint8List(), flush: true);
      await rust.installCompressedDb(zstPath: zstFile.path, outPath: dbPath);
      await zstFile.delete();
    }
    await _copyIfChanged(_dsoAsset, dsoPath);
    return (dbPath: dbPath, dsoPath: dsoPath);
  }

  /// Installs the bundled constellation pack (IAU figures and boundaries, CC BY-SA 4.0: show
  /// `dataAttributions()`) and returns its path, for `annotator(constellationsPath: ...)`.
  static Future<String> installConstellations() async {
    final dir = await getApplicationSupportDirectory();
    final path = '${dir.path}/unisolver_constellations.bin';
    await _copyIfChanged(_constellationsAsset, path);
    return path;
  }

  /// Installs the names pack from the app's own assets and returns its path, or null
  /// when the app did not declare the optional asset (see the class docs).
  static Future<String?> installNames() async {
    final dir = await getApplicationSupportDirectory();
    final path = '${dir.path}/unisolver_names.bin';
    try {
      await _copyIfChanged(_namesAsset, path);
    } on FlutterError {
      return null; // not declared by the app
    }
    return path;
  }

  /// Installs a database archive the **app** bundles in its own assets (declared in the app's
  /// pubspec, e.g. `assets/unisolver_5_10.db.zst` taken from the data release) into [manager]'s
  /// directory, checked against [manifestJson]: the manifest bundled with the same build, so the
  /// two always agree. Idempotent: returns null when that tier is already installed.
  static Future<DbImport?> importBundled(
    DbManager manager,
    String assetKey, {
    required String manifestJson,
  }) async {
    final m = DbManifest.parse(manifestJson);
    final file = assetKey.split('/').last;
    for (final t in m.tiers) {
      if (t.file == file && manager.isInstalled(t)) return null;
    }
    final data = await rootBundle.load(assetKey);
    Directory(manager.dir).createSync(recursive: true);
    final tmp = File('${manager.dir}/$file.bundled');
    await tmp.writeAsBytes(
      data.buffer.asUint8List(data.offsetInBytes, data.lengthInBytes),
      flush: true,
    );
    try {
      return await manager.importFile(tmp.path, manifest: m);
    } finally {
      if (tmp.existsSync()) tmp.deleteSync();
    }
  }

  /// Small assets are copied. **Lengths are compared on every launch**: when an asset
  /// changes in an update, checking only for existence would keep old data forever.
  static Future<void> _copyIfChanged(String asset, String path) async {
    final b = await rootBundle.load(asset);
    final f = File(path);
    if (!await f.exists() || (await f.length()) != b.lengthInBytes) {
      await f.writeAsBytes(b.buffer.asUint8List(), flush: true);
    }
  }

}
