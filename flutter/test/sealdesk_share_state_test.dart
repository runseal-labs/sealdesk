import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sealdesk/common/sealdesk_share_settings.dart';
import 'package:sealdesk/desktop/pages/sealdesk_home_page.dart';
import 'package:sealdesk/desktop/pages/sealdesk_native_home_page.dart';
import 'package:sealdesk/generated_bridge.dart';
import 'package:sealdesk/models/platform_model.dart';
import 'package:sealdesk/models/server_model.dart';

const address = 'tcABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdef_-';

class _I18n extends Fake implements SealdeskImpl {
  @override
  String translate(
      {required String name, required String locale, dynamic hint}) {
    final match = RegExp(r'\{(.*?)\}').firstMatch(name);
    if (match == null) return name;
    return name.replaceFirst(match.group(0)!, match.group(1)!);
  }
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  platformFFI.setBindingForTesting(_I18n());

  Future<void> show(
    WidgetTester tester, {
    required Size size,
    required SealDeskHostState host,
    bool hostOnly = true,
    Future<void> Function()? onStart,
    Future<void> Function()? onStop,
    Widget? permissionBanner,
    Widget? shareSettings,
    Widget Function(bool host)? lanPanelBuilder,
    int Function()? now,
  }) async {
    await tester.binding.setSurfaceSize(size);
    addTearDown(() => tester.binding.setSurfaceSize(null));
    await tester.pumpWidget(MaterialApp(
      home: Scaffold(
        body: SealDeskHomePage(
          hostOnly: hostOnly,
          initialMode: 'host',
          loadHost: () async => host,
          refreshPassword: () async {},
          connect: (_) async {},
          saveMode: (_) async {},
          now: now ?? () => 100,
          onStart: onStart,
          onStop: onStop,
          permissionBanner: permissionBanner,
          shareSettings: shareSettings,
          lanPanelBuilder: lanPanelBuilder,
        ),
      ),
    ));
    await tester.pump();
  }

  SealDeskHostState running() => const SealDeskHostState(
      address: address, password: 'secret', name: 'pixel');

  test('desktop does not force stopped when the service option is clear', () {
    expect(
        sealDeskHostStopped(
            serviceStopOption: false,
            android: false,
            androidServiceStarted: false),
        isFalse);
    expect(
        sealDeskHostStopped(
            serviceStopOption: true,
            android: false,
            androidServiceStarted: true),
        isTrue);
    expect(
        sealDeskHostStopped(
            serviceStopOption: false,
            android: true,
            androidServiceStarted: false),
        isTrue);
    expect(
        sealDeskHostStopped(
            serviceStopOption: false,
            android: true,
            androidServiceStarted: true),
        isFalse);
  });

  testWidgets('S-AC1 stopped hides pairing, expiry, LAN and relay',
      (tester) async {
    var lanBuilt = false;
    await show(
      tester,
      size: const Size(360, 640),
      host: const SealDeskHostState(
          address: address, password: 'secret', name: 'pixel', stopped: true),
      onStart: () async {},
      lanPanelBuilder: (_) {
        lanBuilt = true;
        return const Text('Starting LAN sharing...');
      },
      shareSettings: const Text('Relay settings'),
    );
    expect(find.byKey(const ValueKey('sealdesk-copy')), findsNothing);
    expect(find.text('Refresh password'), findsNothing);
    expect(find.text('Hide QR code'), findsNothing);
    expect(
        find.textContaining('QR codes are valid for 10 minutes'), findsNothing);
    expect(find.text('Starting LAN sharing...'), findsNothing);
    expect(lanBuilt, isFalse);
    expect(find.byKey(const ValueKey('sealdesk-relay-settings')), findsNothing);
    expect(find.text('Sharing stopped'), findsOneWidget);
    expect(find.text('Relay settings'), findsOneWidget);
  });

  testWidgets('S-AC2 start stays on the first 360x640 screen', (tester) async {
    await show(
      tester,
      size: const Size(360, 640),
      host: const SealDeskHostState(
          address: address, password: 'secret', name: 'pixel', stopped: true),
      onStart: () async {},
    );
    final button = find.byKey(const ValueKey('sealdesk-start-service'));
    expect(button, findsOneWidget);
    final rect = tester.getRect(button);
    expect(rect.top, greaterThanOrEqualTo(0));
    expect(rect.bottom, lessThanOrEqualTo(640));
    expect(find.text('Start service'), findsOneWidget);
  });

  testWidgets('S-AC3 running shows QR and actions without the expiry sentence',
      (tester) async {
    await show(
      tester,
      size: const Size(412, 915),
      host: running(),
      now: () => 100,
      onStop: () async {},
    );
    expect(find.byKey(const ValueKey('sealdesk-qr')), findsOneWidget);
    expect(find.text('Copy pairing code'), findsOneWidget);
    expect(find.text('Refresh password'), findsOneWidget);
    expect(find.text('Hide QR code'), findsOneWidget);
    expect(
        find.textContaining('QR codes are valid for 10 minutes'), findsNothing);
    expect(find.byKey(const ValueKey('sealdesk-qr-ttl')), findsOneWidget);
    expect(find.text('QR refreshes in 10 min'), findsOneWidget);
    expect(find.text('10:00'), findsNothing);
    expect(find.byKey(const ValueKey('sealdesk-trusted')), findsNothing);
    final qr = tester.getRect(find.byKey(const ValueKey('sealdesk-qr')));
    expect(qr.top, greaterThanOrEqualTo(0));
    expect(qr.bottom, lessThanOrEqualTo(915));
    expect(find.text('SealDesk'), findsNothing);
    expect(find.text('Scan to connect, no account required'), findsNothing);
    expect(find.byType(AppBar), findsNothing);
  });

  testWidgets('S-AC3 stopped has a single non-wrapping scam line',
      (tester) async {
    await show(
      tester,
      size: const Size(360, 640),
      host: const SealDeskHostState(
          address: '', password: '', name: '', stopped: true),
      onStart: () async {},
    );
    final line = find.byKey(const ValueKey('sealdesk-scam-line'));
    expect(line, findsOneWidget);
    final text = tester
        .widget<Text>(find.descendant(of: line, matching: find.byType(Text)));
    expect(text.maxLines, 1);
    expect(text.data, 'Stranger asks you to share? Hang up');
    expect(find.text('Sharing stopped'), findsOneWidget);
    expect(find.textContaining('Starting LAN sharing'), findsNothing);
  });

  testWidgets('S-AC4 stopped never mounts the LAN panel', (tester) async {
    await show(
      tester,
      size: const Size(360, 640),
      host: const SealDeskHostState(
          address: address, password: 'secret', name: 'pixel', stopped: true),
      lanPanelBuilder: (_) => const Text('Starting LAN sharing...'),
    );
    expect(find.text('Sharing stopped'), findsOneWidget);
    expect(find.text('Starting LAN sharing...'), findsNothing);
  });

  testWidgets('S-AC5 missing permission lists grants and hides the QR',
      (tester) async {
    await show(
      tester,
      size: const Size(360, 640),
      host: const SealDeskHostState(
        address: address,
        password: 'secret',
        name: 'pixel',
        permissionMissing: true,
      ),
      permissionBanner: const Text('Screen Capture'),
    );
    expect(find.text('Screen Capture'), findsOneWidget);
    expect(find.byKey(const ValueKey('sealdesk-qr')), findsNothing);
    expect(find.text('Copy pairing code'), findsNothing);
  });

  testWidgets('S-AC6 desktop running keeps the pairing card', (tester) async {
    await show(tester, size: const Size(800, 900), host: running());
    expect(find.byKey(const ValueKey('sealdesk-sharing')), findsOneWidget);
    expect(find.byKey(const ValueKey('sealdesk-qr')), findsOneWidget);
    expect(find.text('Sharing stopped'), findsNothing);
  });

  testWidgets('S-AC7g stop returns the page to stopped', (tester) async {
    var stopped = false;
    await tester.binding.setSurfaceSize(const Size(360, 800));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    await tester.pumpWidget(MaterialApp(
      home: Scaffold(
        body: SealDeskHomePage(
          hostOnly: true,
          initialMode: 'host',
          loadHost: () async => SealDeskHostState(
            address: stopped ? '' : address,
            password: 'secret',
            name: 'pixel',
            stopped: stopped,
          ),
          refreshPassword: () async {},
          connect: (_) async {},
          saveMode: (_) async {},
          now: () => 100,
          onStop: () async {
            stopped = true;
          },
        ),
      ),
    ));
    await tester.pump();
    expect(find.byKey(const ValueKey('sealdesk-stop-service')), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('sealdesk-stop-service')));
    await tester.pump();
    await tester.pump(const Duration(seconds: 1));
    expect(find.text('Sharing stopped'), findsOneWidget);
    expect(find.byKey(const ValueKey('sealdesk-qr')), findsNothing);
  });

  testWidgets('starting and error hide the QR', (tester) async {
    await show(
      tester,
      size: const Size(360, 640),
      host: const SealDeskHostState(address: '', password: '', name: 'pixel'),
      onStop: () async {},
    );
    expect(find.text('Generating pairing code...'), findsOneWidget);
    expect(find.byKey(const ValueKey('sealdesk-qr')), findsNothing);
    expect(find.text('Stop'), findsOneWidget);

    await show(
      tester,
      size: const Size(360, 640),
      host: const SealDeskHostState(
          address: '', password: '', name: 'pixel', startError: 'map failed'),
    );
    await tester.pump(const Duration(seconds: 1));
    await tester.pump();
    expect(find.text('map failed'), findsOneWidget);
    expect(find.text('Retry'), findsOneWidget);
    expect(find.byKey(const ValueKey('sealdesk-qr')), findsNothing);
  });

  testWidgets('S-AC14 share settings follow the old menu visibility',
      (tester) async {
    final approved = <String>[];
    final methods = <String>[];
    var permanent = 0;
    var lengths = 0;
    var numericToggles = 0;
    Future<void> pump({
      required String approve,
      required String method,
      bool approveFixed = false,
      bool numericFixed = false,
      bool permanentDisabled = false,
    }) {
      return tester.pumpWidget(MaterialApp(
        home: Scaffold(
          body: SealDeskShareSettingsView(
            approveMode: approve,
            verificationMethod: method,
            otpLength: '6',
            numeric: true,
            approveFixed: approveFixed,
            numericFixed: numericFixed,
            permanentPasswordDisabled: permanentDisabled,
            onApprove: (mode) async => approved.add(mode),
            onVerification: (method) async => methods.add(method),
            onSetPermanent: () => permanent++,
            onSetOtpLength: () => lengths++,
            onToggleNumeric: () => numericToggles++,
          ),
        ),
      ));
    }

    await pump(approve: 'password-click', method: kUseTemporaryPassword);
    expect(
        find.byKey(const ValueKey('sealdesk-share-settings')), findsOneWidget);
    expect(find.text('Accept sessions via both'), findsOneWidget);
    expect(find.text('Use one-time password'), findsOneWidget);
    expect(find.byKey(const ValueKey('sealdesk-set-permanent')), findsNothing);
    expect(find.text('One-time password length'), findsOneWidget);
    expect(find.text('6'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('sealdesk-approve')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Accept sessions via password').last);
    await tester.pumpAndSettle();
    expect(approved, ['password']);
    await tester.tap(find.byKey(const ValueKey('sealdesk-verify')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Use permanent password').last);
    await tester.pumpAndSettle();
    expect(methods, [kUsePermanentPassword]);
    await tester.tap(find.byKey(const ValueKey('sealdesk-otp-length')));
    expect(lengths, 1);

    await pump(
        approve: 'click',
        method: kUseBothPasswords,
        approveFixed: true,
        numericFixed: true);
    expect(find.text('Use both passwords'), findsNothing);
    expect(find.byKey(const ValueKey('sealdesk-otp-numeric')), findsNothing);
    final password = tester.widget<DropdownButton<String>>(
        find.byKey(const ValueKey('sealdesk-approve')));
    expect(password.onChanged, isNull);

    await pump(
      approve: 'password',
      method: kUsePermanentPassword,
      permanentDisabled: true,
    );
    expect(find.byKey(const ValueKey('sealdesk-set-permanent')), findsNothing);
    expect(find.text('One-time password length'), findsNothing);

    await pump(approve: 'password', method: kUseBothPasswords);
    expect(
        find.byKey(const ValueKey('sealdesk-set-permanent')), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('sealdesk-set-permanent')));
    expect(permanent, 1);
    final numeric = tester.widget<SwitchListTile>(
        find.byKey(const ValueKey('sealdesk-otp-numeric')));
    expect(numeric.onChanged, isNotNull);
  });
}
