import 'package:flutter_test/flutter_test.dart';
import 'package:sealdesk/common/sealdesk_qr_mask.dart';

void main() {
  test('obscures only on the transition from zero to a connected controller', () {
    final mask = SealDeskQrMask();
    expect(mask.obscured, isFalse);
    expect(mask.showsTapToShow, isFalse);
    expect(mask.connectedCount, 0);

    mask.onConnectedCount(0);
    expect(mask.obscured, isFalse);

    mask.onConnectedCount(1);
    expect(mask.obscured, isTrue);
    expect(mask.showsTapToShow, isTrue);
    expect(mask.connectedCount, 1);

    mask.onConnectedCount(4);
    expect(mask.obscured, isTrue);

    mask.show();
    expect(mask.obscured, isFalse);
    mask.onConnectedCount(5);
    expect(mask.obscured, isFalse,
        reason: 'manual show stays visible while controllers remain');

    mask.onConnectedCount(0);
    expect(mask.obscured, isFalse,
        reason: 'disconnect does not change the current visibility');
    expect(mask.connectedCount, 0);

    mask.onConnectedCount(2);
    expect(mask.obscured, isTrue);

    mask.onConnectedCount(-3);
    expect(mask.connectedCount, 0);
    expect(mask.obscured, isTrue,
        reason: 'dropping to zero does not reveal a hidden code');
  });

  test('tap reveals a mask and only enlarges once the code is visible', () {
    final mask = SealDeskQrMask();
    mask.onConnectedCount(1);
    expect(mask.onQrTap(), SealDeskQrTapAction.reveal);
    expect(mask.obscured, isFalse);
    expect(mask.showsTapToShow, isFalse);
    expect(mask.onQrTap(), SealDeskQrTapAction.enlarge);
    expect(mask.obscured, isFalse);

    mask.hide();
    expect(mask.obscured, isTrue);
    expect(mask.showsTapToShow, isTrue);
    mask.onConnectedCount(1);
    expect(mask.obscured, isTrue,
        reason: 'count staying above zero does not toggle a manual hide');
  });

  test('constructor count does not count as a zero-to-positive transition', () {
    final seeded = SealDeskQrMask(connectedCount: 3);
    expect(seeded.obscured, isFalse);
    expect(seeded.connectedCount, 3);
    seeded.onConnectedCount(3);
    expect(seeded.obscured, isFalse);

    final negative = SealDeskQrMask(connectedCount: -1);
    expect(negative.connectedCount, 0);
    negative.onConnectedCount(1);
    expect(negative.obscured, isTrue);
  });
}
