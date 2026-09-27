import 'dart:io';

import 'package:flutter/material.dart';
import 'package:unisolver_flutter/unisolver_flutter.dart';

import 'calibrate_page.dart';
import 'db_page.dart';
import 'live_page.dart';
import 'solve_page.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await RustLib.init();
  runApp(const UnisolverExampleApp());
}

class UnisolverExampleApp extends StatelessWidget {
  const UnisolverExampleApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'unisolver',
      theme: ThemeData.dark(useMaterial3: true),
      home: const _Home(),
    );
  }
}

class _Home extends StatefulWidget {
  const _Home();

  @override
  State<_Home> createState() => _HomeState();
}

class _HomeState extends State<_Home> {
  int _tab = 0;
  UniSolver? _solver;
  UniSolverPool? _pool;
  String? _dbDir;
  String? _dsoPath;
  String? _namesPath;
  String? _constellationsPath;
  String? _error;

  /// Annotation language (whatever the names pack offers)
  String _lang = 'en';
  List<String> _languages = const [];

  @override
  void initState() {
    super.initState();
    _init();
  }

  Future<void> _init() async {
    try {
      final paths = await UnisolverAssets.ensureInstalled();
      // The names pack is opt-in (GPL): this example declares the optional asset in its
      // pubspec; without it the Databases page can download it from a manifest host
      final namesPath = await UnisolverAssets.installNames();
      // IAU constellation figures and boundaries: bundled with the plugin (CC BY-SA 4.0)
      final constellationsPath = await UnisolverAssets.installConstellations();
      // Once the bundled wide tier is decompressed the directory has at least one tier; the pool
      // scans it (narrow tiers the user installed are there too)
      final dir = File(paths.dbPath).parent.path;
      final pool = await UniSolverPool.openDir(dir: dir);
      final solver = await UniSolver.newInstance(dbPath: paths.dbPath);
      // The languages come from the names pack data; the UI does not hard-code them
      final languages = await _languagesOf(solver, paths.dsoPath, namesPath);
      setState(() {
        _solver = solver;
        _pool = pool;
        _dbDir = dir;
        _dsoPath = paths.dsoPath;
        _namesPath = namesPath;
        _constellationsPath = constellationsPath;
        _languages = languages;
      });
    } catch (e) {
      setState(() => _error = 'Initialization failed: $e');
    }
  }

  /// The languages come from the names pack data; the UI does not hard-code them.
  static Future<List<String>> _languagesOf(
    UniSolver solver,
    String dsoPath,
    String? namesPath,
  ) async {
    if (namesPath == null) return const ['en'];
    final probe = await solver.annotator(
      dsoPath: dsoPath,
      namesPath: namesPath,
    );
    final languages = probe.languages();
    probe.dispose(); // only needed to ask which languages exist
    return languages;
  }

  /// Called by the Databases page after it downloads the names pack.
  Future<void> _useNames(String path) async {
    final languages = await _languagesOf(_solver!, _dsoPath!, path);
    setState(() {
      _namesPath = path;
      _languages = languages;
    });
  }

  /// The attributions the data licenses require (Gaia DR3 in particular). Apps show
  /// them somewhere users can find, such as an About page.
  void _showAttributions(BuildContext context) {
    final items = dataAttributions();
    showDialog<void>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Data sources'),
        content: SizedBox(
          width: 480,
          child: ListView(
            shrinkWrap: true,
            children: [
              for (final a in items)
                ListTile(
                  title: Text('${a.name} (${a.license})'),
                  subtitle: Text('${a.appliesTo}\n${a.text}'),
                  isThreeLine: true,
                ),
            ],
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text('Close'),
          ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final solver = _solver;
    Widget body;
    if (_error != null) {
      body = Center(child: Text(_error!));
    } else if (solver == null) {
      body = const Center(
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            CircularProgressIndicator(),
            SizedBox(height: 12),
            Text('First launch: decompressing the star database…'),
          ],
        ),
      );
    } else {
      body = IndexedStack(
        index: _tab,
        children: [
          SolvePage(
            pool: _pool!,
            dsoPath: _dsoPath!,
            namesPath: _namesPath,
            constellationsPath: _constellationsPath,
            language: _lang,
          ),
          CalibratePage(solver: solver),
          LivePage(solver: solver),
          DbPage(pool: _pool!, dir: _dbDir!, onNamesInstalled: _useNames),
        ],
      );
    }
    return Scaffold(
      appBar: AppBar(
        title: const Text('unisolver example'),
        actions: [
          if (_languages.isNotEmpty)
            PopupMenuButton<String>(
              tooltip: 'Annotation language',
              initialValue: _lang,
              onSelected: (v) => setState(() => _lang = v),
              itemBuilder: (_) => [
                for (final l in _languages)
                  PopupMenuItem(value: l, child: Text(l)),
              ],
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: 12),
                child: Row(
                  children: [
                    const Icon(Icons.translate, size: 18),
                    const SizedBox(width: 4),
                    Text(_lang),
                  ],
                ),
              ),
            ),
          IconButton(
            tooltip: 'Data sources',
            icon: const Icon(Icons.info_outline),
            onPressed: () => _showAttributions(context),
          ),
        ],
      ),
      body: body,
      bottomNavigationBar: NavigationBar(
        selectedIndex: _tab,
        onDestinationSelected: (i) => setState(() => _tab = i),
        destinations: const [
          NavigationDestination(icon: Icon(Icons.image_search), label: 'Solve'),
          NavigationDestination(
            icon: Icon(Icons.center_focus_strong),
            label: 'Calibrate',
          ),
          NavigationDestination(icon: Icon(Icons.videocam), label: 'Live'),
          NavigationDestination(icon: Icon(Icons.storage), label: 'Databases'),
        ],
      ),
    );
  }
}
