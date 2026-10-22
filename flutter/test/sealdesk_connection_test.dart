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
    expect(sealDeskConnectionLabel('LAN'), 'LAN direct');
    expect(sealDeskConnectionDescription('LAN'),
        'LAN direct · encrypted connection');
    expect(sealDeskRelaySuffix('LAN', ''), '');
    expect(sealDeskConnectionAsset('secure', 'LAN', ''), 'assets/secure.svg');
    expect(sealDeskConnectionShowsSecureGlyph('LAN'), isTrue);
    expect(sealDeskConnectionLabel('Direct'), 'Unverified identity');
    expect(sealDeskConnectionDescription('Direct'), 'Unverified identity');
    expect(sealDeskConnectionDescription('Direct'), isNot(contains('encrypted')));
    expect(sealDeskConnectionShowsSecureGlyph('Direct'), isFalse);
    expect(sealDeskRelaySuffix('Direct', ''), '');
  });

  test('path label and glyph follow unknown, DERP, P2P, then DERP again', () {
    PublishedConnection step(String name, Map<String, dynamic> evt) {
      final published = publishedConnectionFromEvent(name, evt);
      expect(published, isNotNull);
      return published!;
    }

    final unknown = step('connection_ready', {
      'secure': 'true',
      'direct': 'unknown',
      'stream_type': 'Tailcat',
    });
    expect(unknown.direct, isNull);
    expect(connectionDirectFromCache(null), unknown.direct);
    expect(sealDeskConnectionLabel(unknown.streamType), 'Checking the link');
    expect(
      sealDeskRelaySuffix(unknown.streamType, 'unknown'),
      '',
    );
    expect(
      sealDeskConnectionAsset('secure', unknown.streamType, 'unknown'),
      'assets/secure.svg',
    );

    final derp = step('tailcat_path', {
      'direct': 'false',
      'stream_type': 'Tailcat/DERP',
    });
    expect(derp.direct, isFalse);
    expect(derp.secure, isTrue);
    expect(sealDeskConnectionLabel(derp.streamType), 'DERP relay');
    expect(sealDeskRelaySuffix(derp.streamType, '_relay'), '_relay');
    expect(
      sealDeskConnectionAsset('secure', derp.streamType, '_relay'),
      'assets/secure_relay.svg',
    );

    final p2p = step('tailcat_path', {
      'direct': 'true',
      'stream_type': 'Tailcat/P2P',
    });
    expect(p2p.direct, isTrue);
    expect(sealDeskConnectionLabel(p2p.streamType), 'P2P direct');
    expect(sealDeskRelaySuffix(p2p.streamType, ''), '');
    expect(
      sealDeskConnectionAsset('secure', p2p.streamType, ''),
      'assets/secure.svg',
    );

    final fallback = step('tailcat_path', {
      'direct': 'false',
      'stream_type': 'Tailcat/DERP',
    });
    expect(fallback.direct, isFalse);
    expect(sealDeskConnectionLabel(fallback.streamType), 'DERP relay');
    expect(
      sealDeskConnectionAsset('secure', fallback.streamType, '_relay'),
      'assets/secure_relay.svg',
    );
    expect(
      connectionDirectFromCache(fallback.direct),
      isFalse,
    );
  });

  test('a path event without stream_type still names DERP or P2P', () {
    final derp = publishedConnectionFromEvent('tailcat_path', {
      'direct': 'false',
    });
    expect(derp?.streamType, 'Tailcat/DERP');
    expect(sealDeskConnectionLabel(derp!.streamType), 'DERP relay');

    final p2p = publishedConnectionFromEvent('tailcat_path', {
      'direct': 'true',
    });
    expect(p2p?.streamType, 'Tailcat/P2P');
    expect(sealDeskConnectionLabel(p2p!.streamType), 'P2P direct');
  });
}
