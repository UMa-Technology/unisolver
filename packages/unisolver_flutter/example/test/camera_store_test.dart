import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:unisolver_flutter_example/camera_store.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  test('camera json round-trips through shared preferences', () async {
    SharedPreferences.setMockInitialValues({});
    const j = '{"focal_length_px":1262.7,"principal_point":[959.5,539.5]}';
    await CameraStore.save(j);
    expect(await CameraStore.load(), j);
    await CameraStore.clear();
    expect(await CameraStore.load(), isNull);
  });
}
