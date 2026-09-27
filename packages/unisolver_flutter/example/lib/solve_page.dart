import 'dart:io';
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:image_picker/image_picker.dart';
import 'package:path_provider/path_provider.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:unisolver_flutter/unisolver_flutter.dart';

import 'overlay_painter.dart';

class SolvePage extends StatefulWidget {
  const SolvePage({
    super.key,
    required this.pool,
    required this.dsoPath,
    required this.namesPath,
    required this.constellationsPath,
    required this.language,
  });

  /// Tier pool: **no database named by hand**; routing and cross-tier fallback happen in the engine (the recommended integration)
  final UniSolverPool pool;
  final String dsoPath;

  /// Multilingual names pack (13 languages); null when the app has none (English names)
  final String? namesPath;

  /// Constellation pack (figures and IAU boundaries); null draws neither
  final String? constellationsPath;

  /// Current annotation language code (`zh_cn` / `ja` / …)
  final String language;

  @override
  State<SolvePage> createState() => _SolvePageState();
}

class _SolvePageState extends State<SolvePage> {
  ui.Image? _decoded;
  SolveOutcomeDto? _outcome;
  AnnotationsDto? _annotations;

  /// Cross-tier attempts for display ("46°@unisolver_10_80→Ok")
  List<String> _attempts = const [];
  String? _solvedBy;

  /// Annotators are **cached per tier**: building one reads and parses the DSO catalog and
  /// names pack (about 1.6 ms) while annotating a frame takes 0.65 ms, so rebuilding per solve
  /// triples the cost. Language is an annotate() argument, so one annotator serves all languages.
  final Map<String, UniAnnotator> _annotators = {};

  Future<UniAnnotator> _annotatorFor(String? db) async {
    final key = db ?? '';
    final cached = _annotators[key];
    if (cached != null) return cached;
    final made = await widget.pool.annotator(
      db: db,
      dsoPath: widget.dsoPath,
      namesPath: widget.namesPath,
      constellationsPath: widget.constellationsPath,
    );
    _annotators[key] = made;
    return made;
  }

  @override
  void didUpdateWidget(SolvePage old) {
    super.didUpdateWidget(old);
    // A names pack installed later needs new annotators
    if (old.namesPath != widget.namesPath) _dropAnnotators();
    if (old.language != widget.language || old.namesPath != widget.namesPath) {
      _annotate();
    }
  }

  void _dropAnnotators() {
    for (final a in _annotators.values) {
      a.dispose();
    }
    _annotators.clear();
  }

  @override
  void dispose() {
    _dropAnnotators();
    _viewer.dispose();
    super.dispose();
  }

  String? _status;
  bool _busy = false;
  bool _usedCalibratedCamera = false;

  Future<void> _loadDecoded(File f) async {
    final bytes = await f.readAsBytes();
    final codec = await ui.instantiateImageCodec(bytes);
    final frame = await codec.getNextFrame();
    setState(() => _decoded = frame.image);
  }

  Future<void> _solve(File imgFile) async {
    setState(() {
      _busy = true;
      _status = 'Solving…';
      _outcome = null;
      _annotations = null;
      _attempts = const [];
    });
    await _loadDecoded(imgFile);
    try {
      final sw = Stopwatch()..start();
      final prefs = await SharedPreferences.getInstance();
      final savedCamera = prefs.getString('unisolver.camera');

      // Both cases use the pool's automatic entry: calibrated, the camera goes into opts and the
      // pool picks tiers by its FOV without a ladder; otherwise header hints + aspect ladder, plus
      // a range sweep for tiers never reached.
      final opts = SolveOptionsDto.defaults(fovEstimateDeg: 70);
      SolveOptionsDto base = opts;
      if (savedCamera != null) {
        final cam = await cameraParamsFromJson(j: savedCamera);
        base = SolveOptionsDto(
          // Horizontal FOV from the focal length (the pool picks tiers by it; with a camera,
          // fovEstimate only passes validation)
          fovEstimateDeg:
              2 *
              math.atan(_decoded!.width / (2 * cam.focalLengthPx)) *
              180 /
              math.pi,
          fovMaxErrorDeg: null,
          camera: cam,
          attitudeHintWxyz: null,
          hintUncertaintyDeg: opts.hintUncertaintyDeg,
          strictHint: false,
          profile: const ExtractionProfileDto.phoneJpeg(),
          retryAlternateProfile: true,
          thorough: false,
          matchThreshold: opts.matchThreshold,
          timeoutMs: opts.timeoutMs,
          observationUnixMs: null,
        );
      }
      _usedCalibratedCamera = savedCamera != null;
      final res = await widget.pool.solveImageFileAuto(
        path: imgFile.path,
        base: base,
      );
      final out = res.outcome;
      final attempts = res.attempts
          .map(
            (a) => '${a.fovDeg.toStringAsFixed(1)}°@${a.db}→${a.status.name}',
          )
          .toList();
      _solvedBy = res.db;
      sw.stop();

      setState(() {
        _outcome = out;
        _annotations = null;
        _attempts = attempts;
        final g = out.solution;
        _status = g == null
            ? 'Not solved (${out.status.name}, ${out.centroids.length} centroids, ${sw.elapsedMilliseconds} ms)'
            : 'ra=${g.raDeg.toStringAsFixed(3)}°  dec=${g.decDeg.toStringAsFixed(3)}°  '
                  'roll=${g.rollDeg.toStringAsFixed(1)}°  fov=${g.fovDeg.toStringAsFixed(1)}°  '
                  '${g.numMatches} stars  rmse=${g.rmseArcsec.toStringAsFixed(0)}″  '
                  '${sw.elapsedMilliseconds}ms'
                  '${out.extractionRetried ? ' (profile retried)' : ''}'
                  '${_usedCalibratedCamera ? ' (calibrated)' : ''}'
                  '${_solvedBy == null ? '' : '  ← $_solvedBy'}';
      });
      await _annotate();
    } catch (e) {
      setState(() => _status = 'Error: $e');
    } finally {
      setState(() => _busy = false);
    }
  }

  // ── Viewer: pinch-zoom the photo; the overlay follows in screen space ──

  final _viewer = TransformationController();
  Size _viewSize = Size.zero;

  /// What the fit was computed for; a new image or view size refits
  (ui.Image, Size)? _fittedFor;

  bool _raDecGrid = true;
  bool _altAzGrid = false;
  bool _diagnostics = false;

  /// Annotations are requested for one viewport at a time; a reply for an older one is dropped
  int _annotateSeq = 0;

  /// Fit the photo to the view (as BoxFit.contain), then annotate for that view
  void _fit(ui.Image image, Size view) {
    final s = math.min(view.width / image.width, view.height / image.height);
    _viewer.value = Matrix4.identity()
      ..translateByDouble(
        (view.width - image.width * s) / 2,
        (view.height - image.height * s) / 2,
        0,
        1,
      )
      ..scaleByDouble(s, s, s, 1);
    _fittedFor = (image, view);
    _annotate();
  }

  /// The visible part of the image in image pixels, and the zoom: the engine fits grid
  /// spacing, line sampling and label positions to it
  ViewportDto _viewport() {
    final m = _viewer.value;
    final s = viewerScale(m);
    return ViewportDto(
      x: -m.storage[12] / s,
      y: -m.storage[13] / s,
      width: _viewSize.width / s,
      height: _viewSize.height / s,
      scale: s,
    );
  }

  /// The horizontal grid needs when and where the photo was taken (EXIF time with its zone
  /// and GPS position, reported by the solve)
  bool get _canAltAz =>
      _outcome?.observationUnixMs != null && _outcome?.observer != null;

  /// Annotate for the current view. Cheap (about a millisecond), so it runs again after every
  /// pinch: the grid gets finer as you zoom in and labels stay on the visible edges.
  Future<void> _annotate() async {
    final out = _outcome;
    final g = out?.solution;
    if (out == null || g == null || _viewSize.isEmpty) return;
    final seq = ++_annotateSeq;
    // Annotate with the tier that solved the frame (narrow tiers are denser); annotators are cached per tier
    final annotator = await _annotatorFor(_solvedBy);
    final ann = await annotator.annotate(
      wcs: g.wcs,
      opts: AnnotateOptionsDto(
        starMaxMag: 5.0,
        maxStars: 120,
        includeStarNames: true,
        includeDso: true,
        dsoMaxMag: 8.0,
        dsoOutlines: true,
        maxOutlineLevel: 3,
        language: widget.language,
        // A photo's EXIF (with its zone) or a FITS header gives the time, and EXIF GPS the
        // place: the solve reports both, so the solar-system layer needs no input here
        includeSolarSystem: out.observationUnixMs != null,
        observationUnixMs: out.observationUnixMs,
        observer: out.observer,
        includeConstellations: true,
        constellationBoundaries: true,
        equatorialGrid: _raDecGrid,
        horizontalGrid: _altAzGrid && _canAltAz,
        viewport: _viewport(),
      ),
    );
    if (seq == _annotateSeq && mounted) setState(() => _annotations = ann);
  }

  Future<void> _solveSample() async {
    final bytes = await rootBundle.load('assets/sample_scorpius.jpg');
    final dir = await getApplicationSupportDirectory();
    final f = File('${dir.path}/sample_scorpius.jpg');
    await f.writeAsBytes(bytes.buffer.asUint8List(), flush: true);
    await _solve(f);
  }

  Future<void> _solveFromGallery() async {
    final picked = await ImagePicker().pickImage(source: ImageSource.gallery);
    if (picked == null) return;
    await _solve(File(picked.path));
  }

  @override
  Widget build(BuildContext context) {
    final d = _decoded;
    return Column(
      children: [
        Expanded(
          child: d == null
              ? const Center(child: Text('Pick the sample or a photo to solve'))
              : LayoutBuilder(
                  builder: (context, box) {
                    _viewSize = box.biggest;
                    if (_fittedFor != (d, _viewSize)) {
                      // The controller notifies the viewer: set it after this frame
                      WidgetsBinding.instance.addPostFrameCallback(
                        (_) => _fit(d, _viewSize),
                      );
                    }
                    return ClipRect(
                      child: Stack(
                        children: [
                          // The photo zooms; the overlay is drawn on top in screen space, so
                          // strokes and text keep their size at any zoom
                          InteractiveViewer(
                            transformationController: _viewer,
                            constrained: false,
                            minScale: 0.01,
                            maxScale: 64,
                            boundaryMargin: const EdgeInsets.all(
                              double.infinity,
                            ),
                            onInteractionEnd: (_) => _annotate(),
                            child: SizedBox(
                              width: d.width.toDouble(),
                              height: d.height.toDouble(),
                              child: RawImage(image: d),
                            ),
                          ),
                          IgnorePointer(
                            child: CustomPaint(
                              size: _viewSize,
                              painter: SolveOverlayPainter(
                                transform: _viewer,
                                outcome: _outcome,
                                annotations: _annotations,
                                diagnostics: _diagnostics,
                              ),
                            ),
                          ),
                        ],
                      ),
                    );
                  },
                ),
        ),
        if (_outcome?.solution != null)
          Wrap(
            spacing: 8,
            children: [
              FilterChip(
                label: const Text('RA/Dec grid'),
                selected: _raDecGrid,
                onSelected: (v) {
                  setState(() => _raDecGrid = v);
                  _annotate();
                },
              ),
              Tooltip(
                message: _canAltAz
                    ? 'Altitude and azimuth where and when the photo was taken'
                    : 'Needs the capture time and GPS position (EXIF)',
                child: FilterChip(
                  label: const Text('Alt/Az grid'),
                  selected: _altAzGrid && _canAltAz,
                  onSelected: _canAltAz
                      ? (v) {
                          setState(() => _altAzGrid = v);
                          _annotate();
                        }
                      : null,
                ),
              ),
              FilterChip(
                label: const Text('Detections'),
                selected: _diagnostics,
                onSelected: (v) => setState(() => _diagnostics = v),
              ),
            ],
          ),
        if (_status != null)
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
            child: Text(_status!, style: const TextStyle(fontSize: 13)),
          ),
        if (_attempts.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(bottom: 4),
            child: Text(
              'Ladder: ${_attempts.join('  ')}',
              style: const TextStyle(fontSize: 11, color: Colors.grey),
            ),
          ),
        Padding(
          padding: const EdgeInsets.all(8),
          child: Row(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              FilledButton(
                onPressed: _busy ? null : _solveSample,
                child: const Text('Sample'),
              ),
              const SizedBox(width: 12),
              FilledButton.tonal(
                onPressed: _busy ? null : _solveFromGallery,
                child: const Text('Photos'),
              ),
            ],
          ),
        ),
      ],
    );
  }
}
