import 'package:flutter_test/flutter_test.dart';
import 'package:sealdesk/models/relay_status.dart';

void main() {
  test('fromJson rejects empty and invalid payloads', () {
    expect(RelayStatus.fromJson(''), isNull);
    expect(RelayStatus.fromJson('   '), isNull);
    expect(RelayStatus.fromJson('not-json'), isNull);
    expect(RelayStatus.fromJson('null'), isNull);
    expect(RelayStatus.fromJson('[]'), isNull);
    expect(RelayStatus.fromJson('"stopped"'), isNull);
  });

  test('fromJson reads relay fields and init failure text', () {
    final status = RelayStatus.fromJson(
      '{"running":true,"state":"reachable","region_code":"nyc",'
      '"probe_rtt_ms":42,"last_probe_ms":1700000000000,'
      '"last_error":"","init_stage":"start","init_error":"bind",'
      '"rust_init_error":"serve: ignored"}',
    );
    expect(status, isNotNull);
    expect(status!.running, isTrue);
    expect(status.state, 'reachable');
    expect(status.regionCode, 'nyc');
    expect(status.probeRttMs, 42);
    expect(status.lastProbeMs, 1700000000000);
    expect(status.hasChip, isTrue);
    expect(status.initFailure, 'start: bind');

    final fallback = RelayStatus.fromJson(
      '{"running":false,"state":"stopped","init_error":"","init_stage":"",'
      '"rust_init_error":"serve: boom"}',
    )!;
    expect(fallback.hasChip, isFalse);
    expect(fallback.initFailure, 'serve: boom');
    expect(fallback.probeRttMs, isNull);

    final bare = RelayStatus.fromJson(
      '{"state":"unreachable","last_error":"dial timeout","probe_rtt_ms":12.4}',
    )!;
    expect(bare.running, isFalse);
    expect(bare.probeRttMs, 12);
    expect(bare.lastError, 'dial timeout');
    expect(bare.initFailure, isNull);
    expect(bare.hasChip, isTrue);

    final connecting = RelayStatus.fromJson('{"state":"connecting"}')!;
    expect(connecting.hasChip, isTrue);
    expect(connecting.initFailure, isNull);
  });
}
