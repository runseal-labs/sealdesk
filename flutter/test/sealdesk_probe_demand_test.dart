import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sealdesk/common/sealdesk_probe_demand.dart';

void main() {
  test('demand string carries share, background, and screen', () {
    expect(
      sealDeskProbeDemandValue(
        shareVisible: true,
        background: false,
        screenOff: false,
      ),
      'share=1,background=0,screen=0',
    );
    expect(
      sealDeskProbeDemandValue(
        shareVisible: false,
        background: true,
        screenOff: true,
      ),
      'share=0,background=1,screen=1',
    );
  });

  test('paused hidden and detached stop probes, inactive does not', () {
    expect(sealDeskLifecycleAsleep(AppLifecycleState.paused), isTrue);
    expect(sealDeskLifecycleAsleep(AppLifecycleState.hidden), isTrue);
    expect(sealDeskLifecycleAsleep(AppLifecycleState.detached), isTrue);
    expect(sealDeskLifecycleAsleep(AppLifecycleState.inactive), isFalse);
    expect(sealDeskLifecycleAsleep(AppLifecycleState.resumed), isFalse);
  });
}
