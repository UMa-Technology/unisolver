/// Dart model of the tier manifest (`manifest.json`, version 2).
///
/// The manifest is the single entry point of distribution: clients read it, then fetch tiers
/// by `key`. Fields match the manifest generator one to one; change both together.
library;

import 'dart:convert';

/// What [DbManager] needs to download and verify an item: tiers and assets alike.
abstract interface class DbItem {
  String get name;

  /// File name on the host (and the stem of the local file)
  String get file;

  /// Content-addressed path on the host; null when the item is not hosted
  String? get key;

  /// Download size in bytes
  int get bytes;

  /// sha256 of the downloaded file (verified after every download)
  String get sha256;
}

/// One tier in the manifest.
class DbTier implements DbItem {
  const DbTier({
    required this.name,
    required this.file,
    required this.minFovDeg,
    required this.maxFovDeg,
    required this.bytes,
    required this.sha256,
    required this.mobile,
    required this.bundled,
    this.key,
    this.rawBytes,
    this.format,
    this.minEngine,
    this.numStars,
    this.numPatterns,
    this.starMaxMagnitude,
    this.license,
    this.attribution,
  });

  /// Tier name = database file name without extension (`unisolver_10_80`), also the installed file's stem
  @override
  final String name;

  /// Archive file name (`unisolver_10_80.db.zst`)
  @override
  final String file;

  /// Content-addressed download path (`db/<last 8 hex of sha256>/<file>`). **Absent for the bundled tier**
  @override
  final String? key;

  final double minFovDeg;
  final double maxFovDeg;

  /// Archive size in bytes (the download)
  @override
  final int bytes;

  /// Decompressed size in bytes (disk use); may be missing
  final int? rawBytes;

  /// sha256 of the archive (verify it after every download)
  @override
  final String sha256;

  /// Whether mobile devices should use it (min_fov ≥ 2.5°: phones have no narrower fields,
  /// and deeper tiers are too large to keep resident on mobile)
  final bool mobile;

  /// Ships with the plugin assets, not on the host (the wide tier). Clients must not download it
  final bool bundled;

  final String? format;
  final String? minEngine;
  final int? numStars;
  final int? numPatterns;
  final double? starMaxMagnitude;

  /// SPDX-style license of the database (Gaia-derived tiers are CC-BY-SA-3.0-IGO); may be missing
  final String? license;

  /// Attribution text the license asks for; may be missing
  final String? attribution;

  /// Peak disk use during download and decompression (archive and database coexist)
  int get diskBytesNeeded => bytes + (rawBytes ?? (bytes * 5) ~/ 2);

  String get fovLabel => '${_fov(minFovDeg)}–${_fov(maxFovDeg)}°';

  static String _fov(double v) =>
      v == v.roundToDouble() ? v.toStringAsFixed(0) : v.toString();

  factory DbTier.fromJson(Map<String, dynamic> j) => DbTier(
        name: j['name'] as String,
        file: j['file'] as String,
        key: j['key'] as String?,
        minFovDeg: (j['min_fov_deg'] as num).toDouble(),
        maxFovDeg: (j['max_fov_deg'] as num).toDouble(),
        bytes: (j['bytes'] as num).toInt(),
        rawBytes: (j['raw_bytes'] as num?)?.toInt(),
        sha256: (j['sha256'] as String).toLowerCase(),
        // A missing field means "not for mobile": better to let the user opt in than to silently
        // install a tier a phone cannot hold
        mobile: j['mobile'] as bool? ?? false,
        bundled: j['bundled'] as bool? ?? false,
        format: j['format'] as String?,
        minEngine: j['min_engine'] as String?,
        numStars: (j['num_stars'] as num?)?.toInt(),
        numPatterns: (j['num_patterns'] as num?)?.toInt(),
        starMaxMagnitude: (j['star_max_magnitude'] as num?)?.toDouble(),
        license: j['license'] as String?,
        attribution: j['attribution'] as String?,
      );
}

/// A manifest.
class DbManifest {
  const DbManifest({
    required this.version,
    required this.baseUrl,
    required this.tiers,
    this.assets = const [],
  });

  final int version;

  /// Host prefix; download URL = baseUrl + tier.key
  final String baseUrl;

  /// Wide to narrow
  final List<DbTier> tiers;

  /// Optional downloadable assets (the multilingual names pack); empty in older manifests
  final List<DbAsset> assets;

  /// Highest supported manifest version. A newer one may change the meaning of `key`, so ask
  /// for an engine upgrade instead of guessing.
  static const int supportedVersion = 2;

  DbTier? byName(String name) =>
      tiers.where((t) => t.name == name).firstOrNull;

  DbAsset? assetByName(String name) =>
      assets.where((a) => a.name == name).firstOrNull;

  /// Tiers covering this FOV (with the solver's [0.8×min, 1.25×max] tolerance)
  List<DbTier> covering(double fovDeg) => tiers
      .where((t) => fovDeg >= t.minFovDeg * 0.8 && fovDeg <= t.maxFovDeg * 1.25)
      .toList();

  factory DbManifest.parse(String jsonText) {
    final j = json.decode(jsonText) as Map<String, dynamic>;
    final v = (j['version'] as num?)?.toInt() ?? 0;
    if (v > supportedVersion) {
      throw DbManifestException(
        'manifest version $v is newer than this engine supports '
        '($supportedVersion) — upgrade unisolver_flutter',
      );
    }
    final tiers = ((j['tiers'] as List?) ?? const [])
        .map((e) => DbTier.fromJson(e as Map<String, dynamic>))
        .toList()
      ..sort((a, b) => b.minFovDeg.compareTo(a.minFovDeg));
    final assets = ((j['assets'] as List?) ?? const [])
        .map((e) => DbAsset.fromJson(e as Map<String, dynamic>))
        .toList();
    return DbManifest(
      version: v,
      baseUrl: (j['base_url'] as String?) ?? '',
      tiers: tiers,
      assets: assets,
    );
  }
}

/// An optional downloadable file that is not a star database (the names pack).
class DbAsset implements DbItem {
  const DbAsset({
    required this.name,
    required this.kind,
    required this.file,
    required this.bytes,
    required this.sha256,
    this.key,
    this.license,
    this.attribution,
  });

  @override
  final String name;

  /// What it is (`names` for the multilingual names pack)
  final String kind;

  @override
  final String file;

  @override
  final String? key;

  @override
  final int bytes;

  @override
  final String sha256;

  /// SPDX-style license of the file (the names pack is GPL-2.0-or-later)
  final String? license;

  /// Attribution text its license asks for
  final String? attribution;

  factory DbAsset.fromJson(Map<String, dynamic> j) => DbAsset(
        name: j['name'] as String,
        kind: (j['kind'] as String?) ?? '',
        file: j['file'] as String,
        key: j['key'] as String?,
        bytes: (j['bytes'] as num).toInt(),
        sha256: (j['sha256'] as String).toLowerCase(),
        license: j['license'] as String?,
        attribution: j['attribution'] as String?,
      );
}

class DbManifestException implements Exception {
  DbManifestException(this.message);
  final String message;
  @override
  String toString() => 'DbManifestException: $message';
}

extension _FirstOrNull<T> on Iterable<T> {
  T? get firstOrNull => isEmpty ? null : first;
}
