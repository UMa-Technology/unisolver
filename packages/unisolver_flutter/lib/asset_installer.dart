import 'dart:io';

import 'package:flutter/foundation.dart' show FlutterError;
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';

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
    if (await _needsInstall(dbFile)) {
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

  /// Small assets are copied. **Lengths are compared on every launch**: when an asset
  /// changes in an update, checking only for existence would keep old data forever.
  static Future<void> _copyIfChanged(String asset, String path) async {
    final b = await rootBundle.load(asset);
    final f = File(path);
    if (!await f.exists() || (await f.length()) != b.lengthInBytes) {
      await f.writeAsBytes(b.buffer.asUint8List(), flush: true);
    }
  }

  /// Missing, empty or old-format (no UNISOLV2 magic) files trigger a reinstall: v1 postcard
  /// databases work but stay fully resident, so upgrades move them to the mmap-friendly v2.
  static Future<bool> _needsInstall(File f) async {
    if (!await f.exists() || (await f.length()) < 8) return true;
    final raf = await f.open();
    try {
      final magic = await raf.read(8);
      return String.fromCharCodes(magic) != 'UNISOLV2';
    } finally {
      await raf.close();
    }
  }
}
