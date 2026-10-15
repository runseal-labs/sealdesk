import 'package:sealdesk/common/widgets/autocomplete.dart';
import 'package:sealdesk/models/peer_model.dart';
import 'package:flutter_test/flutter_test.dart';

Peer _peer({
  required String id,
  String alias = '',
  String username = '',
  String hostname = '',
  bool online = false,
}) {
  final peer = Peer(
    id: id,
    username: username,
    hostname: hostname,
    alias: alias,
    platform: '',
    tags: [],
    hash: '',
    password: '',
    forceAlwaysRelay: false,
    rdpPort: '',
    rdpUsername: '',
    loginName: '',
    device_group_name: '',
    note: '',
  );
  peer.online = online;
  return peer;
}

void main() {
  test('merged autocomplete peers keep recent metadata and online state', () {
    final peers = mergeAutocompletePeers(
      recentPeers: [
        _peer(id: '123456789', alias: 'Office PC', username: 'recent-user'),
        _peer(id: '123456789', online: true),
      ],
      restRecentPeerIds: ['987654321'],
    );

    expect(peers, hasLength(2));
    expect(peers.first.id, '123456789');
    expect(peers.first.alias, 'Office PC');
    expect(peers.first.username, 'recent-user');
    expect(peers.first.online, isTrue);
    expect(peers.last.id, '987654321');
  });

  test('online callback updates loaded peers', () {
    final loader = AllPeersLoader();
    loader.peers = [_peer(id: '123456789')];

    loader.updateOnlineStateForTesting({
      'onlines': '123456789',
      'offlines': '',
    });

    expect(loader.peers.single.online, isTrue);
  });
}
