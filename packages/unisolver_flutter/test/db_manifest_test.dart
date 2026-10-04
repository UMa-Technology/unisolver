import 'package:flutter_test/flutter_test.dart';
import 'package:unisolver_flutter/unisolver_flutter.dart';

const _manifest = '''
{"version": 3, "base_url": "https://host/", "tiers": [
  {"name": "t", "file": "t.db.zst", "key": "db/1/t.db.zst", "min_fov_deg": 1, "max_fov_deg": 2,
   "bytes": 10, "sha256": "AA", "raw_bytes": 20, "raw_sha256": "BB", "mobile": false}],
 "packages": [{"name": "np", "kind": "blind-index", "min_fov_deg": 0.18, "max_fov_deg": 3.1,
   "mobile": false, "min_engine": "0.5.0", "builder": "starmatch-1", "num_patterns": 5,
   "num_stars": 6, "license": "L", "attribution": "A", "files": [
   {"role": "index", "file": "np.idx.zst", "key": "pkg/1/np.idx.zst", "bytes": 100,
    "sha256": "CC", "raw_bytes": 300, "raw_sha256": "DD"},
   {"role": "stars", "file": "np.stars.zst", "key": "pkg/2/np.stars.zst", "bytes": 200,
    "sha256": "EE", "raw_bytes": 250, "raw_sha256": "FF"}]}]}
''';

void main() {
  test('packages and decompressed digests are read', () {
    final m = DbManifest.parse(_manifest);
    expect(m.tiers.single.rawSha256, 'bb');
    final p = m.packageByName('np')!;
    expect(p.index.localFile, 'np.idx');
    expect(p.stars.name, 'np/stars');
    expect(p.stars.sha256, 'ee');
    expect(p.stars.rawSha256, 'ff');
    expect(
      (p.downloadBytes, p.installedBytes, p.diskBytesNeeded),
      (300, 550, 750),
    );
    expect(p.mobile, isFalse);
    expect(p.minEngine, '0.5.0');
    expect(p.fovLabel, '0.18–3.1°');
    expect(m.packageByName('nope'), isNull);
  });

  test('a manifest without packages still parses', () {
    final m = DbManifest.parse('{"version": 3, "tiers": []}');
    expect(m.packages, isEmpty);
  });

  test('engine versions compare by number', () {
    expect(engineSatisfies('0.5.0', '0.5.0'), isTrue);
    expect(engineSatisfies('0.10.0', '0.9.3'), isTrue);
    expect(engineSatisfies('1.0.0', '0.9.9'), isTrue);
    expect(engineSatisfies('0.4.4', '0.5.0'), isFalse);
    expect(engineSatisfies('0.5.0', null), isTrue);
    expect(engineSatisfies('0.5.0', 'x'), isTrue);
  });
}
