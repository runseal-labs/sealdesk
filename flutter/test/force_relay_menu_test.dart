import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sealdesk/common/force_relay.dart';

void main() {
  test('force relay is off unless the peer option is explicitly on', () {
    expect(storedPeerForceRelay(''), isFalse);
    expect(storedPeerForceRelay('N'), isFalse);
    expect(storedPeerForceRelay('false'), isFalse);
    expect(storedPeerForceRelay('Y'), isTrue);
    expect(storedPeerForceRelay('true'), isTrue);
    expect(showForceAlwaysRelayMenu(web: false), isTrue);
    expect(showForceAlwaysRelayMenu(web: true), isFalse);
  });

  testWidgets('connection menu switch shows force relay and toggles it',
      (tester) async {
    var on = false;
    await tester.pumpWidget(MaterialApp(
      home: Scaffold(
        body: StatefulBuilder(
          builder: (context, setState) => SwitchListTile(
            title: const Text(forceRelayMenuLabel),
            value: on,
            onChanged: (value) => setState(() => on = value),
          ),
        ),
      ),
    ));

    expect(find.text('Always connect via relay'), findsOneWidget);
    expect(on, isFalse);
    await tester.tap(find.byType(Switch));
    await tester.pump();
    expect(on, isTrue);
    await tester.tap(find.byType(Switch));
    await tester.pump();
    expect(on, isFalse);
  });
}
