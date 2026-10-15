import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_hbb/common/taildesk_pairing.dart';
import 'package:flutter_hbb/mobile/pages/taildesk_scan_page.dart';
import 'package:flutter_test/flutter_test.dart';

const address = 'tcABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdef_-';
const pairing = TailDeskPairing(
    address: address,
    password: ' 密码; with spaces ',
    name: '电脑',
    expiresAt: 200);

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  int? cameraId;
  TailDeskPairing? result;
  late List<String> cameraCalls;
  Future<void> Function()? onPause;

  MethodChannel cameraChannel() =>
      MethodChannel('net.touchcapture.qr.flutterqr/qrview_$cameraId');

  setUp(() {
    cameraId = null;
    result = null;
    cameraCalls = [];
    onPause = null;
    messenger.setMockMethodCallHandler(SystemChannels.platform_views,
        (call) async {
      if (call.arguments is! Map) return null;
      final arguments = call.arguments as Map;
      if (call.method == 'create') {
        cameraId = arguments['id'] as int;
        messenger.setMockMethodCallHandler(cameraChannel(), (cameraCall) async {
          cameraCalls.add(cameraCall.method);
          if (cameraCall.method == 'pauseCamera') await onPause?.call();
          return null;
        });
        return cameraId;
      }
      if (call.method == 'resize') {
        return {'width': arguments['width'], 'height': arguments['height']};
      }
      return null;
    });
  });

  tearDown(() {
    messenger.setMockMethodCallHandler(SystemChannels.platform_views, null);
    if (cameraId != null)
      messenger.setMockMethodCallHandler(cameraChannel(), null);
  });

  Future<void> openScanner(WidgetTester tester,
      {Future<TailDeskPairing?> Function()? pickImage,
      int Function()? now}) async {
    await tester.pumpWidget(MaterialApp(
        home: Builder(
            builder: (context) => Scaffold(
                  body: TextButton(
                      onPressed: () async {
                        result =
                            await Navigator.of(context).push<TailDeskPairing>(
                          MaterialPageRoute(
                              builder: (_) => TailDeskScanPage(
                                  pickImage: pickImage, now: now ?? () => 100)),
                        );
                      },
                      child: const Text('开始扫码')),
                ))));
    await tester.tap(find.text('开始扫码'));
    await tester.pumpAndSettle();
    expect(cameraId, isNotNull);
    expect(cameraCalls, contains('startScan'));
  }

  Future<void> emit(
      WidgetTester tester, String method, dynamic arguments) async {
    final completed = Completer<void>();
    await messenger.handlePlatformMessage(
        cameraChannel().name,
        const StandardMethodCodec()
            .encodeMethodCall(MethodCall(method, arguments)),
        (_) => completed.complete());
    await completed.future;
    await tester.pump();
  }

  Future<void> scan(WidgetTester tester, String code) =>
      emit(tester, 'onRecognizeQR', {'code': code, 'type': 'QR_CODE'});

  void scanTest(String description, WidgetTesterCallback callback) {
    testWidgets(description, (tester) async {
      try {
        await callback(tester);
      } finally {
        await tester.pumpWidget(const SizedBox());
        await tester.pump(const Duration(milliseconds: 400));
      }
    },
        variant: TargetPlatformVariant(
            {TargetPlatform.android, TargetPlatform.iOS}));
  }

  scanTest('camera returns one pairing and preserves password and name',
      (tester) async {
    await openScanner(tester);
    final code = pairing.encode(now: 100);
    await scan(tester, code);
    await scan(tester, code);
    await tester.pumpAndSettle();
    expect(result!.address, address);
    expect(result!.password, pairing.password);
    expect(result!.name, pairing.name);
    expect(cameraCalls.where((call) => call == 'pauseCamera'), hasLength(1));
    expect(find.text('开始扫码'), findsOneWidget);
  });

  for (final invalid in {
    'WIFI:S:test;;': '不是有效的 TailDesk 配对码',
    'TAILDESK:V:1;A:$address;E:100;;': '配对码已过期，请让对方刷新二维码',
    'TAILDESK:V:2;A:$address;;': '配对码版本不受支持，请升级 TailDesk',
  }.entries) {
    scanTest('invalid scan stays open: ${invalid.value}', (tester) async {
      await openScanner(tester);
      await scan(tester, invalid.key);
      await scan(tester, invalid.key);
      expect(result, isNull);
      expect(find.text(invalid.value), findsOneWidget);
      expect(cameraCalls, isNot(contains('pauseCamera')));
      await scan(tester, pairing.encode(now: 100));
      await tester.pumpAndSettle();
      expect(result, isNotNull);
    });
  }

  scanTest('permission denial blocks camera results but allows a gallery image',
      (tester) async {
    await openScanner(tester, pickImage: () async => pairing);
    await emit(tester, 'onPermissionSet', false);
    expect(find.textContaining('没有摄像头权限'), findsOneWidget);
    await scan(tester, pairing.encode(now: 100));
    expect(result, isNull);
    await tester.tap(find.text('载入二维码图片'));
    await tester.pumpAndSettle();
    expect(result!.password, pairing.password);
  });

  scanTest('cancelling the image picker resumes scanning', (tester) async {
    await openScanner(tester, pickImage: () async => null);
    await tester.tap(find.text('载入二维码图片'));
    await tester.pumpAndSettle();
    expect(result, isNull);
    expect(cameraCalls, containsAllInOrder(['pauseCamera', 'resumeCamera']));
    expect(find.byKey(const ValueKey('taildesk-scan-error')), findsNothing);
    await scan(tester, pairing.encode(now: 100));
    await tester.pumpAndSettle();
    expect(result, isNotNull);
  });

  scanTest('expiry is checked again after camera pause', (tester) async {
    var now = 100;
    await openScanner(tester, now: () => now);
    onPause = () async => now = 200;
    await scan(tester, pairing.encode(now: 100));
    await tester.pumpAndSettle();
    expect(result, isNull);
    expect(find.text('配对码已过期，请让对方刷新二维码'), findsOneWidget);
    expect(cameraCalls, contains('resumeCamera'));
  });

  scanTest('background scans are ignored and foreground resumes the camera',
      (tester) async {
    await openScanner(tester);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
    await tester.pump();
    await scan(tester, pairing.encode(now: 100));
    expect(result, isNull);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await tester.pump();
    expect(cameraCalls, containsAllInOrder(['pauseCamera', 'resumeCamera']));
    await scan(tester, pairing.encode(now: 100));
    await tester.pumpAndSettle();
    expect(result, isNotNull);
  });

  scanTest('camera events cannot replace a pending gallery selection',
      (tester) async {
    final selected = Completer<TailDeskPairing?>();
    var selections = 0;
    await openScanner(tester, pickImage: () {
      selections++;
      return selected.future;
    });
    await tester.tap(find.text('载入二维码图片'));
    await tester.pump();
    await tester.tap(find.text('载入二维码图片'));
    await scan(tester, pairing.encode(now: 100));
    expect(result, isNull);
    expect(selections, 1);
    selected
        .complete(const TailDeskPairing(address: address, password: 'gallery'));
    await tester.pumpAndSettle();
    expect(result!.password, 'gallery');
  });

  scanTest('camera pause failure does not return a pairing', (tester) async {
    await openScanner(tester);
    onPause = () async => throw PlatformException(code: 'camera_unavailable');
    await scan(tester, pairing.encode(now: 100));
    await tester.pumpAndSettle();
    expect(result, isNull);
    expect(find.text('摄像头操作失败，请返回后重新打开扫码'), findsOneWidget);
  });

  scanTest('leaving during camera pause never returns a late result',
      (tester) async {
    final paused = Completer<void>();
    await openScanner(tester);
    onPause = () => paused.future;
    await scan(tester, pairing.encode(now: 100));
    await tester.pumpWidget(const SizedBox());
    paused.complete();
    await tester.pump();
    expect(result, isNull);
    expect(tester.takeException(), isNull);
  });
}
