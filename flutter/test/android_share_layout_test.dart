import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sealdesk/common/sealdesk_share_settings.dart';
import 'package:sealdesk/desktop/pages/sealdesk_home_page.dart';
import 'package:sealdesk/generated_bridge.dart';
import 'package:sealdesk/models/platform_model.dart';
import 'package:sealdesk/models/server_model.dart';

const address = 'tcABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdef_-';

class _I18n extends Fake implements SealdeskImpl {
  @override
  String translate(
      {required String name, required String locale, dynamic hint}) {
    return name;
  }
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  platformFFI.setBindingForTesting(_I18n());

  Future<void> show(
    WidgetTester tester, {
    required Size size,
    double textScale = 1,
  }) async {
    await tester.binding.setSurfaceSize(size);
    addTearDown(() => tester.binding.setSurfaceSize(null));
    await tester.pumpWidget(MaterialApp(
      builder: (context, child) {
        final data = MediaQuery.of(context);
        return MediaQuery(
          data: data.copyWith(textScaler: TextScaler.linear(textScale)),
          child: child!,
        );
      },
      home: Scaffold(
        body: SealDeskHomePage(
          hostOnly: true,
          shareColumn: true,
          initialMode: 'host',
          loadHost: () async => const SealDeskHostState(
            address: address,
            password: 'secret',
            name: 'pixel',
          ),
          refreshPassword: () async {},
          connect: (_) async {},
          saveMode: (_) async {},
          now: () => 100,
          onStart: () async {},
          onStop: () async {},
          permissionBanner: const Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text('Permissions'),
              Text('Screen Capture'),
              Text('Input Control'),
              Text('Transfer file'),
            ],
          ),
          shareSettings: SealDeskShareSettingsView(
            approveMode: 'password-click',
            verificationMethod: kUseBothPasswords,
            otpLength: '8',
            numeric: false,
            approveFixed: false,
            numericFixed: false,
            permanentPasswordDisabled: false,
            passwordValue: 'secret',
            onApprove: (_) async {},
            onVerification: (_) async {},
            onSetPermanent: () {},
            onSetOtpLength: () {},
            onToggleNumeric: () {},
          ),
        ),
      ),
    ));
    await tester.pump();
    await tester.pump();
  }

  Rect group(WidgetTester tester, String key) =>
      tester.getRect(find.byKey(ValueKey(key)));

  void expectOrdered(WidgetTester tester) {
    const keys = [
      'sealdesk-share-qr-card',
      'sealdesk-share-service',
      'sealdesk-share-permissions',
      'sealdesk-share-security',
    ];
    final rects = [for (final key in keys) group(tester, key)];
    for (var i = 0; i < rects.length; i++) {
      expect(rects[i].width, greaterThan(0), reason: keys[i]);
      expect(rects[i].height, greaterThan(0), reason: keys[i]);
      for (var j = i + 1; j < rects.length; j++) {
        expect(rects[i].overlaps(rects[j]), isFalse,
            reason: '${keys[i]} overlaps ${keys[j]}');
      }
      if (i > 0) {
        expect(rects[i].top + 0.5, greaterThanOrEqualTo(rects[i - 1].bottom),
            reason: '${keys[i]} is not below ${keys[i - 1]}');
      }
    }
  }

  Future<void> expectScrollsToEnd(WidgetTester tester, Size size) async {
    final scrollable = find.descendant(
      of: find.byKey(const ValueKey('sealdesk-share-scroll')),
      matching: find.byType(Scrollable),
    );
    final states = tester.stateList<ScrollableState>(scrollable).toList();
    expect(states, isNotEmpty);
    states.sort((a, b) =>
        a.position.maxScrollExtent.compareTo(b.position.maxScrollExtent));
    final position = states.last.position;
    expect(position.maxScrollExtent, greaterThan(0));
    position.jumpTo(position.maxScrollExtent);
    await tester.pump();
    final viewport =
        tester.getRect(find.byKey(const ValueKey('sealdesk-share-scroll')));
    final security = group(tester, 'sealdesk-share-security');
    expect(security.bottom, lessThanOrEqualTo(viewport.bottom + 1));
    expect(security.bottom, greaterThan(viewport.top));
    expect(security.top, lessThan(viewport.bottom));
    expect(viewport.width, lessThanOrEqualTo(size.width + 1));
    expectOrdered(tester);
  }

  testWidgets(
      'share column does not overlap on small, landscape, and large text',
      (tester) async {
    const cases = <(Size, double)>[
      (Size(320, 568), 1),
      (Size(640, 320), 1),
      (Size(320, 568), 2),
      (Size(640, 320), 2),
    ];
    for (final item in cases) {
      await show(tester, size: item.$1, textScale: item.$2);
      expect(find.byKey(const ValueKey('sealdesk-qr')), findsOneWidget);
      expect(find.text(address), findsOneWidget);
      expect(find.byKey(const ValueKey('sealdesk-service-switch')),
          findsOneWidget);
      expect(find.text('Permissions'), findsOneWidget);
      expect(find.text('Access method'), findsOneWidget);
      expect(find.text('Password type'), findsOneWidget);
      expect(find.text('Password value'), findsOneWidget);
      expect(find.text('secret'), findsOneWidget);
      expect(
          find.descendant(
            of: find.byKey(const ValueKey('sealdesk-share-qr-card')),
            matching: find.text('Password value'),
          ),
          findsNothing);
      expect(
          find.descendant(
            of: find.byKey(const ValueKey('sealdesk-share-security')),
            matching: find.byKey(const ValueKey('sealdesk-approve')),
          ),
          findsOneWidget);
      expect(
          find.descendant(
            of: find.byKey(const ValueKey('sealdesk-share-security')),
            matching: find.byKey(const ValueKey('sealdesk-verify')),
          ),
          findsOneWidget);
      expect(find.byType(Positioned), findsNothing);
      expectOrdered(tester);
      await expectScrollsToEnd(tester, item.$1);
      await tester.pumpWidget(const SizedBox());
      await tester.pump();
    }
  });
}
