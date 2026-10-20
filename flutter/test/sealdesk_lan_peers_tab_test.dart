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
  test('version 1 is unmarked and version 2 does not need confirmation', () {
    final legacy = SealDeskLanDevice.fromJson({
      'name': 'Old',
      'protocol': 'sealdesk',
      'identity_checked': false,
    });
    final current = SealDeskLanDevice.fromJson({
      'name': 'New',
      'protocol': 'sealdesk',
      'identity_checked': true,
    });
    final omitted = SealDeskLanDevice.fromJson({
      'protocol': 'sealdesk',
    });
    expect(legacy.needsIdentityConfirmation, isTrue);
    expect(sealDeskLanNeedsIdentityConfirmation('sealdesk', false), isTrue);
    expect(current.needsIdentityConfirmation, isFalse);
    expect(omitted.needsIdentityConfirmation, isFalse);
    expect(
        sealDeskLanNeedsIdentityConfirmation('rustdesk', false), isFalse);
  });

  final binding = _LanTabBinding();

  setUp(() {
    platformFFI.setBindingForTesting(binding);
    binding.options.clear();
    binding.peers.clear();
  });

  testWidgets('LAN connect clears hide-recent and records a permanent password',
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
    expect(binding.options[address]?[kSealDeskPasswordKind],
        kSealDeskPasswordPermanent);
    expect(binding.options[address]?[kSealDeskHideRecent], isEmpty);
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

  testWidgets('version 1 is labeled unverified and connect waits for confirmation',
      (tester) async {
    final address = 'lan:192.168.1.6:21118/${'aa' * 32}';
    binding.peers.add({
      'name': 'Old host',
      'ip': '192.168.1.6',
      'address': address,
      'protocol': 'sealdesk',
      'identity_checked': false,
    });
    await tester.pumpWidget(const MaterialApp(
        home: Scaffold(body: SealDeskLanPeersTab())));
    await tester.pumpAndSettle();
    expect(find.text(kSealDeskUnverifiedIdentity), findsOneWidget);
    expect(find.text(kSealDeskEncryptedChip), findsNothing);
    await tester.tap(find.byKey(ValueKey('sealdesk-lan-connect-$address')));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('sealdesk-unverified-lan')), findsOneWidget);
    expect(binding.options[address], isNull);
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
    expect(binding.options[address], isNull);
    await tester.tap(find.byKey(ValueKey('sealdesk-lan-connect-$address')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('sealdesk-unverified-lan-continue')));
    await tester.pump();
    expect(binding.options[address]?[kSealDeskPasswordKind],
        kSealDeskPasswordPermanent);
    tester.takeException();
    await tester.pumpWidget(const SizedBox());
    await tester.pump();
  });
}
