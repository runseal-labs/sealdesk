import 'dart:convert';
import 'dart:io';

import 'package:flutter_hbb/common/taildesk_pairing.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  final fixtures = jsonDecode(
    File('test/fixtures/taildesk_pairing.json').readAsStringSync(),
  ) as List<dynamic>;

  for (final fixture in fixtures) {
    test(fixture['label'] as String, () {
      final input = fixture['input'] as String;
      final now = fixture['now'] as int;
      final expected = fixture['expected'] as Map<String, dynamic>?;
      if (expected == null) {
        expect(() => TailDeskPairing.parse(input, now: now),
            throwsFormatException);
        return;
      }
      final pairing = TailDeskPairing.parse(input, now: now);
      expect(pairing.address, expected['address']);
      expect(pairing.password, expected['password']);
      expect(pairing.name, expected['name']);
      expect(pairing.expiresAt, expected['expires_at']);
      final roundTrip =
          TailDeskPairing.parse(pairing.encode(now: now), now: now);
      expect(roundTrip.address, pairing.address);
      expect(roundTrip.password, pairing.password);
      expect(roundTrip.name, pairing.name);
      expect(roundTrip.expiresAt, pairing.expiresAt);
    });
  }
}
