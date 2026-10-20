import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sealdesk/common/sealdesk_connect_target.dart';
import 'package:sealdesk/common/sealdesk_lan_panel.dart';
import 'package:sealdesk/generated_bridge.dart';
import 'package:sealdesk/models/platform_model.dart';

class _LanBinding extends Fake implements SealdeskImpl {
  List<Map<String, dynamic>> peers = [];
  List<Map<String, dynamic>> trusted = [];
  final List<String> revoked = [];
  int discoveryQueries = 0;
  bool ready = true;
  List<String> compatAddresses = [];
  @override
  String translate(
          {required String name, required String locale, dynamic hint}) =>
      name;
  @override
  Future<String> mainGetOption({required String key, dynamic hint}) async {
    if (key == 'sealdesk-lan-peers') discoveryQueries++;
    if (key == 'sealdesk-lan-ready') return ready ? 'Y' : '';
    if (key == 'sealdesk-lan-compat') return jsonEncode(compatAddresses);
    return jsonEncode(key == 'sealdesk-lan-peers' ? peers : trusted);
  }

  @override
  Future<void> mainSetOption(
      {required String key, required String value, dynamic hint}) async {
    if (key == 'sealdesk-lan-forget') {
      revoked.add(value);
      trusted.removeWhere((p) => p['key'] == value);
    }
  }
}

void main() {
  final binding = _LanBinding();
  setUp(() {
    platformFFI.setBindingForTesting(binding);
    binding.ready = true;
    binding.compatAddresses.clear();
    binding.peers.clear();
    binding.trusted.clear();
    binding.revoked.clear();
  });
  testWidgets(
      'host exposes the plaintext compatibility endpoint only while ready',
      (tester) async {
    binding.compatAddresses.add('192.168.1.7:21118');
    await tester.pumpWidget(MaterialApp(
        home: Scaffold(
            body: SealDeskLanPanel(host: true, connectPeer: (_) async {}))));
    await tester.pumpAndSettle();
    expect(find.textContaining('Direct and unencrypted connection'),
        findsOneWidget);
    expect(find.textContaining('192.168.1.7:21118'), findsOneWidget);
    binding.ready = false;
    await tester.pump(const Duration(seconds: 3));
    await tester.pumpAndSettle();
    expect(find.textContaining('192.168.1.7:21118'), findsNothing);
    await tester.pumpWidget(const SizedBox());
  });
  testWidgets('discovered selection retains the host identity in the address',
      (tester) async {
    final address = 'lan:192.168.1.7:21118/${'a' * 64}';
    binding.peers
        .add({'name': 'Test host', 'ip': '192.168.1.7', 'address': address});
    String? connected;
    await tester.pumpWidget(MaterialApp(
        home: Scaffold(
            body: SealDeskLanPanel(
                host: false,
                connectPeer: (value) async {
                  connected = value;
                }))));
    await tester.pumpAndSettle();
    expect(find.text('Test host'), findsOneWidget);
    await tester.tap(find.text('Connect'));
    expect(connected, address);
    await tester.pumpWidget(const SizedBox());
  });
  testWidgets(
      'RustDesk peers show plaintext status and keep the direct endpoint',
      (tester) async {
    binding.peers.add({
      'name': 'Original RustDesk',
      'ip': '192.168.1.9',
      'address': '192.168.1.9:21118',
      'protocol': 'rustdesk'
    });
    String? connected;
    await tester.pumpWidget(MaterialApp(
        home: Scaffold(
            body: SealDeskLanPanel(
                host: false,
                connectPeer: (address) async {
                  connected = address;
                }))));
    await tester.pumpAndSettle();
    expect(find.textContaining('Compatible with RustDesk · Unencrypted'),
        findsOneWidget);
    await tester.tap(find.text('Connect'));
    expect(connected, '192.168.1.9:21118');
    await tester.pumpWidget(const SizedBox());
  });
  testWidgets(
      'mobile discovery scrolls and stops polling after leaving the tab',
      (tester) async {
    await tester.binding.setSurfaceSize(const Size(360, 300));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    for (var i = 0; i < 8; i++) {
      binding.peers.add({
        'name': 'LAN host $i',
        'ip': '192.168.1.${i + 2}',
        'address': 'lan:192.168.1.${i + 2}:21118/${'a' * 64}'
      });
    }
    String? connected;
    await tester.pumpWidget(MaterialApp(
        home: Scaffold(
            body: SingleChildScrollView(
                child: SealDeskLanPanel(
                    host: false,
                    connectPeer: (address) async {
                      connected = address;
                    })))));
    await tester.pumpAndSettle();
    await tester.scrollUntilVisible(find.text('LAN host 7'), 200,
        scrollable: find.byType(Scrollable).first);
    await tester.tap(find.descendant(
        of: find.widgetWithText(ListTile, 'LAN host 7'),
        matching: find.byType(TextButton)));
    expect(connected, binding.peers.last['address']);
    await tester.pumpWidget(const SizedBox());
    final queries = binding.discoveryQueries;
    await tester.pump(const Duration(seconds: 4));
    expect(binding.discoveryQueries, queries);
    expect(tester.takeException(), isNull);
  });
  testWidgets('version 1 is labeled unverified and connect asks first',
      (tester) async {
    final address = 'lan:192.168.1.6:21118/${'a' * 64}';
    binding.peers.add({
      'name': 'Old host',
      'ip': '192.168.1.6',
      'address': address,
      'protocol': 'sealdesk',
      'identity_checked': false,
    });
    String? connected;
    await tester.pumpWidget(MaterialApp(
        home: Scaffold(
            body: SealDeskLanPanel(
                host: false,
                connectPeer: (value) async {
                  connected = value;
                }))));
    await tester.pumpAndSettle();
    expect(find.text(kSealDeskUnverifiedIdentity), findsOneWidget);
    expect(find.text('Encrypted direct connection'), findsNothing);
    await tester.tap(find.text('Connect'));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('sealdesk-unverified-lan')), findsOneWidget);
    expect(connected, isNull);
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
    expect(connected, isNull);
    await tester.tap(find.text('Connect'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('sealdesk-unverified-lan-continue')));
    await tester.pumpAndSettle();
    expect(connected, address);
    await tester.pumpWidget(const SizedBox());
  });
  testWidgets('revocation reads back the updated controller list',
      (tester) async {
    final key = 'b' * 64;
    binding.trusted.add({'key': key, 'name': 'Controller'});
    await tester.pumpWidget(MaterialApp(
        home: Scaffold(
            body: SealDeskLanPanel(host: true, connectPeer: (_) async {}))));
    await tester.pumpAndSettle();
    expect(find.text('LAN sharing is ready'), findsOneWidget);
    await tester.tap(find.text('Revoke access'));
    await tester.pumpAndSettle();
    expect(binding.revoked, [key]);
    expect(find.text('Controller'), findsNothing);
    await tester.pumpWidget(const SizedBox());
  });
}
