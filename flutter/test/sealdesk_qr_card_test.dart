import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qr_flutter/qr_flutter.dart';
import 'package:sealdesk/desktop/pages/sealdesk_home_page.dart';
import 'package:sealdesk/generated_bridge.dart';
import 'package:sealdesk/models/platform_model.dart';

const address = 'tcABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdef_-';

class _I18n extends Fake implements SealdeskImpl {
  @override
  String translate(
          {required String name, required String locale, dynamic hint}) =>
      name;
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  platformFFI.setBindingForTesting(_I18n());

  Future<void> show(
    WidgetTester tester, {
    required Size size,
    required SealDeskHostState Function() host,
    bool sidePane = false,
    bool hostOnly = true,
    int Function()? now,
  }) async {
    final page = sidePane
        ? Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              SizedBox(
                width: size.width < 800 ? 200 : 300,
                child: SealDeskHomePage(
                  sidePane: true,
                  hostOnly: true,
                  initialMode: 'host',
                  loadHost: () async => host(),
                  refreshPassword: () async {},
                  connect: (_) async {},
                  saveMode: (_) async {},
                  now: now ?? () => 100,
                  resetAddress: () async {},
                ),
              ),
              const VerticalDivider(width: 1),
              const Expanded(child: SizedBox.expand()),
            ],
          )
        : SealDeskHomePage(
            hostOnly: hostOnly,
            initialMode: 'host',
            loadHost: () async => host(),
            refreshPassword: () async {},
            connect: (_) async {},
            saveMode: (_) async {},
            now: now ?? () => 100,
          );
    await tester.pumpWidget(MaterialApp(
      home: Scaffold(
        body: MediaQuery(
          data: MediaQueryData(size: size),
          child: page,
        ),
      ),
    ));
    await tester.pump();
  }

  SealDeskHostState running({int connectedCount = 0}) => SealDeskHostState(
      address: address,
      password: 'secret',
      name: 'pixel',
      connectedCount: connectedCount);

  testWidgets('tap shows a full-size QR and the code itself has no controls',
      (tester) async {
    await tester.binding.setSurfaceSize(const Size(412, 915));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    await show(tester, size: const Size(412, 915), host: () => running());
    expect(find.byKey(const ValueKey('sealdesk-scam-line')), findsOneWidget);
    final scam = tester.widget<Text>(find.descendant(
        of: find.byKey(const ValueKey('sealdesk-scam-line')),
        matching: find.byType(Text)));
    expect(scam.maxLines, 2);
    expect(scam.data, 'Stranger asks you to share? Hang up');
    expect(find.byKey(const ValueKey('sealdesk-qr')), findsOneWidget);
    expect(
        find.descendant(
            of: find.byKey(const ValueKey('sealdesk-qr-tap')),
            matching: find.byType(IconButton)),
        findsNothing);
    expect(
        find.descendant(
            of: find.byKey(const ValueKey('sealdesk-qr-tap')),
            matching: find.byType(TextButton)),
        findsNothing);
    final qr = tester.getRect(find.byKey(const ValueKey('sealdesk-qr')));
    final copy = tester.getRect(find.byKey(const ValueKey('sealdesk-copy')));
    expect(copy.top, greaterThanOrEqualTo(qr.bottom - 1));

    await tester.tap(find.byKey(const ValueKey('sealdesk-qr-tap')));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 300));
    expect(find.byKey(const ValueKey('sealdesk-qr-full')), findsOneWidget);
    expect(find.byKey(const ValueKey('sealdesk-qr')), findsOneWidget);
    final full = tester.getSize(find.byKey(const ValueKey('sealdesk-qr-full')));
    expect(full.width, closeTo(412, 1));
    expect(full.height, closeTo(412, 1));
    final fullRect =
        tester.getRect(find.byKey(const ValueKey('sealdesk-qr-full')));
    final close = tester.getRect(find.byKey(const ValueKey('sealdesk-qr-close')));
    expect(fullRect.overlaps(close), isFalse);
    expect(close.top, greaterThanOrEqualTo(fullRect.bottom - 1));
    await tester.tap(find.byKey(const ValueKey('sealdesk-qr-close')));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 300));
    expect(find.byKey(const ValueKey('sealdesk-qr-full')), findsNothing);
  });

  testWidgets('a connected peer masks the QR until it is shown by hand',
      (tester) async {
    var count = 0;
    await show(tester,
        size: const Size(412, 915), host: () => running(connectedCount: count));
    expect(find.byType(QrImageView), findsOneWidget);

    count = 1;
    await tester.pump(const Duration(seconds: 1));
    await tester.pump();
    expect(find.byType(QrImageView), findsNothing);
    expect(find.text('Tap to show'), findsOneWidget);

    count = 2;
    await tester.pump(const Duration(seconds: 1));
    await tester.pump();
    expect(find.byType(QrImageView), findsNothing);

    await tester.tap(find.byKey(const ValueKey('sealdesk-qr-mask')));
    await tester.pump();
    expect(find.byType(QrImageView), findsOneWidget);

    count = 2;
    await tester.pump(const Duration(seconds: 1));
    await tester.pump();
    expect(find.byType(QrImageView), findsOneWidget);

    count = 0;
    await tester.pump(const Duration(seconds: 1));
    await tester.pump();
    expect(find.byType(QrImageView), findsOneWidget);

    count = 1;
    await tester.pump(const Duration(seconds: 1));
    await tester.pump();
    expect(find.byType(QrImageView), findsNothing);
    expect(find.text('Tap to show'), findsOneWidget);
  });

  testWidgets('opening while already connected starts masked', (tester) async {
    await show(tester,
        size: const Size(412, 800), host: () => running(connectedCount: 1));
    expect(find.byType(QrImageView), findsNothing);
    expect(find.text('Tap to show'), findsOneWidget);
    await tester.tap(find.text('Show QR code'));
    await tester.pump();
    expect(find.byType(QrImageView), findsOneWidget);
  });

  testWidgets('desktop side pane replaces the ID card and collapses under 800',
      (tester) async {
    await show(tester,
        size: const Size(1100, 720), sidePane: true, host: () => running());
    expect(find.byType(QrImageView), findsOneWidget);
    expect(find.byKey(const ValueKey('sealdesk-copy')), findsOneWidget);
    expect(find.byKey(const ValueKey('sealdesk-refresh')), findsOneWidget);
    expect(find.byKey(const ValueKey('sealdesk-hide')), findsOneWidget);
    expect(find.byType(TextField), findsNothing);
    expect(find.text('Copy pairing code'), findsNothing);
    expect(find.text('Your Desktop'), findsNothing);
    expect(find.byKey(const ValueKey('sealdesk-scam-line')), findsOneWidget);
    expect(
        find.descendant(
            of: find.byKey(const ValueKey('sealdesk-qr-tap')),
            matching: find.byType(Icon)),
        findsNothing);
    final qr = tester.getRect(find.byKey(const ValueKey('sealdesk-qr')));
    expect(qr.right, lessThanOrEqualTo(300));
    final copy = tester.getRect(find.byKey(const ValueKey('sealdesk-copy')));
    expect(copy.bottom, lessThanOrEqualTo(qr.top + 1));

    await tester.tap(find.byKey(const ValueKey('sealdesk-hide')));
    await tester.pump();
    expect(find.byType(QrImageView), findsNothing);
    expect(find.text('Tap to show'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('sealdesk-qr-mask')));
    await tester.pump();
    expect(find.byType(QrImageView), findsOneWidget);

    await tester.tap(find.byKey(const ValueKey('sealdesk-qr-tap')));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 300));
    expect(find.byKey(const ValueKey('sealdesk-qr-full')), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('sealdesk-qr-close')));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 300));

    await show(tester,
        size: const Size(760, 640), sidePane: true, host: () => running());
    expect(
        find.byKey(const ValueKey('sealdesk-side-collapsed')), findsOneWidget);
    expect(find.text('Sharing'), findsOneWidget);
    expect(find.byType(QrImageView), findsNothing);
  });
}
