import 'dart:async';
import 'dart:io';
import 'dart:ui' as ui;

import 'package:desktop_drop/desktop_drop.dart';
import 'package:flutter/gestures.dart';
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
    final match = RegExp(r'\{(.*?)\}').firstMatch(name);
    var key = name;
    String? value;
    if (match != null) {
      value = match.group(1);
      key = name.replaceFirst(match.group(0)!, '{}');
    }
    if (language == 'en') {
      return value == null ? key : key.replaceFirst('{}', value);
    }
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
      'Tap to show': '点按显示',
      'Sharing stopped': '共享已停止',
      'Sharing': '共享中',
      'Waiting for connection': '等待连接',
      'Start service': '启动服务',
      'Stop service': '停止服务',
      'Stranger asks you to share? Hang up': '陌生人来电要求共享？请挂断',
      'Connected': '已连接',
      'Updating QR code...': '正在更新二维码…',
      'Resetting address...': '正在重置地址…',
      'This disconnects the current session and invalidates all old QR codes. Share the new code with the other person.':
          '这会断开当前被控会话，并使所有旧二维码失效。请将新二维码重新分享给对方。',
      'QR code hidden': '二维码已遮挡',
      'Close': '关闭',
      'Unable to get pairing code, please try again': '无法获取二维码，请重试',
      'Connecting...': '正在连接…',
      'Connect': '连接',
      'Cancel': '取消',
      'Paste text': '粘贴文本',
      'Checking relay...': '正在检查中继…',
      'Relay reachable': '中继可达',
      'Relay unreachable; scanning will likely time out': '中继不可达，扫码连接很可能会超时',
      'Cannot reach the relay server. Check the network or relay settings':
          '连不上中继服务器，请检查网络或中继设置',
      'Error details': '错误详情',
      'QR refreshes in {} min': '二维码 {} 分钟后刷新',
    };
    final text = values[key] ?? key;
    return value == null ? text : text.replaceFirst('{}', value);
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
    bool sidePane = false,
    bool hostOnly = false,
    Future<void> Function()? onStart,
    Future<void> Function()? onStop,
    Size surface = const Size(800, 900),
  }) async {
    await tester.binding.setSurfaceSize(surface);
    addTearDown(() => tester.binding.setSurfaceSize(null));
    await tester.pumpWidget(MaterialApp(
      home: Scaffold(
        body: RepaintBoundary(
          key: const ValueKey('page-screenshot'),
          child: Builder(builder: (context) {
            final page = SealDeskHomePage(
              sidePane: sidePane,
              hostOnly: hostOnly || sidePane,
              initialMode: mode,
              initialInput: initialInput,
              loadHost: loadHost ??
                  () async => const SealDeskHostState(
                      address: address, password: 'secret', name: '电脑'),
              refreshPassword: refresh ?? () async {},
              connect: connect ?? (_) async {},
              saveMode: saveMode ?? (_) async {},
              onStart: onStart,
              onStop: onStop,
              now: now ?? () => 100,
              pickImage: pickImage,
              loadImageFile: loadImageFile,
              resetAddress: resetAddress,
              pasteImage: pasteImage,
            );
            if (!sidePane) return page;
            return MediaQuery(
              data: MediaQueryData(size: surface),
              child: Align(
                alignment: Alignment.topLeft,
                child: SizedBox(
                  width: surface.width < 800 ? 200 : 300,
                  child: page,
                ),
              ),
            );
          }),
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
    expect(find.textContaining('expired'), findsOneWidget);
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
    expect(find.textContaining('expired'), findsOneWidget);
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
    expect(
        find.text(
            'Pairing code has expired. Ask the other person to refresh the QR code'),
        findsOneWidget);
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
    expect(find.textContaining('expired'), findsOneWidget);
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
    expect(find.text('共享中'), findsOneWidget);
    binding.language = 'en';
    await tester.pump(const Duration(seconds: 1));
    expect(find.text('Share this device'), findsOneWidget);
    expect(find.text('Sharing'), findsOneWidget);
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
    expect(find.text('共享中'), findsOneWidget);
    expect(find.byType(QrImageView), findsOneWidget);
    for (final active in [1, 2, 0]) {
      count = active;
      await tester.pump(const Duration(seconds: 1));
      await tester.pump();
      expect(find.text('共享中'), findsOneWidget);
      // 0 -> N masks; staying connected or dropping back to 0 does not
      // reveal or mask again.
      expect(find.byType(QrImageView), findsNothing);
      expect(find.text('点按显示'), findsOneWidget);
    }
  });

  testWidgets('missing or invalid saved page opens share this device',
      (tester) async {
    for (final mode in ['', 'nope']) {
      final saved = <String>[];
      await showPage(tester,
          mode: mode, saveMode: (mode) async => saved.add(mode));
      expect(find.byType(QrImageView), findsOneWidget);
      expect(find.byKey(const ValueKey('sealdesk-input')), findsNothing);
      expect(find.text('请选择使用方式，下次启动会记住你的选择'), findsNothing);
      expect(
        tester
            .widget<ChoiceChip>(find.widgetWithText(ChoiceChip, '共享本机'))
            .selected,
        isTrue,
      );
      expect(
        tester
            .widget<ChoiceChip>(find.widgetWithText(ChoiceChip, '连接设备'))
            .selected,
        isFalse,
      );
      expect(saved, isEmpty);
      await tester.pumpWidget(const SizedBox());
    }
  });

  test('default page is fixed and a stored mode is not restored', () {
    expect(sealDeskInitialPage(), 'host');
    expect(sealDeskPersistsUiMode(desktop: true), isFalse);
    expect(sealDeskPersistsUiMode(desktop: false), isTrue);
  });

  testWidgets(
      'opening the share page does not rewrite mode or report switch failure',
      (tester) async {
    for (final forced in [
      (sidePane: true, hostOnly: false),
      (sidePane: false, hostOnly: true),
    ]) {
      final saved = <String>[];
      await showPage(
        tester,
        mode: 'control',
        sidePane: forced.sidePane,
        hostOnly: forced.hostOnly,
        saveMode: (mode) async {
          saved.add(mode);
          throw const FormatException('切换失败，请重试');
        },
      );
      expect(saved, isEmpty);
      expect(find.text('切换失败，请重试'), findsNothing);
      await tester.pump(const Duration(seconds: 1));
      expect(saved, isEmpty);
      expect(find.text('切换失败，请重试'), findsNothing);
      await tester.pumpWidget(const SizedBox());
    }
  });

  testWidgets('stopped share card stays stopped until start is pressed',
      (tester) async {
    var starts = 0;
    final saved = <String>[];
    await showPage(
      tester,
      mode: 'control',
      hostOnly: true,
      saveMode: (mode) async => saved.add(mode),
      onStart: () async {
        starts += 1;
      },
      loadHost: () async => const SealDeskHostState(
        address: '',
        password: null,
        name: '电脑',
        stopped: true,
      ),
    );
    expect(find.text('共享已停止'), findsOneWidget);
    expect(
        find.byKey(const ValueKey('sealdesk-start-service')), findsOneWidget);
    expect(saved, isEmpty);
    expect(starts, 0);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets(
      'explicit control session opens connect and saves the next choice',
      (tester) async {
    final saved = <String>[];
    await showPage(tester,
        mode: 'control', saveMode: (mode) async => saved.add(mode));
    expect(find.byType(QrImageView), findsNothing);
    expect(find.byKey(const ValueKey('sealdesk-input')), findsOneWidget);
    expect(
      tester
          .widget<ChoiceChip>(find.widgetWithText(ChoiceChip, '连接设备'))
          .selected,
      isTrue,
    );
    await tester.tap(find.text('共享本机'));
    await tester.pumpAndSettle();
    expect(saved, ['host']);
    expect(find.byType(QrImageView), findsOneWidget);
    await tester.pumpWidget(const SizedBox());
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

  testWidgets('relay settings reject invalid URLs without changing the map',
      (tester) async {
    binding.language = 'en';
    String? saved;
    await tester.pumpWidget(MaterialApp(
      home: Builder(
        builder: (context) => Scaffold(
          body: TextButton(
            onPressed: () async {
              saved = await showDialog<String>(
                context: context,
                builder: (context) => const SealDeskRelaySettingsDialog(
                  initialUrl: '',
                  hosting: false,
                ),
              );
            },
            child: const Text('open'),
          ),
        ),
      ),
    ));
    await tester.tap(find.text('open'));
    await tester.pumpAndSettle();
    for (final value in [
      'http://relay.example/map.json',
      'https://',
      'https://user:password@relay.example/map.json',
      'https://relay.example/map.json#fragment',
    ]) {
      await tester.enterText(
          find.byKey(const ValueKey('sealdesk-derp-map-url')), value);
      await tester.tap(find.text('Save'));
      await tester.pumpAndSettle();
      expect(find.text('Enter an HTTPS URL without credentials or a fragment'),
          findsOneWidget);
      expect(saved, isNull);
    }
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
    expect(find.byType(AlertDialog), findsNothing);
    expect(saved, isNull);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('host initialization failure shows retry and recovers a QR code',
      (tester) async {
    binding.language = 'en';
    var failed = true;
    var retries = 0;
    await showPage(tester,
        loadHost: () async => SealDeskHostState(
            address: failed ? '' : address,
            password: 'secret',
            name: 'Computer',
            startError: failed ? 'tailcat: DERP map fetch: timeout' : ''),
        saveMode: (mode) async {
          if (mode == 'host') {
            if (retries++ == 0) {
              throw const FormatException('Sharing could not restart');
            }
            failed = false;
          }
        });
    expect(
        find.text(
            'Cannot reach the relay server. Check the network or relay settings'),
        findsOneWidget);
    expect(find.text('tailcat: DERP map fetch: timeout'), findsNothing);
    await tester.tap(find.text('Error details'));
    await tester.pumpAndSettle();
    expect(find.text('tailcat: DERP map fetch: timeout'), findsOneWidget);
    expect(find.text('Generating pairing code...'), findsNothing);
    expect(find.byType(QrImageView), findsNothing);
    expect(find.byKey(const ValueKey('sealdesk-relay-settings')), findsNothing);
    await tester.tap(find.text('Retry'));
    await tester.pumpAndSettle();
    expect(find.text('Sharing could not restart'), findsOneWidget);
    expect(find.text('Retry'), findsOneWidget);
    await tester.tap(find.text('Retry'));
    await tester.pumpAndSettle();
    expect(find.text('tailcat: DERP map fetch: timeout'), findsNothing);
    expect(
        find.text(
            'Cannot reach the relay server. Check the network or relay settings'),
        findsNothing);
    expect(find.text('Retry'), findsNothing);
    expect(find.text('Sharing'), findsOneWidget);
    expect(find.byType(QrImageView), findsOneWidget);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('relay settings load, save and reset the persisted map',
      (tester) async {
    binding.language = 'en';
    var current = 'https://relay.example/map.json';
    String? saved;
    Future<void> open() async {
      await tester.tap(find.text('open'));
      await tester.pumpAndSettle();
    }

    await tester.pumpWidget(MaterialApp(
      home: Builder(builder: (context) {
        return Scaffold(
          body: TextButton(
            onPressed: () async {
              saved = await showDialog<String>(
                context: context,
                builder: (context) => SealDeskRelaySettingsDialog(
                  initialUrl: current,
                  hosting: true,
                ),
              );
            },
            child: const Text('open'),
          ),
        );
      }),
    ));
    final input = find.byKey(const ValueKey('sealdesk-derp-map-url'));
    await open();
    expect(find.text(current), findsOneWidget);
    expect(
        find.text('Saving restarts sharing and disconnects current sessions.'),
        findsOneWidget);
    await tester.enterText(input, 'https://other.example/derp.json');
    await tester.tap(find.text('Save'));
    await tester.pumpAndSettle();
    expect(saved, 'https://other.example/derp.json');
    current = saved!;
    await open();
    expect(find.text(current), findsOneWidget);
    await tester.tap(find.text('Reset to default'));
    await tester.pumpAndSettle();
    expect(find.text('https://tailcat.dev/derpmap.json'), findsOneWidget);
    expect(saved, 'https://other.example/derp.json');
    await tester.tap(find.text('Save'));
    await tester.pumpAndSettle();
    expect(saved, isEmpty);
    current = '';
    await open();
    expect(find.text('https://tailcat.dev/derpmap.json'), findsOneWidget);
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
    expect(find.byType(AlertDialog), findsNothing);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets(
      'mode switch waits for service confirmation and ignores duplicates',
      (tester) async {
    final confirmation = Completer<void>();
    var calls = 0;
    await showPage(tester, mode: 'host', saveMode: (_) {
      calls++;
      return confirmation.future;
    });
    expect(find.byType(QrImageView), findsOneWidget);
    await tester.tap(find.text('连接设备'));
    await tester.pump();
    await tester.tap(find.text('共享本机'));
    await tester.pump();
    expect(calls, 1);
    expect(find.byType(QrImageView), findsOneWidget);
    confirmation.complete();
    await tester.pumpAndSettle();
    expect(find.byType(QrImageView), findsNothing);
    await tester.tap(find.text('共享本机'));
    await tester.pumpAndSettle();
    expect(calls, 2);
    expect(find.byType(QrImageView), findsOneWidget);
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
    expect(find.text('共享中'), findsOneWidget);
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

  testWidgets('countdown chip counts QR refresh minutes, not a session limit',
      (tester) async {
    var now = 100;
    await showPage(tester, now: () => now);
    Text chipText() => tester.widget<Text>(find.descendant(
        of: find.byKey(const ValueKey('sealdesk-qr-ttl')),
        matching: find.byType(Text)));
    expect(chipText().data, '二维码 10 分钟后刷新');
    expect(find.textContaining('10 分钟内有效'), findsNothing);
    final expiresAt = (await copyPairing(tester)).expiresAt;
    now = 160;
    await tester.pump(const Duration(seconds: 1));
    expect(chipText().data, '二维码 9 分钟后刷新');
    now = 641;
    await tester.pump(const Duration(seconds: 1));
    expect(chipText().data, '二维码 1 分钟后刷新');
    expect((await copyPairing(tester)).expiresAt, expiresAt);
    binding.language = 'en';
    now = 100;
    await tester.pump(const Duration(seconds: 1));
    expect(chipText().data, 'QR refreshes in 10 min');
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('mask hides QR and restores it on demand', (tester) async {
    await showPage(tester);
    await tester.tap(find.text('遮挡二维码'));
    await tester.pump();
    expect(find.byType(QrImageView), findsNothing);
    expect(find.text('点按显示'), findsOneWidget);
    expect(
        find.descendant(
            of: find.byKey(const ValueKey('sealdesk-qr-mask')),
            matching: find.byType(IconButton)),
        findsNothing);
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
    expect(find.byKey(const ValueKey('sealdesk-copy')), findsNothing);
    expect(find.text('刷新密码'), findsNothing);
    expect(find.text('遮挡二维码'), findsNothing);
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
    expect(
        find.text(
            'Pairing code has expired. Ask the other person to refresh the QR code'),
        findsOneWidget);
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

  testWidgets('relay chip stays under the QR and startup errors use retry',
      (tester) async {
    binding.language = 'en';
    RelayStatus? relay = RelayStatus.fromJson(
        '{"running":true,"state":"reachable","probe_rtt_ms":42}');
    var addr = address;
    var startError = '';
    await showPage(tester,
        loadHost: () async => SealDeskHostState(
            address: addr,
            password: 'secret',
            name: '电脑',
            relayStatus: relay,
            startError: startError));
    expect(find.byType(QrImageView), findsOneWidget);
    expect(find.text('Sharing'), findsOneWidget);
    expect(find.text('Relay reachable (42 ms)'), findsOneWidget);
    expect(find.byKey(const ValueKey('sealdesk-relay')), findsOneWidget);

    relay = RelayStatus.fromJson(
        '{"running":true,"state":"unreachable","last_error":"dial timeout"}');
    await tester.pump(const Duration(seconds: 1));
    await tester.pump();
    expect(find.byType(QrImageView), findsOneWidget);
    expect(find.text('Sharing'), findsOneWidget);
    expect(find.text('Relay unreachable; scanning will likely time out'),
        findsOneWidget);
    expect(find.text('dial timeout'), findsNothing);
    await tester.tap(find.text('Error details'));
    await tester.pumpAndSettle();
    expect(find.text('dial timeout'), findsOneWidget);
    expect(find.text('Relay reachable (42 ms)'), findsNothing);

    addr = '';
    startError = 'DERP relay/bootstrap: connection refused';
    relay = RelayStatus.fromJson(
        '{"running":false,"state":"stopped","init_stage":"relay","init_error":"DERP relay/bootstrap: connection refused"}');
    await tester.pump(const Duration(seconds: 1));
    await tester.pump();
    expect(find.byType(QrImageView), findsNothing);
    expect(find.text('DERP relay/bootstrap: connection refused'), findsNothing);
    expect(
        find.text(
            'Cannot reach the relay server. Check the network or relay settings'),
        findsOneWidget);
    expect(find.text('Retry'), findsOneWidget);
    expect(find.textContaining('Generating pairing code'), findsNothing);
    expect(find.byKey(const ValueKey('sealdesk-relay')), findsNothing);

    startError = '';
    relay = RelayStatus.fromJson(
        '{"running":false,"state":"stopped","init_stage":"start","init_error":"bind"}');
    await tester.pump(const Duration(seconds: 1));
    await tester.pump();
    expect(find.text('Generating pairing code...'), findsOneWidget);
    expect(find.text('start: bind'), findsNothing);
    expect(find.text('Retry'), findsNothing);
    expect(find.byKey(const ValueKey('sealdesk-relay')), findsNothing);

    addr = address;
    relay = RelayStatus.fromJson('{"running":true,"state":"connecting"}');
    await tester.pump(const Duration(seconds: 1));
    await tester.pump();
    expect(find.byType(QrImageView), findsOneWidget);
    expect(find.text('Sharing'), findsOneWidget);
    expect(find.text('Checking relay...'), findsOneWidget);
    expect(find.text('Generating pairing code...'), findsNothing);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('relay status follows zh-cn', (tester) async {
    binding.language = 'zh-cn';
    RelayStatus? relay = RelayStatus.fromJson(
        '{"running":true,"state":"reachable","probe_rtt_ms":42}');
    await showPage(tester,
        loadHost: () async => SealDeskHostState(
            address: address,
            password: 'secret',
            name: '电脑',
            relayStatus: relay));
    expect(find.text('中继可达 (42 ms)'), findsOneWidget);

    relay = RelayStatus.fromJson(
        '{"running":true,"state":"unreachable","last_error":"dial timeout"}');
    await tester.pump(const Duration(seconds: 1));
    await tester.pump();
    expect(find.text('中继不可达，扫码连接很可能会超时'), findsOneWidget);
    expect(find.text('dial timeout'), findsNothing);
    expect(find.text('错误详情'), findsOneWidget);
    expect(find.text('中继可达 (42 ms)'), findsNothing);

    relay = RelayStatus.fromJson('{"state":"connecting"}');
    await tester.pump(const Duration(seconds: 1));
    await tester.pump();
    expect(find.text('正在检查中继…'), findsOneWidget);
    expect(find.text('中继不可达，扫码连接很可能会超时'), findsNothing);
    await tester.pumpWidget(const SizedBox());
  });

  Future<void> _shot(WidgetTester tester, String name) async {
    final dir = Platform.environment['SEALDESK_QR_SHOT'];
    if (dir == null || dir.isEmpty) return;
    await tester.pumpAndSettle();
    final boundary = tester.renderObject<RenderRepaintBoundary>(
        find.byKey(const ValueKey('page-screenshot')));
    await tester.runAsync(() async {
      final image = await boundary.toImage(pixelRatio: 2);
      final data = await image.toByteData(format: ui.ImageByteFormat.png);
      image.dispose();
      final file = File('$dir/$name.png');
      await file.parent.create(recursive: true);
      await file.writeAsBytes(data!.buffer.asUint8List());
    });
  }

  testWidgets('desktop QR card screenshots', (tester) async {
    await showPage(tester, sidePane: true, surface: const Size(1100, 720));
    expect(find.byType(QrImageView), findsOneWidget);
    await _shot(tester, 'qr-desktop');
    await tester.tap(find.byKey(const ValueKey('sealdesk-hide')));
    await tester.pump();
    await _shot(tester, 'qr-masked');
    await tester.tap(find.byKey(const ValueKey('sealdesk-qr-mask')));
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('sealdesk-qr-tap')));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 300));
    expect(find.byKey(const ValueKey('sealdesk-qr-full')), findsOneWidget);
    await tester.pumpWidget(const SizedBox());

    await showPage(tester, sidePane: true, surface: const Size(760, 640));
    expect(
        find.byKey(const ValueKey('sealdesk-side-collapsed')), findsOneWidget);
    await _shot(tester, 'qr-collapsed');
    await tester.pumpWidget(const SizedBox());

    await showPage(tester, surface: const Size(412, 915));
    expect(find.byType(QrImageView), findsOneWidget);
    await _shot(tester, 'qr-mobile');
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('share icon tips do not cover the relay status', (tester) async {
    binding.language = 'en';
    await showPage(
      tester,
      sidePane: true,
      surface: const Size(1400, 900),
      resetAddress: () async {},
      loadHost: () async => SealDeskHostState(
        address: address,
        password: 'secret',
        name: 'pc',
        relayStatus: RelayStatus.fromJson(
            '{"running":true,"state":"reachable","probe_rtt_ms":42}'),
      ),
    );
    await tester.pump();
    await tester.pump();
    expect(find.text('Relay reachable (42 ms)'), findsOneWidget);

    final gesture = await tester.createGesture(kind: PointerDeviceKind.mouse);
    await gesture.addPointer(location: const Offset(2, 2));
    addTearDown(gesture.removePointer);

    Future<void> expectTip(String key, String message) async {
      final icon = find.byKey(ValueKey(key));
      await gesture.moveTo(const Offset(2, 2));
      await tester.pump();
      await gesture.moveTo(tester.getCenter(icon));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 20));
      final tip = find.ancestor(
        of: find.text(message),
        matching: find.byType(Container),
      );
      expect(tip, findsWidgets);
      final tipRect = tester.getRect(tip.first);
      final relayRect =
          tester.getRect(find.byKey(const ValueKey('sealdesk-relay')));
      final ttlRect =
          tester.getRect(find.byKey(const ValueKey('sealdesk-qr-ttl')));
      expect(tipRect.overlaps(relayRect), isFalse);
      expect(tipRect.overlaps(ttlRect), isFalse);
    }

    await expectTip('sealdesk-refresh', 'Refresh password');
    await expectTip('sealdesk-reset-address', 'Reset address');
    await expectTip('sealdesk-hide', 'Hide QR code');
    await gesture.moveTo(const Offset(2, 2));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 100));
    await tester.pump(const Duration(milliseconds: 75));
    expect(find.text('Refresh password'), findsNothing);
    expect(find.text('Reset address'), findsNothing);
    expect(find.text('Hide QR code'), findsNothing);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('sidebar stop service calls onStop once while sharing',
      (tester) async {
    var stops = 0;
    Future<void> onStop() async {
      stops++;
    }

    Future<SealDeskHostState> loadHost() async => const SealDeskHostState(
          address: address,
          password: 'secret',
          name: '电脑',
        );

    await showPage(
      tester,
      sidePane: true,
      surface: const Size(1100, 720),
      onStop: onStop,
      loadHost: loadHost,
    );
    expect(find.byKey(const ValueKey('sealdesk-stop-service')), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('sealdesk-stop-service')));
    await tester.pump();
    expect(stops, 1);

    stops = 0;
    await showPage(
      tester,
      sidePane: true,
      surface: const Size(760, 640),
      onStop: onStop,
      loadHost: loadHost,
    );
    expect(
        find.byKey(const ValueKey('sealdesk-side-collapsed')), findsOneWidget);
    expect(find.byKey(const ValueKey('sealdesk-stop-service')), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('sealdesk-stop-service')));
    await tester.pump();
    expect(stops, 1);
    await tester.pumpWidget(const SizedBox());
  });
}
