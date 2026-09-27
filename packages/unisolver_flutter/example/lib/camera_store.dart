import 'package:shared_preferences/shared_preferences.dart';

/// Persists the calibrated camera (a JSON string in Rust's serde format).
class CameraStore {
  static const key = 'unisolver.camera';

  static Future<void> save(String json) async {
    final p = await SharedPreferences.getInstance();
    await p.setString(key, json);
  }

  static Future<String?> load() async {
    final p = await SharedPreferences.getInstance();
    return p.getString(key);
  }

  static Future<void> clear() async {
    final p = await SharedPreferences.getInstance();
    await p.remove(key);
  }
}
