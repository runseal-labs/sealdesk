import 'package:flutter_test/flutter_test.dart';
import 'package:sealdesk/common/sealdesk_connect_target.dart';

void main() {
  test('typed direct addresses accept CGNAT, LAN, port, and IPv6', () {
    for (final input in [
      '100.64.0.8',
      '100.127.255.1',
      '192.168.1.20',
      '10.1.2.3',
      '100.64.12.9:21118',
      '192.168.0.4:21118',
      '10.2.3.4:21118',
    ]) {
      final target = parseSealDeskConnectInput(input);
      expect(target.ok, isTrue, reason: input);
      expect(target.kind, 'ip', reason: input);
      expect(target.id, input, reason: input);
    }

    final v6 = parseSealDeskConnectInput('[fd00::1]:21118');
    expect(v6.ok, isTrue);
    expect(v6.kind, 'ip');
    expect(v6.id, '[fd00::1]:21118');

    final bare = parseSealDeskConnectInput('fd12::abcd');
    expect(bare.ok, isTrue);
    expect(bare.kind, 'ip');
    expect(bare.id, 'fd12::abcd');

    final ula = parseSealDeskConnectInput('fd00::1');
    expect(ula.ok, isTrue);
    expect(ula.kind, 'ip');
    expect(ula.id, 'fd00::1');
  });

  test('typed direct addresses reject ipv6 forms the backend parser rejects', () {
    for (final input in ['::1', 'fd00::', '[fd00::1]', '2001:db8::']) {
      final target = parseSealDeskConnectInput(input);
      expect(target.ok, isFalse, reason: input);
      expect(target.kind, 'invalid', reason: input);
    }
  });
}
