import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sealdesk/common/sealdesk_connect.dart';
import 'package:sealdesk/common/sealdesk_connect_target.dart';
import 'package:sealdesk/common/widgets/sealdesk_lan_peers_tab.dart';
import 'package:sealdesk/generated_bridge.dart';
import 'package:sealdesk/models/platform_model.dart';

class _LanTabBinding extends Fake implements SealdeskImpl {
  final Map<String, Map<String, String>> options = {};
  List<Map<String, dynamic>> peers = [];

  @override
  String translate(
          {required String name, required String locale, dynamic hint}) =>
      name;

  @override
  Future<String> mainGetOption({required String key, dynamic hint}) async =>
      jsonEncode(peers);

  @override
  Future<void> mainSetPeerOption(
      {required String id,
      required String key,
      required String value,
      dynamic hint}) async {
    options.putIfAbsent(id, () => {})[key] = value;
  }

  @override
  String mainGetLocalOption({required String key, dynamic hint}) => '';

  @override
  Future<void> mainSetLocalOption(
          {required String key, required String value, dynamic hint}) async {}
}

void main() {
  final binding = _LanTabBinding();

  setUp(() {
    platformFFI.setBindingForTesting(binding);
    binding.options.clear();
    binding.peers.clear();
  });

  testWidgets('LAN connect clears hide-recent',
      (tester) async {
    final address = 'lan:192.168.1.7:21118/${'ab' * 32}';
    binding.peers.add({
      'name': 'Desk',
      'ip': '192.168.1.7',
      'address': address,
      'protocol': 'sealdesk',
    });
    await tester.pumpWidget(const MaterialApp(
        home: Scaffold(body: SealDeskLanPeersTab())));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(ValueKey('sealdesk-lan-connect-$address')));
    await tester.pump();
    await tester.pump();
    expect(binding.options[address]?['sealdesk-hide-recent'], isEmpty);
    tester.takeException();
    await tester.pumpWidget(const SizedBox());
    await tester.pump();
  });

  testWidgets('LAN list offers file transfer and hides terminal on Android', (tester) async {
    final desktop = 'lan:192.168.1.8:21118/${'cd' * 32}';
    final android = 'lan:192.168.1.9:21118/${'ef' * 32}';
    binding.peers.addAll([
      {
        'name': 'Desk',
        'ip': '192.168.1.8',
        'address': desktop,
        'protocol': 'sealdesk',
        'platform': 'Linux',
      },
      {
        'name': 'Phone',
        'ip': '192.168.1.9',
        'address': android,
        'protocol': 'sealdesk',
        'platform': 'Android',
      },
    ]);
    await tester.pumpWidget(const MaterialApp(
        home: Scaffold(body: SealDeskLanPeersTab())));
    await tester.pumpAndSettle();
    expect(find.byKey(ValueKey('sealdesk-lan-file-$desktop')), findsOneWidget);
    expect(find.byKey(ValueKey('sealdesk-lan-terminal-$desktop')), findsOneWidget);
    expect(find.byKey(ValueKey('sealdesk-lan-file-$android')), findsOneWidget);
    expect(find.byKey(ValueKey('sealdesk-lan-terminal-$android')), findsNothing);
    await tester.tap(find.byKey(ValueKey('sealdesk-lan-terminal-$desktop')));
    await tester.pump();
    tester.takeException();
    await tester.pumpWidget(const SizedBox());
    await tester.pump();
  });

  testWidgets('empty LAN list explains after searching', (tester) async {
    SealDeskLanDirectory.instance.devices = [];
    await tester.pumpWidget(const MaterialApp(
        home: Scaffold(body: SealDeskLanPeersTab())));
    await tester.pump();
    expect(find.text(kSealDeskSearching), findsOneWidget);
    expect(find.text(kSealDeskLanEmptyHint), findsNothing);
    expect(find.byKey(const ValueKey('sealdesk-lan-search')), findsOneWidget);
    await tester.pump(const Duration(seconds: 10));
    expect(find.text(kSealDeskLanEmptyHint), findsOneWidget);
    expect(find.text(kSealDeskSearching), findsNothing);
    expect(find.byKey(const ValueKey('sealdesk-lan-search')), findsOneWidget);
    await tester.pumpWidget(const SizedBox());
    await tester.pump();
  });
}
