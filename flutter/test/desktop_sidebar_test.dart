import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qr_flutter/qr_flutter.dart';
import 'package:sealdesk/common/sealdesk_share_settings.dart';
import 'package:sealdesk/desktop/pages/sealdesk_home_page.dart';
import 'package:sealdesk/desktop/widgets/desktop_sidebar.dart';
import 'package:sealdesk/generated_bridge.dart';
import 'package:sealdesk/models/platform_model.dart';
import 'package:sealdesk/models/server_model.dart';

const _address = 'tcABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdef_-';

class _Binding extends Fake implements SealdeskImpl {
  @override
  String translate(
          {required String name, required String locale, dynamic hint}) =>
      name;
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  platformFFI.setBindingForTesting(_Binding());

  test('sidebar width stays inside the window', () {
    expect(clampDesktopSidebarWidth(900, 800), kDesktopSidebarMaxWidth);
    expect(clampDesktopSidebarWidth(10, 800), kDesktopSidebarMinWidth);
    expect(parseDesktopSidebarWidth(''), isNull);
    expect(parseDesktopSidebarWidth('360.0'), 360);
    expect(clampDesktopSidebarWidth(400, 500),
        500 - kDesktopSidebarDividerExtent - kDesktopSidebarRightMinWidth);
    final narrow = clampDesktopSidebarWidth(400, 400);
    expect(narrow, lessThan(kDesktopSidebarMinWidth));
    expect(narrow,
        400 - kDesktopSidebarDividerExtent - kDesktopSidebarRightMinWidth);
  });

  testWidgets('short window scrolls the share sidebar to the bottom',
      (tester) async {
    final controller = ScrollController();
    addTearDown(controller.dispose);
    await tester.binding.setSurfaceSize(const Size(800, 420));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    await _pumpSplit(tester, controller: controller, savedWidth: '');
    await tester.pump();
    await tester.pump(const Duration(seconds: 1));
    await tester.pump();
    expect(find.byType(QrImageView), findsOneWidget);
    final qrTop = tester.getTopLeft(find.byType(QrImageView)).dy;
    final settingsTop = tester
        .getTopLeft(find.byKey(const ValueKey('sealdesk-share-settings')))
        .dy;
    expect(settingsTop, greaterThan(qrTop));
    final bottom = find.byKey(const ValueKey('sealdesk-otp-numeric'));
    expect(_fullyVisible(tester, bottom), isFalse);
    await tester.scrollUntilVisible(
      bottom,
      80,
      scrollable: find.descendant(
        of: find.byKey(const ValueKey('desktop-sidebar-scroll')),
        matching: find.byType(Scrollable),
      ),
    );
    expect(_fullyVisible(tester, bottom), isTrue);
    expect(controller.offset, greaterThan(0));
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('dragging the divider clamps and restores the width',
      (tester) async {
    final controller = ScrollController();
    addTearDown(controller.dispose);
    await tester.binding.setSurfaceSize(const Size(800, 600));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    final saved = <String>[];
    await _pumpSplit(tester,
        controller: controller, savedWidth: '', onSave: saved.add);
    await tester.pump();
    await tester.pump(const Duration(seconds: 1));
    await tester.pump();
    expect(_paneWidth(tester), kDesktopSidebarDefaultWidth);
    expect(_fullyVisible(tester, find.byType(QrImageView)), isTrue);
    final cursor = tester.widget<MouseRegion>(find.ancestor(
        of: find.byKey(const ValueKey('desktop-sidebar-divider')),
        matching: find.byType(MouseRegion)));
    expect(cursor.cursor, SystemMouseCursors.resizeLeftRight);

    await tester.drag(find.byKey(const ValueKey('desktop-sidebar-divider')),
        const Offset(500, 0));
    await tester.pump();
    expect(_paneWidth(tester), kDesktopSidebarMaxWidth);
    expect(saved.last, kDesktopSidebarMaxWidth.toStringAsFixed(1));

    await tester.drag(find.byKey(const ValueKey('desktop-sidebar-divider')),
        const Offset(-1000, 0));
    await tester.pump();
    expect(_paneWidth(tester), kDesktopSidebarMinWidth);
    expect(saved.last, kDesktopSidebarMinWidth.toStringAsFixed(1));

    await tester.pumpWidget(const SizedBox());
    await _pumpSplit(tester,
        controller: controller, savedWidth: saved.last, onSave: saved.add);
    await tester.pump();
    expect(_paneWidth(tester), kDesktopSidebarMinWidth);

    await tester.binding.setSurfaceSize(const Size(500, 600));
    await tester.pumpWidget(const SizedBox());
    await _pumpSplit(tester,
        controller: controller, savedWidth: '400', onSave: saved.add);
    await tester.pump();
    final narrowed =
        500 - kDesktopSidebarDividerExtent - kDesktopSidebarRightMinWidth;
    expect(_paneWidth(tester), narrowed);
    await tester.drag(find.byKey(const ValueKey('desktop-sidebar-divider')),
        const Offset(300, 0));
    await tester.pump();
    expect(_paneWidth(tester), narrowed);
    expect(saved.last, narrowed.toStringAsFixed(1));
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('rtl drag widens the pane toward the pointer', (tester) async {
    final controller = ScrollController();
    addTearDown(controller.dispose);
    await tester.binding.setSurfaceSize(const Size(800, 600));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    await tester.pumpWidget(MaterialApp(
      home: Directionality(
        textDirection: TextDirection.rtl,
        child: Scaffold(
          body: DesktopSidebarScaffold(
            savedWidth: '',
            defaultWidth: kDesktopSidebarDefaultWidth,
            onSave: (_) {},
            scrollController: controller,
            leftChildren: const [SizedBox(height: 20)],
            right: const ColoredBox(color: Color(0xFFF7F8FA)),
          ),
        ),
      ),
    ));
    await tester.pump();
    final pane =
        tester.getRect(find.byKey(const ValueKey('desktop-sidebar-pane')));
    final divider =
        tester.getRect(find.byKey(const ValueKey('desktop-sidebar-divider')));
    expect(pane.left, greaterThanOrEqualTo(divider.right - 0.1));
    expect(pane.width, kDesktopSidebarDefaultWidth);

    await tester.drag(find.byKey(const ValueKey('desktop-sidebar-divider')),
        const Offset(-60, 0));
    await tester.pump();
    expect(_paneWidth(tester), kDesktopSidebarDefaultWidth + 60);

    await tester.drag(find.byKey(const ValueKey('desktop-sidebar-divider')),
        const Offset(60, 0));
    await tester.pump();
    expect(_paneWidth(tester), kDesktopSidebarDefaultWidth);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('desktop sidebar screenshots', (tester) async {
    final dir = Platform.environment['SEALDESK_SIDE_SHOT'];
    if (dir == null || dir.isEmpty) return;
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    addTearDown(() => debugDefaultTargetPlatformOverride = null);
    final controller = ScrollController();
    addTearDown(controller.dispose);
    addTearDown(() => tester.binding.setSurfaceSize(null));

    await tester.binding.setSurfaceSize(const Size(800, 600));
    await _pumpSplit(tester,
        controller: controller, savedWidth: '', shot: true);
    await tester.pump();
    await tester.pump(const Duration(seconds: 1));
    await tester.pump();
    expect(find.byType(QrImageView), findsOneWidget);
    await _shot(tester, dir, 'desktop-default');

    await tester.binding.setSurfaceSize(const Size(800, 420));
    await tester.pump();
    await _shot(tester, dir, 'desktop-short');

    await tester.binding.setSurfaceSize(const Size(800, 600));
    await tester.pump();
    await tester.drag(find.byKey(const ValueKey('desktop-sidebar-divider')),
        const Offset(500, 0));
    await tester.pump();
    expect(_paneWidth(tester), kDesktopSidebarMaxWidth);
    await _shot(tester, dir, 'desktop-wide');
    await tester.pumpWidget(const SizedBox());
    debugDefaultTargetPlatformOverride = null;
  });
}

Future<void> _pumpSplit(
  WidgetTester tester, {
  required ScrollController controller,
  required String savedWidth,
  ValueChanged<String>? onSave,
  bool shot = false,
}) {
  final split = DesktopSidebarScaffold(
    savedWidth: savedWidth,
    defaultWidth: kDesktopSidebarDefaultWidth,
    onSave: onSave ?? (_) {},
    scrollController: controller,
    leftChildren: [_sharePane()],
    right: const ColoredBox(
      color: Color(0xFFF7F8FA),
      child: Padding(
        padding: EdgeInsets.all(28),
        child: Align(
          alignment: Alignment.topLeft,
          child: Text('Connect to a device',
              style: TextStyle(fontSize: 18, fontWeight: FontWeight.w600)),
        ),
      ),
    ),
  );
  return tester.pumpWidget(MaterialApp(
    home: Scaffold(
      body: shot
          ? RepaintBoundary(
              key: const ValueKey('page-screenshot'), child: split)
          : split,
    ),
  ));
}

Widget _sharePane() {
  return SealDeskHomePage(
    sidePane: true,
    hostOnly: true,
    initialMode: 'host',
    loadHost: () async => const SealDeskHostState(
        address: _address, password: 'secret', name: '电脑'),
    refreshPassword: () async {},
    connect: (_) async {},
    saveMode: (_) async {},
    now: () => 100,
    shareSettings: SealDeskShareSettingsView(
      approveMode: 'password-click',
      verificationMethod: kUseTemporaryPassword,
      otpLength: '6',
      numeric: true,
      approveFixed: false,
      numericFixed: false,
      permanentPasswordDisabled: false,
      onApprove: (_) async {},
      onVerification: (_) async {},
      onSetPermanent: () {},
      onSetOtpLength: () {},
      onToggleNumeric: () {},
    ),
  );
}

double _paneWidth(WidgetTester tester) =>
    tester.getSize(find.byKey(const ValueKey('desktop-sidebar-pane'))).width;

bool _fullyVisible(WidgetTester tester, Finder target) {
  final scroll =
      tester.getRect(find.byKey(const ValueKey('desktop-sidebar-scroll')));
  final rect = tester.getRect(target);
  return rect.top >= scroll.top - 0.5 && rect.bottom <= scroll.bottom + 0.5;
}

Future<void> _shot(WidgetTester tester, String dir, String name) async {
  await tester.pump();
  final boundary = tester.renderObject<RenderRepaintBoundary>(
      find.byKey(const ValueKey('page-screenshot')));
  await tester.runAsync(() async {
    final image = await boundary.toImage(pixelRatio: 1);
    final data = await image.toByteData(format: ui.ImageByteFormat.png);
    image.dispose();
    final file = File('$dir/$name.png');
    await file.parent.create(recursive: true);
    await file.writeAsBytes(data!.buffer.asUint8List());
  });
}
