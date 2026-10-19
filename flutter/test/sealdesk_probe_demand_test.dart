import 'dart:async';

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

  test('a slower demand write cannot finish as the applied value', () async {
    final values = <String>[];
    var inFlight = 0;
    var maxInFlight = 0;
    final release = Completer<void>();
    final started = Completer<void>();
    SealDeskProbeDemand.debugResetForTesting();
    SealDeskProbeDemand.debugWriteForTesting = (value) async {
      inFlight++;
      if (inFlight > maxInFlight) maxInFlight = inFlight;
      values.add(value);
      if (!started.isCompleted) started.complete();
      try {
        if (values.length == 1) await release.future;
      } finally {
        inFlight--;
      }
    };

    SealDeskProbeDemand.setShareVisible(true);
    await started.future;
    expect(values, ['share=1,background=0,screen=0']);
    expect(inFlight, 1);

    SealDeskProbeDemand.setLifecycle(AppLifecycleState.paused);
    SealDeskProbeDemand.setShareVisible(false);
    expect(values, hasLength(1));

    release.complete();
    await SealDeskProbeDemand.debugDrainForTesting();
    expect(maxInFlight, 1);
    expect(values, [
      'share=1,background=0,screen=0',
      'share=0,background=1,screen=1',
    ]);
    SealDeskProbeDemand.debugResetForTesting();
  });

  test('a burst keeps only the newest demand that has not started', () async {
    final values = <String>[];
    var inFlight = 0;
    var maxInFlight = 0;
    SealDeskProbeDemand.debugResetForTesting();
    SealDeskProbeDemand.debugWriteForTesting = (value) async {
      inFlight++;
      if (inFlight > maxInFlight) maxInFlight = inFlight;
      values.add(value);
      inFlight--;
    };

    SealDeskProbeDemand.setShareVisible(true);
    SealDeskProbeDemand.setLifecycle(AppLifecycleState.paused);
    SealDeskProbeDemand.setLifecycle(AppLifecycleState.resumed);
    await SealDeskProbeDemand.debugDrainForTesting();

    expect(maxInFlight, 1);
    expect(values, ['share=1,background=0,screen=0']);
    SealDeskProbeDemand.debugResetForTesting();
  });
}
