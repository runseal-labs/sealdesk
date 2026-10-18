import 'dart:async';
import 'dart:io';
import 'dart:ui' as ui;

import 'package:desktop_drop/desktop_drop.dart';
import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:sealdesk/common/sealdesk_pairing.dart';
import 'package:sealdesk/common/sealdesk_qr_image.dart';
import 'package:sealdesk/desktop/pages/sealdesk_home_page.dart';
import 'package:sealdesk/models/relay_status.dart';
import 'package:sealdesk/generated_bridge.dart';
import 'package:sealdesk/models/platform_model.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qr_flutter/qr_flutter.dart';

const address = 'tcABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdef_-';

class _I18nBinding extends Fake implements SealdeskImpl {
  String language = 'zh';

  @override
  String translate(
      {required String name, required String locale, dynamic hint}) {
    if (language == 'en') return name;
    const values = {
      'Load QR image': '载入二维码图片',
      'Drop one QR image at a time': '请一次拖入一张二维码图片',
      'Choose how to use SealDesk. Your choice will be remembered.':
          '请选择使用方式，下次启动会记住你的选择',
      'Share this device': '共享本机',
      'Connect to a device': '连接设备',
      'Copy pairing code': '复制配对码',
      'Refresh password': '刷新密码',
      'Reset address': '重置地址',
      'Confirm reset': '确认重置',
      'Hide QR code': '遮挡二维码',
      'Show QR code': '显示二维码',
      'Sharing stopped': '共享已停止',
      'Waiting for connection': '等待连接',
      'Connected': '已连接',
      'Updating QR code...': '正在更新二维码…',
      'Resetting address...': '正在重置地址…',
      'This disconnects the current session and invalidates all old QR codes. Share the new code with the other person.': '这会断开当前被控会话，并使所有旧二维码失效。请将新二维码重新分享给对方。',
      'QR code hidden': '二维码已遮挡',
      'Unable to get pairing code, please try again': '无法获取二维码，请重试',
      'Connecting...': '正在连接…',
      'Connect': '连接',
      'Cancel': '取消',
      'Paste text': '粘贴文本',
    };
    return values[name] ?? name;
  }
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final binding = _I18nBinding();
  platformFFI.setBindingForTesting(binding);
  String? clipboard;
  setUp(() {
    binding.language = 'zh';
    clipboard = null;
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(SystemChannels.platform, (call) async {
      if (call.method == 'Clipboard.setData') {
        clipboard = (call.arguments as Map)['text'] as String;
      }
      if (call.method == 'Clipboard.getData') return {'text': clipboard};
      return null;
    });
  });
  tearDown(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(SystemChannels.platform, null);
  });

  Future<void> showPage(
    WidgetTester tester, {
    String mode = 'host',
    String? initialInput,
    Future<SealDeskHostState> Function()? loadHost,
    Future<void> Function()? refresh,
    Future<void> Function(SealDeskPairing)? connect,
    Future<void> Function(String)? saveMode,
    int Function()? now,
    Future<SealDeskPairing?> Function()? pickImage,
    Future<SealDeskPairing> Function(String)? loadImageFile,
    Future<void> Function()? resetAddress,
    Future<SealDeskPairing> Function()? pasteImage,
  }) async {
    await tester.binding.setSurfaceSize(const Size(800, 900));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    await tester.pumpWidget(MaterialApp(
      home: Scaffold(
        body: RepaintBoundary(
          key: const ValueKey('page-screenshot'),
          child: SealDeskHomePage(
            initialMode: mode,
            initialInput: initialInput,
            loadHost: loadHost ??
                () async => const SealDeskHostState(
                    address: address, password: 'secret', name: '电脑'),
            refreshPassword: refresh ?? () async {},
            connect: connect ?? (_) async {},
            saveMode: saveMode ?? (_) async {},
            now: now ?? () => 100,
            pickImage: pickImage,
            loadImageFile: loadImageFile,
            resetAddress: resetAddress,
            pasteImage: pasteImage,
          ),
        ),
      ),
    ));
    await tester.pump();
  }

  testWidgets('startup text connects once with original password',
      (tester) async {
    final connections = <SealDeskPairing>[];
    const pairing =
        SealDeskPairing(address: address, password: '秘密 &?= ;:', name: '工作电脑');
    await showPage(tester,
        mode: 'control',
        initialInput: pairing.encode(now: 100),
        connect: (pairing) async => connections.add(pairing));
    await tester.pump(const Duration(seconds: 2));
    expect(connections, hasLength(1));
    expect(connections.single.password, pairing.password);
    expect(connections.single.name, pairing.name);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('startup image imports once and rechecks expiry after decoding',
      (tester) async {
    var now = 100;
    var connects = 0;
    final result = Completer<SealDeskPairing>();
    await showPage(tester,
        mode: 'control',
        initialInput: '/tmp/配对 图.png',
        now: () => now,
        connect: (_) async => connects++,
        loadImageFile: (path) {
          expect(path, '/tmp/配对 图.png');
          return result.future;
        });
    now = 102;
    result.complete(const SealDeskPairing(address: address, expiresAt: 101));
    await tester.pumpAndSettle();
    expect(connects, 0);
    expect(find.textContaining('已过期'), findsOneWidget);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('startup image connects once and ignores results after disposal',
      (tester) async {
    var connects = 0;
    var imports = 0;
    await showPage(tester,
        mode: 'control',
        initialInput: '/tmp/二维码.png',
        connect: (_) async => connects++,
        loadImageFile: (_) async {
          imports++;
          return const SealDeskPairing(address: address);
        });
    await tester.pump(const Duration(seconds: 2));
    expect(connects, 1);
    expect(imports, 1);
    await tester.pumpWidget(const SizedBox());
    final result = Completer<SealDeskPairing>();
    await showPage(tester,
        mode: 'control',
        initialInput: '/tmp/二维码.png',
        connect: (_) async => connects++,
        loadImageFile: (_) => result.future);
    await tester.pumpWidget(const SizedBox());
    result.complete(const SealDeskPairing(address: address));
    await tester.pump();
    expect(connects, 1);
    expect(tester.takeException(), isNull);
  });

  testWidgets('expired startup code and host mode never connect',
      (tester) async {
    var connects = 0;
    const pairing = SealDeskPairing(address: address, expiresAt: 99);
    final code = pairing.encode(now: 98);
    await showPage(tester,
        mode: 'control', initialInput: code, connect: (_) async => connects++);
    expect(find.textContaining('已过期'), findsOneWidget);
    expect(connects, 0);
    await tester.pumpWidget(const SizedBox());
    await showPage(tester,
        mode: 'host', initialInput: code, connect: (_) async => connects++);
    expect(connects, 0);
    await tester.pumpWidget(const SizedBox());
  });

  Future<SealDeskPairing> copyPairing(WidgetTester tester,
      {int now = 100}) async {
    await tester.tap(find.text('复制配对码'));
    await tester.pump();
    return SealDeskPairing.parse(clipboard!, now: now);
  }

  // desktop_drop 0.4.4 accepts a drop while idle only for GTK
  // (performOperation_linux). macOS and Windows enter first, then
  // performOperation with filesystem paths.
  Future<void> dropFiles(WidgetTester tester, String uris) async {
    final center = tester.getCenter(find.byType(DropTarget));
    final messenger = tester.binding.defaultBinaryMessenger;
    Future<void> invoke(String method, Object? arguments) {
      return messenger.handlePlatformMessage(
        'desktop_drop',
        const StandardMethodCodec().encodeMethodCall(
          MethodCall(method, arguments),
        ),
        (_) {},
      );
    }

    if (Platform.isLinux) {
      await invoke('performOperation_linux', [
        uris,
        [center.dx, center.dy],
      ]);
    } else {
      final paths = uris
          .split('\n')
          .where((line) => line.isNotEmpty)
          .map((line) => Uri.parse(line).toFilePath(windows: false))
          .toList();
      final scale = Platform.isWindows ? tester.view.devicePixelRatio : 1.0;
      await invoke('entered', [center.dx * scale, center.dy * scale]);
      await invoke('performOperation', paths);
    }
    await tester.pump();
  }

  testWidgets('rendered host QR decodes into the same real pairing data',
      (tester) async {
    await showPage(tester);
    await tester.pumpAndSettle();
    final boundary = tester.renderObject<RenderRepaintBoundary>(
        find.byKey(const ValueKey('page-screenshot')));
    final pairing = (await tester.runAsync(() async {
      final screenshot = await boundary.toImage(pixelRatio: 2);
      final bytes = await screenshot.toByteData(format: ui.ImageByteFormat.png);
      final decoded =
          decodeSealDeskQrImage(bytes!.buffer.asUint8List(), now: 100);
      screenshot.dispose();
      return decoded;
    }))!;
    expect(pairing.password, 'secret');
    expect(pairing.name, '电脑');
    expect(pairing.address, address);
    final qr = tester.widget<QrImageView>(find.byType(QrImageView));
    expect(qr.errorCorrectionLevel, QrErrorCorrectLevel.M);
  });

  testWidgets(
      'image selection connects automatically and cancellation is quiet',
      (tester) async {
    SealDeskPairing? selected;
    final connected = <SealDeskPairing>[];
    await showPage(tester,
        mode: 'control',
        pickImage: () async => selected,
        connect: (pairing) async => connected.add(pairing));
    await tester.tap(find.text('载入二维码图片'));
    await tester.pump();
    expect(connected, isEmpty);
    expect(find.byKey(const ValueKey('sealdesk-error')), findsNothing);
    selected =
        const SealDeskPairing(address: address, password: ' from image ');
    await tester.tap(find.text('载入二维码图片'));
    await tester.pump();
    expect(connected.single.password, ' from image ');
  });

  testWidgets('image that expires during selection is rejected before login',
      (tester) async {
    var connected = false;
    await showPage(tester,
        mode: 'control',
        pickImage: () async =>
            const SealDeskPairing(address: address, expiresAt: 100),
        connect: (_) async => connected = true);
    await tester.tap(find.text('载入二维码图片'));
    await tester.pump();
    expect(connected, isFalse);
    expect(find.text('配对码已过期，请让对方刷新二维码'), findsOneWidget);
  });

  testWidgets('clipboard QR screenshot connects with its original credentials',
      (tester) async {
    await showPage(tester);
    await tester.pumpAndSettle();
    final boundary = tester.renderObject<RenderRepaintBoundary>(
        find.byKey(const ValueKey('page-screenshot')));
    final bytes = (await tester.runAsync(() async {
      final screenshot = await boundary.toImage(pixelRatio: 2);
      final data = await screenshot.toByteData(format: ui.ImageByteFormat.png);
      screenshot.dispose();
      return data!.buffer.asUint8List();
    }))!;
    await tester.pumpWidget(const SizedBox());
    final connected = <SealDeskPairing>[];
    await showPage(tester,
        mode: 'control',
        pasteImage: () async => decodeSealDeskQrImage(bytes, now: 100),
        connect: (pairing) async => connected.add(pairing));
    await tester.tap(find.byKey(const ValueKey('sealdesk-paste-image')));
    await tester.pumpAndSettle();
    expect(connected, hasLength(1));
    expect(connected.single.address, address);
    expect(connected.single.password, 'secret');
    expect(connected.single.name, '电脑');
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('clipboard image errors explain the failure without connecting',
      (tester) async {
    var connections = 0;
    await showPage(tester,
        mode: 'control',
        pasteImage: () async {
          throw const FormatException('剪贴板中没有可读取的图片，请复制二维码截图后重试');
        },
        connect: (_) async => connections++);
    await tester.tap(find.byKey(const ValueKey('sealdesk-paste-image')));
    await tester.pumpAndSettle();
    expect(find.text('剪贴板中没有可读取的图片，请复制二维码截图后重试'), findsOneWidget);
    expect(connections, 0);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('clipboard import ignores duplicate clicks and rechecks expiry',
      (tester) async {
    var now = 100;
    var calls = 0;
    var connections = 0;
    final result = Completer<SealDeskPairing>();
    await showPage(tester,
        mode: 'control',
        now: () => now,
        pasteImage: () {
          calls++;
          return result.future;
        },
        connect: (_) async => connections++);
    await tester.tap(find.byKey(const ValueKey('sealdesk-paste-image')));
    await tester.pump();
    final button = tester
        .widget<TextButton>(find.byKey(const ValueKey('sealdesk-paste-image')));
    expect(button.onPressed, isNull);
    now = 102;
    result.complete(const SealDeskPairing(address: address, expiresAt: 101));
    await tester.pumpAndSettle();
    expect(calls, 1);
    expect(connections, 0);
    expect(find.textContaining('已过期'), findsOneWidget);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('single image drop connects once and multiple files are rejected',
      (tester) async {
    final loaded = <String>[];
    var connections = 0;
    await showPage(tester,
        mode: 'control',
        loadImageFile: (path) async {
          loaded.add(path);
          return const SealDeskPairing(address: address, password: 'image');
        },
        connect: (_) async => connections++);
    await dropFiles(tester, 'file:///tmp/qr.png\n');
    expect(loaded, ['/tmp/qr.png']);
    expect(connections, 1);
    await dropFiles(tester, 'file:///tmp/a.png\nfile:///tmp/b.png\n');
    expect(loaded, hasLength(1));
    expect(connections, 1);
    expect(find.text('请一次拖入一张二维码图片'), findsOneWidget);
  });

  testWidgets('home labels and dynamic status follow language changes',
      (tester) async {
    await showPage(tester, mode: 'host');
    expect(find.text('共享本机'), findsOneWidget);
    expect(find.text('等待连接'), findsOneWidget);
    binding.language = 'en';
    await tester.pump(const Duration(seconds: 1));
    expect(find.text('Share this device'), findsOneWidget);
    expect(find.text('Waiting for connection'), findsOneWidget);
  });

  testWidgets('host status follows active remote connection count',
      (tester) async {
    var count = 0;
    await showPage(tester,
        loadHost: () async => SealDeskHostState(
            address: address,
            password: 'secret',
            name: '电脑',
            connectedCount: count));
    expect(find.text('等待连接'), findsOneWidget);
    for (final active in [1, 2, 0]) {
      count = active;
      await tester.pump(const Duration(seconds: 1));
      await tester.pump();
      expect(find.text(active == 0 ? '等待连接' : '已连接 ($active)'),
          findsOneWidget);
      if (active > 0) expect(find.text('等待连接'), findsNothing);
      expect(find.byType(QrImageView), findsOneWidget);
    }
  });

  testWidgets('first launch chooses and remembers mode before publishing QR',
      (tester) async {
    final saved = <String>[];
    await showPage(tester, mode: '', saveMode: (mode) async => saved.add(mode));
    expect(find.byType(QrImageView), findsNothing);
    expect(find.text('请选择使用方式，下次启动会记住你的选择'), findsOneWidget);
    await tester.tap(find.text('共享本机'));
    await tester.pump();
    expect(saved, ['host']);
    expect(find.byType(QrImageView), findsOneWidget);
  });

  testWidgets('failed mode switch keeps the current mode and reports failure',
      (tester) async {
    await showPage(tester, mode: 'control', saveMode: (_) async {
      throw const FormatException('切换失败，请重试');
    });
    await tester.tap(find.text('共享本机'));
    await tester.pumpAndSettle();
    expect(find.byType(QrImageView), findsNothing);
    expect(find.text('切换失败，请重试'), findsOneWidget);
    final control =
        tester.widget<ChoiceChip>(find.widgetWithText(ChoiceChip, '连接设备'));
    expect(control.selected, isTrue);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets(
      'mode switch waits for service confirmation and ignores duplicates',
      (tester) async {
    final confirmation = Completer<void>();
    var calls = 0;
    await showPage(tester, mode: '', saveMode: (_) {
      calls++;
      return confirmation.future;
    });
    await tester.tap(find.text('共享本机'));
    await tester.pump();
    await tester.tap(find.text('连接设备'));
    await tester.pump();
    expect(calls, 1);
    expect(find.byType(QrImageView), findsNothing);
    confirmation.complete();
    await tester.pumpAndSettle();
    expect(find.byType(QrImageView), findsOneWidget);
    await tester.tap(find.text('连接设备'));
    await tester.pumpAndSettle();
    expect(find.byType(QrImageView), findsNothing);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('host copies real password and keeps QR stable between polls',
      (tester) async {
    await showPage(tester);
    final first = await copyPairing(tester);
    expect(first.address, address);
    expect(first.password, 'secret');
    expect(first.name, '电脑');
    expect(first.expiresAt, 700);
    final encoded = clipboard;
    await tester.pump(const Duration(seconds: 1));
    await copyPairing(tester);
    expect(clipboard, encoded);
    expect(find.text('等待连接'), findsOneWidget);
  });

  testWidgets('password refresh redraws QR with updated password',
      (tester) async {
    var password = 'old';
    await showPage(
      tester,
      loadHost: () async =>
          SealDeskHostState(address: address, password: password, name: '电脑'),
      refresh: () async => password = 'new',
    );
    expect((await copyPairing(tester)).password, 'old');
    await tester.tap(find.text('刷新密码'));
    await tester.pump();
    expect((await copyPairing(tester)).password, 'new');
  });

  testWidgets('address reset requires confirmation and cancellation keeps QR',
      (tester) async {
    var calls = 0;
    await showPage(tester);
    expect(find.text('重置地址'), findsNothing);
    await showPage(tester, resetAddress: () async => calls++);
    final original = await copyPairing(tester);
    await tester.tap(find.byKey(const ValueKey('sealdesk-reset-address')));
    await tester.pumpAndSettle();
    expect(find.textContaining('这会断开当前被控会话'), findsOneWidget);
    expect(calls, 0);
    await tester.tap(find.text('取消'));
    await tester.pumpAndSettle();
    expect(calls, 0);
    expect((await copyPairing(tester)).address, original.address);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('address reset hides stale QR until a new address arrives',
      (tester) async {
    var calls = 0;
    var polls = 0;
    var currentAddress = address;
    const replacement = '${address}new';
    final confirmation = Completer<void>();
    final stalePoll = Completer<SealDeskHostState>();
    await showPage(tester, loadHost: () {
      if (++polls == 2) return stalePoll.future;
      return Future.value(SealDeskHostState(
          address: currentAddress, password: 'secret', name: '电脑'));
    }, resetAddress: () {
      calls++;
      return confirmation.future;
    });
    await tester.pump(const Duration(seconds: 1));
    await tester.tap(find.byKey(const ValueKey('sealdesk-reset-address')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('确认重置'));
    await tester.pumpAndSettle();
    expect(calls, 1);
    expect(find.byType(QrImageView), findsNothing);
    final button = tester.widget<TextButton>(
        find.byKey(const ValueKey('sealdesk-reset-address')));
    expect(button.onPressed, isNull);
    stalePoll.complete(const SealDeskHostState(
        address: address, password: 'secret', name: '电脑'));
    await tester.pump(const Duration(seconds: 1));
    expect(find.byType(QrImageView), findsNothing);
    confirmation.complete();
    await tester.pumpAndSettle();
    expect(find.byType(QrImageView), findsNothing);
    expect(find.text('正在更新二维码…'), findsOneWidget);
    currentAddress = replacement;
    await tester.pump(const Duration(seconds: 1));
    final renewed = await copyPairing(tester);
    expect(renewed.address, replacement);
    expect(renewed.password, 'secret');
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('failed reset keeps old QR hidden and can be retried',
      (tester) async {
    var calls = 0;
    var currentAddress = address;
    await showPage(tester,
        loadHost: () async => SealDeskHostState(
            address: currentAddress, password: 'secret', name: '电脑'),
        resetAddress: () async {
          if (++calls == 1) throw const FormatException('服务未确认重置，请重试');
          currentAddress = '${address}new';
        });
    for (var attempt = 0; attempt < 2; attempt++) {
      await tester.tap(find.byKey(const ValueKey('sealdesk-reset-address')));
      await tester.pumpAndSettle();
      await tester.tap(find.text('确认重置'));
      await tester.pumpAndSettle();
      if (attempt == 0) {
        await tester.pump(const Duration(seconds: 1));
        expect(find.text('服务未确认重置，请重试'), findsOneWidget);
        expect(find.byType(QrImageView), findsNothing);
      }
    }
    expect(calls, 2);
    expect((await copyPairing(tester)).address, '${address}new');
    expect(find.text('服务未确认重置，请重试'), findsNothing);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('expiry renews QR without changing the host password',
      (tester) async {
    var now = 100;
    await showPage(tester, now: () => now);
    expect((await copyPairing(tester)).expiresAt, 700);
    now = 700;
    await tester.pump(const Duration(seconds: 1));
    final renewed = await copyPairing(tester, now: now);
    expect(renewed.expiresAt, 1300);
    expect(renewed.password, 'secret');
  });

  testWidgets('mask hides QR and restores it on demand', (tester) async {
    await showPage(tester);
    await tester.tap(find.text('遮挡二维码'));
    await tester.pump();
    expect(find.byType(QrImageView), findsNothing);
    expect(find.text('二维码已遮挡'), findsOneWidget);
    await tester.tap(find.text('显示二维码'));
    await tester.pump();
    expect(find.byType(QrImageView), findsOneWidget);
  });

  testWidgets(
      'stopped or unavailable service never publishes stale credentials',
      (tester) async {
    var stopped = false;
    await showPage(tester,
        loadHost: () async => SealDeskHostState(
              address: address,
              password: 'secret',
              name: '电脑',
              stopped: stopped,
            ));
    expect(find.byType(QrImageView), findsOneWidget);
    stopped = true;
    await tester.pump(const Duration(seconds: 1));
    expect(find.byType(QrImageView), findsNothing);
    expect(find.text('共享已停止'), findsOneWidget);
    final copy =
        tester.widget<TextButton>(find.byKey(const ValueKey('sealdesk-copy')));
    expect(copy.onPressed, isNull);
  });

  testWidgets('load failure removes QR and reports a recoverable error',
      (tester) async {
    await showPage(tester, loadHost: () async => throw StateError('offline'));
    expect(find.byType(QrImageView), findsNothing);
    expect(find.text('无法获取二维码，请重试'), findsOneWidget);
  });

  testWidgets(
      'pairing text passes password unchanged and clears input after launch',
      (tester) async {
    SealDeskPairing? connected;
    await showPage(tester,
        mode: 'control', connect: (pairing) async => connected = pairing);
    const expected = SealDeskPairing(
        address: address,
        password: ' pass;,:" 中\\ ',
        name: '桌面',
        expiresAt: 200);
    await tester.enterText(find.byType(TextField), expected.encode(now: 100));
    await tester.tap(find.text('连接'));
    await tester.pump();
    expect(connected!.password, expected.password);
    expect(connected!.address, expected.address);
    expect(connected!.name, expected.name);
    expect(tester.widget<TextField>(find.byType(TextField)).controller!.text,
        isEmpty);
  });

  testWidgets('expired text is rejected before creating a session',
      (tester) async {
    var connected = false;
    await showPage(tester,
        mode: 'control', connect: (_) async => connected = true);
    await tester.enterText(
        find.byType(TextField), 'SEALDESK:V:1;A:$address;P:secret;E:100;;');
    await tester.tap(find.text('连接'));
    await tester.pump();
    expect(connected, isFalse);
    expect(find.text('配对码已过期，请让对方刷新二维码'), findsOneWidget);
  });

  testWidgets(
      'omitted password remains absent for the normal authentication prompt',
      (tester) async {
    SealDeskPairing? connected;
    await showPage(tester,
        mode: 'control', connect: (pairing) async => connected = pairing);
    await tester.enterText(find.byType(TextField), 'SEALDESK:V:1;A:$address;;');
    await tester.tap(find.text('连接'));
    await tester.pump();
    expect(connected, isNotNull);
    expect(connected!.password, isNull);
  });

  testWidgets('clipboard paste preserves whitespace inside passwords',
      (tester) async {
    SealDeskPairing? connected;
    await showPage(tester,
        mode: 'control', connect: (pairing) async => connected = pairing);
    const pairing = SealDeskPairing(address: address, password: ' two words ');
    clipboard = pairing.encode(now: 100);
    await tester.tap(find.text('粘贴文本'));
    await tester.pump();
    expect(connected!.password, pairing.password);
    expect(tester.widget<TextField>(find.byType(TextField)).controller!.text,
        isEmpty);
  });

  testWidgets('invalid image reports an error without starting a session',
      (tester) async {
    var connected = false;
    await showPage(tester,
        mode: 'control',
        pickImage: () async => throw const FormatException('未找到二维码，请重新截图'),
        connect: (_) async => connected = true);
    await tester.tap(find.text('载入二维码图片'));
    await tester.pump();
    expect(connected, isFalse);
    expect(find.text('未找到二维码，请重新截图'), findsOneWidget);
  });

  testWidgets('pending image import ignores repeated clicks and file drops',
      (tester) async {
    final pending = Completer<SealDeskPairing?>();
    var selections = 0;
    var loads = 0;
    var connections = 0;
    await showPage(tester,
        mode: 'control',
        pickImage: () {
          selections++;
          return pending.future;
        },
        loadImageFile: (_) async {
          loads++;
          return const SealDeskPairing(address: address);
        },
        connect: (_) async => connections++);
    await tester.tap(find.text('载入二维码图片'));
    await tester.pump();
    await tester.tap(find.text('载入二维码图片'));
    await dropFiles(tester, 'file:///tmp/other.png\n');
    expect(selections, 1);
    expect(loads, 0);
    pending.complete(const SealDeskPairing(address: address));
    await tester.pump();
    expect(connections, 1);
  });

  testWidgets(
      'disposing during connection does not update a disposed controller',
      (tester) async {
    final completed = Completer<void>();
    await showPage(tester, mode: 'control', connect: (_) => completed.future);
    await tester.enterText(find.byType(TextField), 'SEALDESK:V:1;A:$address;;');
    await tester.tap(find.text('连接'));
    await tester.pump();
    await tester.pumpWidget(const MaterialApp(home: SizedBox()));
    completed.complete();
    await tester.pump();
    expect(tester.takeException(), isNull);
  });

  testWidgets(
      'relay chip stays under the QR and init errors replace the placeholder',
      (tester) async {
    RelayStatus? relay = RelayStatus.fromJson(
        '{"running":true,"state":"reachable","probe_rtt_ms":42}');
    var addr = address;
    await showPage(tester,
        loadHost: () async => SealDeskHostState(
            address: addr,
            password: 'secret',
            name: '电脑',
            relayStatus: relay));
    expect(find.byType(QrImageView), findsOneWidget);
    expect(find.text('等待连接'), findsOneWidget);
    expect(find.text('Relay reachable (42 ms)'), findsOneWidget);
    expect(find.byKey(const ValueKey('sealdesk-relay')), findsOneWidget);

    relay = RelayStatus.fromJson(
        '{"running":true,"state":"unreachable","last_error":"dial timeout"}');
    await tester.pump(const Duration(seconds: 1));
    await tester.pump();
    expect(find.byType(QrImageView), findsOneWidget);
    expect(find.text('等待连接'), findsOneWidget);
    expect(find.text('Relay unreachable; scanning will likely time out'),
        findsOneWidget);
    expect(find.text('dial timeout'), findsOneWidget);
    expect(find.text('Relay reachable (42 ms)'), findsNothing);

    addr = '';
    relay = RelayStatus.fromJson(
        '{"running":false,"state":"stopped","init_stage":"start","init_error":"bind"}');
    await tester.pump(const Duration(seconds: 1));
    await tester.pump();
    expect(find.byType(QrImageView), findsNothing);
    expect(find.text('start: bind'), findsOneWidget);
    expect(find.textContaining('Generating pairing code'), findsNothing);
    expect(find.byKey(const ValueKey('sealdesk-relay')), findsNothing);

    relay = RelayStatus.fromJson('{"state":"connecting"}');
    await tester.pump(const Duration(seconds: 1));
    await tester.pump();
    expect(find.text('Generating pairing code...'), findsOneWidget);
    expect(find.text('Checking relay...'), findsOneWidget);
    expect(find.text('start: bind'), findsNothing);
    await tester.pumpWidget(const SizedBox());
  });
}
