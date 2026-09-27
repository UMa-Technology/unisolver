import 'dart:io';

import 'package:camera/camera.dart';
import 'package:flutter/material.dart';
import 'package:unisolver_flutter/unisolver_flutter.dart';

import 'fov_presets.dart';
import 'frame_convert.dart';

/// Live tracking (a demonstration of the frame pipeline).
///
/// Known limitation: Flutter's camera plugin does not expose a manual exposure time
/// (SENSOR_EXPOSURE_TIME), so under a night sky the preview stream is often underexposed and
/// this page only demonstrates the pipeline and tracking cadence. A long-exposure still-frame
/// loop needs the host app to control exposure through a platform channel and feed the frames
/// to the same solveFrame API.
class LivePage extends StatefulWidget {
  const LivePage({super.key, required this.solver});
  final UniSolver solver;

  @override
  State<LivePage> createState() => _LivePageState();
}

class _LivePageState extends State<LivePage> {
  CameraController? _controller;
  String _status = 'Not started';
  bool _inFlight = false;
  ({F32Array4 quat, double fov})? _hint;
  int _frames = 0;
  int _solved = 0;

  bool get _supported => Platform.isAndroid || Platform.isIOS;

  Future<void> _start() async {
    try {
      final cams = await availableCameras();
      if (cams.isEmpty) {
        setState(() => _status = 'No camera available');
        return;
      }
      final c = CameraController(
        cams.first,
        ResolutionPreset.high,
        enableAudio: false,
        imageFormatGroup: ImageFormatGroup.yuv420,
      );
      await c.initialize();
      await c.startImageStream(_onFrame);
      setState(() {
        _controller = c;
        _status = 'Previewing… (the first solve walks the FOV ladder)';
      });
    } catch (e) {
      setState(() => _status = 'Camera failed to start: $e');
    }
  }

  Future<void> _onFrame(CameraImage img) async {
    // Drop frames while a solve is in flight; never queue
    if (_inFlight) return;
    _inFlight = true;
    _frames++;
    try {
      final y = img.planes[0];
      final frame = yPlaneToFrameDto(
        width: img.width,
        height: img.height,
        bytesPerRow: y.bytesPerRow,
        bytes: y.bytes,
      );
      final hint = _hint;
      final base = SolveOptionsDto.defaults(fovEstimateDeg: hint?.fov ?? 70);
      SolveOutcomeDto out;
      if (hint == null) {
        // First solve: blind, rung by rung (the stream's FOV is unknown)
        out = (await _ladderSolve(frame)).outcome;
      } else {
        out = await widget.solver.solveFrame(
          frame: frame,
          opts: SolveOptionsDto(
            fovEstimateDeg: hint.fov,
            fovMaxErrorDeg: null,
            camera: null,
            attitudeHintWxyz: hint.quat,
            hintUncertaintyDeg: 3,
            strictHint: false,
            profile: const ExtractionProfileDto.customFast(
              sigma: 10,
              maxCentroids: 60,
            ),
            retryAlternateProfile: false,
            thorough: false,
            matchThreshold: base.matchThreshold,
            timeoutMs: BigInt.from(1500),
            observationUnixMs: DateTime.now().millisecondsSinceEpoch,
          ),
        );
        if (out.status != SolveStatusDto.ok) {
          // Fast extraction is weak on dark frames → retry with CCL and the hint (the strategy validated in solvecli)
          out = await widget.solver.solveFrame(
            frame: frame,
            opts: SolveOptionsDto(
              fovEstimateDeg: hint.fov,
              fovMaxErrorDeg: null,
              camera: null,
              attitudeHintWxyz: hint.quat,
              hintUncertaintyDeg: 3,
              strictHint: false,
              profile: const ExtractionProfileDto.phoneJpeg(),
              retryAlternateProfile: false,
              thorough: false,
              matchThreshold: base.matchThreshold,
              timeoutMs: BigInt.from(2000),
              observationUnixMs: DateTime.now().millisecondsSinceEpoch,
            ),
          );
        }
      }
      final g = out.solution;
      if (g != null) {
        _solved++;
        _hint = (quat: g.quatIcrs2CamWxyz, fov: g.fovDeg);
        setState(
          () => _status =
              'ra=${g.raDeg.toStringAsFixed(2)}° dec=${g.decDeg.toStringAsFixed(2)}° '
              'fov=${g.fovDeg.toStringAsFixed(1)}° ${g.numMatches} stars '
              '${out.timing.solveMs.toStringAsFixed(0)} ms  [$_solved/$_frames frames]',
        );
      } else {
        setState(
          () => _status =
              '${out.status.name} (${out.centroids.length} centroids): too few stars or too short an exposure [$_solved/$_frames]',
        );
        // Keep the previous hint and keep tracking
        if (hint != null) _hint = hint;
      }
    } catch (e) {
      setState(() => _status = 'Solve error: $e');
    } finally {
      _inFlight = false;
    }
  }

  Future<LadderOutcomeDto> _ladderSolve(FrameDto frame) {
    return widget.solver
        .solveFrame(
          frame: frame,
          opts: SolveOptionsDto.defaults(fovEstimateDeg: 70),
        )
        .then((first) async {
          if (first.solution != null) {
            return LadderOutcomeDto(outcome: first, attempts: const []);
          }
          // Rung-by-rung retry (solveFrame has no ladder entry, so walk it here)
          for (final p in ladderFor(frame.width, frame.height).skip(1)) {
            final base = SolveOptionsDto.defaults(fovEstimateDeg: p.fovDeg);
            final out = await widget.solver.solveFrame(
              frame: frame,
              opts: SolveOptionsDto(
                fovEstimateDeg: p.fovDeg,
                fovMaxErrorDeg: p.maxErrorDeg,
                camera: null,
                attitudeHintWxyz: null,
                hintUncertaintyDeg: base.hintUncertaintyDeg,
                strictHint: false,
                profile: const ExtractionProfileDto.phoneJpeg(),
                retryAlternateProfile: true,
                thorough: false,
                matchThreshold: base.matchThreshold,
                timeoutMs: BigInt.from(3000),
                observationUnixMs: null,
              ),
            );
            if (out.solution != null) {
              return LadderOutcomeDto(outcome: out, attempts: const []);
            }
          }
          return LadderOutcomeDto(outcome: first, attempts: const []);
        });
  }

  @override
  void dispose() {
    _controller?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    if (!_supported) {
      return const Center(
        child: Text('Live tracking needs a mobile device (iOS/Android)'),
      );
    }
    final c = _controller;
    return Column(
      children: [
        Expanded(
          child: c == null
              ? Center(
                  child: FilledButton(
                    onPressed: _start,
                    child: const Text('Start camera'),
                  ),
                )
              : CameraPreview(c),
        ),
        Padding(
          padding: const EdgeInsets.all(8),
          child: Text(_status, style: const TextStyle(fontSize: 13)),
        ),
      ],
    );
  }
}
