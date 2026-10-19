import 'dart:convert';
import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sealdesk/common/widgets/sealdesk_connect_field.dart';
import 'package:sealdesk/generated_bridge.dart';
import 'package:sealdesk/models/platform_model.dart';

class _ScreenBinding extends Fake implements SealdeskImpl {
  int captures = 0;
  bool wayland = false;
  String reply = jsonEncode({
    'error': 'Unable to read the QR image. Please choose another one',
  });

  @override
  bool mainCurrentIsWayland({dynamic hint}) => wayland;

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
    binding.wayland = false;
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
    expect(find.text('Unable to read the QR image. Please choose another one'),
        findsOneWidget);
    await tester.pumpWidget(const SizedBox());
  });

  test('mac region requests stay in points', () {
    const origin = Offset(100, 40);
    const rect = Rect.fromLTWH(10, 20, 30, 40);
    final points = sealDeskRegionRequest(origin, rect, 2, logicalPoints: true);
    expect(
      points,
      (x: 110, y: 60, width: 30, height: 40),
    );
    final pixels = sealDeskRegionRequest(origin, rect, 2, logicalPoints: false);
    expect(
      pixels,
      (x: 220, y: 120, width: 60, height: 80),
    );
  });

  testWidgets('wayland screen recognition uses a file instead of a region',
      (tester) async {
    if (!Platform.isLinux) return;
    final previous = FilePicker.platform;
    final picker = _CancelFilePicker();
    FilePicker.platform = picker;
    addTearDown(() => FilePicker.platform = previous);
    binding.wayland = true;
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
    expect(find.byKey(const ValueKey('sealdesk-region-picker')), findsNothing);
    expect(binding.captures, 0);
    expect(picker.calls, 1);
    await tester.pumpWidget(const SizedBox());
  });
}

class _CancelFilePicker extends FilePicker {
  int calls = 0;

  @override
  Future<FilePickerResult?> pickFiles({
    String? dialogTitle,
    String? initialDirectory,
    FileType type = FileType.any,
    List<String>? allowedExtensions,
    Function(FilePickerStatus)? onFileLoading,
    bool allowCompression = true,
    bool allowMultiple = false,
    bool withData = false,
    bool withReadStream = false,
    bool lockParentWindow = false,
  }) async {
    calls++;
    return null;
  }
}
