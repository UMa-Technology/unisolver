import 'dart:io';

import 'package:flutter/material.dart';
import 'package:image_picker/image_picker.dart';
import 'package:unisolver_flutter/unisolver_flutter.dart';

import 'camera_store.dart';

/// Calibration flow: pick 3–6 photos from the same camera and orientation → solve each into the
/// session → radial fit → show the residual improvement → save the camera (JSON, SharedPreferences).
class CalibratePage extends StatefulWidget {
  const CalibratePage({super.key, required this.solver});
  final UniSolver solver;

  @override
  State<CalibratePage> createState() => _CalibratePageState();
}

class _CalibratePageState extends State<CalibratePage> {
  UniCalibration? _session;
  final List<String> _frameLog = [];
  CalibrationReportDto? _report;
  String? _savedJson;
  bool _busy = false;

  @override
  void initState() {
    super.initState();
    CameraStore.load().then((j) => setState(() => _savedJson = j));
  }

  Future<void> _addFrames() async {
    final picked = await ImagePicker().pickMultiImage();
    if (picked.isEmpty) return;
    setState(() => _busy = true);
    try {
      final session = _session ?? await widget.solver.newCalibration();
      _session = session;
      for (final x in picked) {
        final opts = SolveOptionsDto.defaults(fovEstimateDeg: 70);
        final out = await session.addImageFile(
          path: x.path,
          opts: SolveOptionsDto(
            fovEstimateDeg: opts.fovEstimateDeg,
            fovMaxErrorDeg: 9,
            camera: null,
            attitudeHintWxyz: null,
            hintUncertaintyDeg: opts.hintUncertaintyDeg,
            strictHint: false,
            profile: const ExtractionProfileDto.phoneJpeg(),
            retryAlternateProfile: true,
            thorough: false,
            matchThreshold: opts.matchThreshold,
            timeoutMs: BigInt.from(6000),
            observationUnixMs: null,
          ),
        );
        _frameLog.add(
          '${File(x.path).uri.pathSegments.last}: ${out.status.name} (${session.count()} frames in the session)',
        );
        setState(() {});
      }
    } catch (e) {
      _frameLog.add('Error: $e');
    } finally {
      setState(() => _busy = false);
    }
  }

  Future<void> _fit() async {
    final session = _session;
    if (session == null) return;
    setState(() => _busy = true);
    try {
      final rep = await session.fit(model: const CalibModelDto.radial());
      setState(() => _report = rep);
    } catch (e) {
      setState(() => _frameLog.add('Fit failed: $e'));
    } finally {
      setState(() => _busy = false);
    }
  }

  Future<void> _save() async {
    final rep = _report;
    if (rep == null) return;
    final j = await cameraParamsToJson(c: rep.camera);
    await CameraStore.save(j);
    setState(() => _savedJson = j);
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(
          content: Text('Camera saved; the Solve page will skip the FOV sweep'),
        ),
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    final rep = _report;
    return ListView(
      padding: const EdgeInsets.all(16),
      children: [
        Text(
          'Shoot 3–6 frames of different sky regions (same phone, same orientation); one calibration lasts.',
          style: Theme.of(context).textTheme.bodyMedium,
        ),
        const SizedBox(height: 8),
        if (_savedJson != null)
          Card(
            child: ListTile(
              leading: const Icon(Icons.verified, color: Colors.green),
              title: const Text('Calibrated camera saved'),
              subtitle: Text(
                _savedJson!.substring(0, 60.clamp(0, _savedJson!.length)),
              ),
              trailing: TextButton(
                onPressed: () async {
                  await CameraStore.clear();
                  setState(() => _savedJson = null);
                },
                child: const Text('Clear'),
              ),
            ),
          ),
        Row(
          children: [
            FilledButton(
              onPressed: _busy ? null : _addFrames,
              child: const Text('Pick calibration photos'),
            ),
            const SizedBox(width: 12),
            FilledButton.tonal(
              onPressed: _busy || (_session?.count() ?? 0) < 2 ? null : _fit,
              child: const Text('Fit radial distortion'),
            ),
          ],
        ),
        const SizedBox(height: 8),
        ..._frameLog.map((l) => Text(l, style: const TextStyle(fontSize: 12))),
        if (rep != null) ...[
          const Divider(),
          Text(
            'Residual: ${rep.rmseBeforePx.toStringAsFixed(2)} px → '
            '${rep.rmseAfterPx.toStringAsFixed(2)} px'
            ' (${rep.nInliers} inliers, ${rep.nOutliers} rejected, ${rep.framesUsed} frames)',
          ),
          Text(
            'Focal length ${rep.camera.focalLengthPx.toStringAsFixed(1)} px',
          ),
          const SizedBox(height: 8),
          FilledButton(
            onPressed: _save,
            child: const Text('Save as default camera'),
          ),
        ],
      ],
    );
  }
}
