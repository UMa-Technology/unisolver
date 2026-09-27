import 'dart:io';

import 'package:flutter/material.dart';
import 'package:unisolver_flutter/unisolver_flutter.dart';

/// Tier manager page: fetch the manifest → install / resume / delete → register with the pool.
///
/// The base URL is editable so the whole host contract (resume and sha256 refusal included)
/// can be rehearsed against any static server, e.g. `scripts/ci/serve_tiers.py`.
class DbPage extends StatefulWidget {
  const DbPage({
    super.key,
    required this.pool,
    required this.dir,
    this.onNamesInstalled,
  });

  /// Called with the local path after the names pack is downloaded
  final Future<void> Function(String path)? onNamesInstalled;

  final UniSolverPool pool;

  /// Where tiers are installed (= the directory the pool scans)
  final String dir;

  @override
  State<DbPage> createState() => _DbPageState();
}

class _DbPageState extends State<DbPage> {
  // Narrow-field tiers are not distributed with the open-source build: point this
  // at your own static host (or scripts/ci/serve_tiers.py for a local rehearsal).
  late final TextEditingController _base = TextEditingController();
  DbManifest? _manifest;
  String? _error;
  bool _loading = false;

  /// Tiers being installed → progress
  final Map<String, DbProgress> _progress = {};
  final Map<String, DbCancel> _cancels = {};
  List<TierInfoDto> _registered = const [];

  DbManager _manager() => DbManager(
    dir: widget.dir,
    baseUrl: _base.text.trim(),
    register: (path) async {
      await widget.pool.register(dbPath: path);
      await _refreshRegistered();
    },
  );

  @override
  void initState() {
    super.initState();
    // Use the cached manifest first (works offline), then refresh in the background
    _manifest = _manager().cachedManifest();
    _refreshRegistered();
    if (_base.text.trim().isNotEmpty) _fetch();
  }

  @override
  void dispose() {
    _base.dispose();
    super.dispose();
  }

  Future<void> _refreshRegistered() async {
    final t = await widget.pool.tiers();
    if (mounted) setState(() => _registered = t);
  }

  Future<void> _fetch() async {
    if (_base.text.trim().isEmpty) {
      setState(() => _error = 'Enter the base URL of a manifest host first');
      return;
    }
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final m = await _manager().fetchManifest();
      if (mounted) setState(() => _manifest = m);
    } catch (e) {
      if (mounted) setState(() => _error = '$e');
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  Future<void> _install(DbTier t, {bool allowNonMobile = false}) async {
    final mgr = _manager();
    final cancel = DbCancel();
    setState(() {
      _cancels[t.name] = cancel;
      _error = null;
    });
    try {
      await mgr.install(
        t,
        cancel: cancel,
        allowNonMobile: allowNonMobile,
        onProgress: (p) {
          if (mounted) setState(() => _progress[t.name] = p);
        },
      );
    } catch (e) {
      if (mounted) setState(() => _error = '${t.name}: $e');
    } finally {
      if (mounted) {
        setState(() {
          _progress.remove(t.name);
          _cancels.remove(t.name);
        });
      }
    }
  }

  Future<void> _installAsset(DbAsset a) async {
    final mgr = _manager();
    final cancel = DbCancel();
    setState(() {
      _cancels[a.name] = cancel;
      _error = null;
    });
    try {
      final path = await mgr.installAsset(
        a,
        cancel: cancel,
        onProgress: (p) {
          if (mounted) setState(() => _progress[a.name] = p);
        },
      );
      if (a.kind == 'names') await widget.onNamesInstalled?.call(path);
    } catch (e) {
      if (mounted) setState(() => _error = '${a.name}: $e');
    } finally {
      if (mounted) {
        setState(() {
          _progress.remove(a.name);
          _cancels.remove(a.name);
        });
      }
    }
  }

  Future<void> _remove(DbTier t) async {
    await _manager().remove(t);
    // A registered tier cannot leave the pool (the handle still holds its mmap); say it frees on restart
    if (mounted) {
      setState(() {});
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text(
            '${t.name} deleted; a registered handle is released on the next app start',
          ),
        ),
      );
    }
  }

  String _mb(int bytes) =>
      '${(bytes / 1e6).toStringAsFixed(bytes > 1e9 ? 1 : 0)} MB';

  @override
  Widget build(BuildContext context) {
    final mgr = _manager();
    final m = _manifest;
    return ListView(
      padding: const EdgeInsets.all(12),
      children: [
        Row(
          children: [
            Expanded(
              child: TextField(
                controller: _base,
                decoration: const InputDecoration(
                  labelText: 'Manifest base URL',
                  isDense: true,
                  helperText: 'For a local rehearsal: http://127.0.0.1:8099/ (scripts/ci/serve_tiers.py)',
                ),
              ),
            ),
            const SizedBox(width: 8),
            FilledButton(
              onPressed: _loading ? null : _fetch,
              child: Text(_loading ? 'Fetching…' : 'Fetch manifest'),
            ),
          ],
        ),
        if (_error != null)
          Padding(
            padding: const EdgeInsets.only(top: 8),
            child: Text(
              _error!,
              style: const TextStyle(color: Colors.redAccent, fontSize: 12),
            ),
          ),
        const SizedBox(height: 8),
        if (m == null)
          const Padding(
            padding: EdgeInsets.symmetric(vertical: 24),
            child: Text(
              'No manifest yet: enter a base URL and fetch (offline, the last cached manifest is used)',
            ),
          )
        else
          ...m.tiers.map((t) => _tierCard(mgr, t)),
        if (m != null && m.assets.isNotEmpty) ...[
          const SizedBox(height: 8),
          Text(
            'Optional assets',
            style: Theme.of(context).textTheme.titleSmall,
          ),
          ...m.assets.map((a) => _assetCard(mgr, a)),
        ],
        const Divider(height: 32),
        Text(
          'Registered tiers: ${_registered.length}',
          style: Theme.of(context).textTheme.titleSmall,
        ),
        ..._registered.map(
          (t) => Text(
            '  ${t.name}  ${t.minFovDeg.toStringAsFixed(1)}–${t.maxFovDeg.toStringAsFixed(1)}°  '
            '${t.numStars} stars / ${t.numPatterns} patterns',
            style: const TextStyle(fontSize: 12, color: Colors.grey),
          ),
        ),
        if (widget.pool.skipped().isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(top: 8),
            child: Text(
              'Skipped: ${widget.pool.skipped().join('; ')}',
              style: const TextStyle(fontSize: 11, color: Colors.orangeAccent),
            ),
          ),
      ],
    );
  }

  Widget _assetCard(DbManager mgr, DbAsset a) {
    final installed = mgr.isAssetInstalled(a);
    final prog = _progress[a.name];
    return Card(
      child: ListTile(
        title: Text(a.name),
        subtitle: Text(
          '${a.kind} | ${_mb(a.bytes)}'
          '${a.license == null ? '' : ' | ${a.license}'}'
          '${a.attribution == null ? '' : '\n${a.attribution}'}',
          style: const TextStyle(fontSize: 12),
        ),
        trailing: installed
            ? const _Tag('installed', Colors.green)
            : prog != null
            ? const SizedBox(
                width: 20,
                height: 20,
                child: CircularProgressIndicator(strokeWidth: 2),
              )
            : FilledButton(
                onPressed: () => _installAsset(a),
                child: const Text('Install'),
              ),
      ),
    );
  }

  Widget _tierCard(DbManager mgr, DbTier t) {
    final installed = mgr.isInstalled(t);
    final partial = mgr.partialBytes(t);
    final prog = _progress[t.name];
    final cancel = _cancels[t.name];
    final blockedOnMobile = (Platform.isAndroid || Platform.isIOS) && !t.mobile;

    return Card(
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Expanded(
                  child: Text(
                    '${t.name}   ${t.fovLabel}',
                    style: const TextStyle(fontWeight: FontWeight.bold),
                  ),
                ),
                if (t.bundled) const _Tag('bundled', Colors.blueGrey),
                if (installed) const _Tag('installed', Colors.green),
                if (blockedOnMobile)
                  const _Tag('not for mobile', Colors.orange),
              ],
            ),
            const SizedBox(height: 4),
            Text(
              'download ${_mb(t.bytes)} | peak disk ${_mb(t.diskBytesNeeded)}'
              '${t.numStars == null ? '' : ' | ${t.numStars} stars'}'
              '${t.starMaxMagnitude == null ? '' : ' | to mag ${t.starMaxMagnitude}'}',
              style: const TextStyle(fontSize: 12, color: Colors.grey),
            ),
            if (prog != null) ...[
              const SizedBox(height: 8),
              LinearProgressIndicator(
                value: prog.phase == DbPhase.downloading ? prog.fraction : null,
              ),
              const SizedBox(height: 4),
              Text(switch (prog.phase) {
                DbPhase.downloading =>
                  'downloading ${_mb(prog.received)} / ${_mb(prog.total)} (${(prog.fraction * 100).toStringAsFixed(0)}%)',
                DbPhase.verifying => 'verifying sha256…',
                DbPhase.decompressing => 'decompressing…',
                DbPhase.registering => 'registering with the pool…',
                DbPhase.done => 'done',
              }, style: const TextStyle(fontSize: 12)),
            ] else if (partial > 0 && !installed)
              Padding(
                padding: const EdgeInsets.only(top: 4),
                child: Text(
                  '${_mb(partial)} downloaded, resumable',
                  style: const TextStyle(fontSize: 12, color: Colors.amber),
                ),
              ),
            const SizedBox(height: 4),
            Row(
              mainAxisAlignment: MainAxisAlignment.end,
              children: [
                if (cancel != null)
                  TextButton(
                    onPressed: cancel.cancel,
                    child: const Text('Cancel'),
                  )
                else if (installed)
                  TextButton(
                    onPressed: () => _remove(t),
                    child: const Text('Delete'),
                  )
                else if (t.bundled)
                  const Text(
                    'installed from the plugin assets',
                    style: TextStyle(fontSize: 12, color: Colors.grey),
                  )
                else
                  FilledButton.tonal(
                    onPressed: () =>
                        _install(t, allowNonMobile: blockedOnMobile),
                    child: Text(partial > 0 ? 'Resume' : 'Install'),
                  ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

class _Tag extends StatelessWidget {
  const _Tag(this.text, this.color);
  final String text;
  final Color color;

  @override
  Widget build(BuildContext context) => Container(
    margin: const EdgeInsets.only(left: 6),
    padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
    decoration: BoxDecoration(
      color: color.withValues(alpha: 0.25),
      borderRadius: BorderRadius.circular(4),
    ),
    child: Text(text, style: TextStyle(fontSize: 11, color: color)),
  );
}
