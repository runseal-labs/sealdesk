import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sealdesk/common.dart';
import 'package:sealdesk/generated_bridge.dart';
import 'package:sealdesk/main.dart';
import 'package:sealdesk/models/platform_model.dart';

class _Core extends Fake implements SealdeskImpl {
  @override
  String translate(
          {required String name, required String locale, dynamic hint}) =>
      name;

  @override
  bool mainHideDock({dynamic hint}) => true;

  @override
  String mainGetLocalOption({required String key, dynamic hint}) => '';

  @override
  String getLocalFlutterOption({required String k, dynamic hint}) => '';
}

void main() {
  final binding = TestWidgetsFlutterBinding.ensureInitialized();
  final calls = <String>[];

  setUp(() async {
    isTest = true;
    calls.clear();
    platformFFI.setBindingForTesting(_Core());
    binding.defaultBinaryMessenger.setMockMethodCallHandler(
        const MethodChannel('window_manager'), (call) async {
      calls.add(call.method);
      if (call.method == 'getOpacity') return 1.0;
      if (call.method == 'getBounds') {
        return {'x': 0.0, 'y': 0.0, 'width': 800.0, 'height': 600.0};
      }
      return false;
    });
    await initGlobalFFI();
    // Non-startup hide only reaches the window after CM startup. That is
    // the path Disconnect hits once the window has been shown.
    await hideCmWindow(isStartup: true);
    calls.clear();
  });

  test('hideCmWindow does not change the window after close starts', () async {
    gFFI.serverModel.closeCmWindow();
    await hideCmWindow();

    expect(calls, contains('close'));
    for (final method in const [
      'setOpacity',
      'minimize',
      'hide',
      'getOpacity',
    ]) {
      expect(calls, isNot(contains(method)));
    }
  });
}
