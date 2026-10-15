import 'package:sealdesk/common/sealdesk_connection.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('connection label waits for a probe before naming the path', () {
    expect(sealDeskConnectionLabel('Tailcat'), '正在检测链路');
    expect(sealDeskConnectionDescription('Tailcat'), '正在检测链路 · WireGuard 加密连接');
    expect(sealDeskConnectionLabel('TCP'), isNull);
    expect(sealDeskConnectionDescription('WebRTC'), isNull);
  });

  test('direct and relay status include encryption and relay guidance', () {
    expect(sealDeskConnectionLabel('Tailcat/P2P'), 'P2P 直连');
    expect(sealDeskConnectionDescription('Tailcat/P2P'),
        'P2P 直连 · WireGuard 加密连接');
    expect(sealDeskConnectionLabel('Tailcat/DERP'), 'DERP 中继');
    expect(sealDeskConnectionDescription('Tailcat/DERP'),
        'DERP 中继 · WireGuard 加密连接；打洞未成功，速度可能受限');
  });
}
