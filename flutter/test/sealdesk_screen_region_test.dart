import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sealdesk/common/widgets/sealdesk_connect_field.dart';
import 'package:sealdesk/generated_bridge.dart';
import 'package:sealdesk/models/platform_model.dart';

class _ScreenBinding extends Fake implements SealdeskImpl {
  int captures = 0;
  String reply = jsonEncode({
    'error': 'Unable to read the QR image. Please choose another one',
  });

  @override
  String translate(
          {required String name, required String locale, dynamic hint}) =>
      name;

  @override
  Future<String> mainGetSealdeskClipboardImage({dynamic hint}) async =>
      jsonEncode({'error': 'clipboard empty'});

  @override
  Future<String> mainCaptureSealdeskRegion(
      {required int x,
      required int y,
      required int width,
      required int height,
      dynamic hint}) async {
    captures++;
    expect(width, greaterThan(0));
    expect(height, greaterThan(0));
    return reply;
  }
}

void main() {
  final binding = _ScreenBinding();

  setUp(() {
    platformFFI.setBindingForTesting(binding);
    binding.captures = 0;
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(const MethodChannel('window_manager'),
            (call) async {
      if (call.method == 'getBounds') {
        return {'x': 0.0, 'y': 0.0, 'width': 800.0, 'height': 600.0};
      }
      return null;
    });
  });

  testWidgets('screen region recognition asks the native capturer for bytes',
      (tester) async {
    await tester.pumpWidget(MaterialApp(
      home: Scaffold(
        body: SealDeskConnectField(
          controller: TextEditingController(),
          desktop: true,
        ),
      ),
    ));
    await tester.tap(find.byKey(const ValueKey('sealdesk-connect-screen')));
    await tester.pumpAndSettle();
    final picker = find.byKey(const ValueKey('sealdesk-region-picker'));
    expect(picker, findsOneWidget);
    expect(tester.getSize(picker), const Size(800, 600));
    expect(tester.getTopLeft(picker), Offset.zero);
    final center = tester.getCenter(picker);
    final gesture = await tester.startGesture(center);
    await gesture.moveBy(const Offset(120, 80));
    await tester.pump();
    await gesture.up();
    await tester.pumpAndSettle();
    expect(picker, findsNothing);
    expect(binding.captures, 1);
    expect(
        find.text('Unable to read the QR image. Please choose another one'),
        findsOneWidget);
    await tester.pumpWidget(const SizedBox());
  });
}
