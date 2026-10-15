import 'package:flutter_hbb/common/taildesk_connection.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('connection label waits for a probe before naming the path', () {
    expect(tailDeskConnectionLabel('Tailcat'), '正在检测链路');
    expect(tailDeskConnectionDescription('Tailcat'), '正在检测链路 · WireGuard 加密连接');
    expect(tailDeskConnectionLabel('TCP'), isNull);
    expect(tailDeskConnectionDescription('WebRTC'), isNull);
  });

  test('direct and relay status include encryption and relay guidance', () {
    expect(tailDeskConnectionLabel('Tailcat/P2P'), 'P2P 直连');
    expect(tailDeskConnectionDescription('Tailcat/P2P'),
        'P2P 直连 · WireGuard 加密连接');
    expect(tailDeskConnectionLabel('Tailcat/DERP'), 'DERP 中继');
    expect(tailDeskConnectionDescription('Tailcat/DERP'),
        'DERP 中继 · WireGuard 加密连接；打洞未成功，速度可能受限');
  });
}
