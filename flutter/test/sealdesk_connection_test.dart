import 'package:sealdesk/common/sealdesk_connection.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('connection label waits for a probe before naming the path', () {
    expect(sealDeskConnectionLabel('Tailcat'), 'Checking the link');
    expect(sealDeskConnectionDescription('Tailcat'),
        'Checking the link · WireGuard encrypted connection');
    expect(sealDeskConnectionLabel('TCP'), isNull);
    expect(sealDeskConnectionDescription('WebRTC'), isNull);
  });

  test('direct and relay status include encryption and relay guidance', () {
    expect(sealDeskConnectionLabel('Tailcat/P2P'), 'P2P direct');
    expect(sealDeskConnectionDescription('Tailcat/P2P'),
        'P2P direct · WireGuard encrypted connection');
    expect(sealDeskConnectionLabel('Tailcat/DERP'), 'DERP relay');
    expect(sealDeskConnectionDescription('Tailcat/DERP'),
        'DERP relay · WireGuard encrypted connection; hole punching failed, speed may be limited');
  });
}
