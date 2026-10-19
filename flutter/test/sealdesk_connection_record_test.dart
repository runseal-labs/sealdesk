import 'package:flutter_test/flutter_test.dart';
import 'package:sealdesk/common/sealdesk_connection_record.dart';

import 'sealdesk_pr2_support.dart';

void main() {
  late Map<String, dynamic> fixture;

  setUp(() => fixture = loadSealDeskPr2Fixture());

  test('shortens long tc addresses and classifies LAN separately from legacy', () {
    final tailcat = fixture['tailcat'] as String;
    expect(tailcat.length, greaterThan(40));
    expect(sealDeskClassifyAddress(tailcat), SealDeskRecordKind.tailcat);
    expect(sealDeskShortAddress(tailcat), fixture['tailcat_short']);
    expect(sealDeskDisplayAddress(tailcat), fixture['tailcat_short']);
    expect(sealDeskRecordBadge(tailcat), isNull);
    expect(sealDeskRecordId(tailcat), tailcat);
    expect(sealDeskShortAddress('tcshort'), 'tcshort');
    expect(sealDeskShortAddress('123456789012'), '123456789012');
    expect(sealDeskShortAddress('1234567890123'), '123456…123');

    expect(sealDeskClassifyAddress(fixture['lan_url'] as String),
        SealDeskRecordKind.lan);
    expect(sealDeskRecordId(fixture['lan_url'] as String), fixture['lan_id']);
    expect(sealDeskDisplayAddress(fixture['lan_url'] as String), fixture['lan_ip']);
    expect(sealDeskRecordBadge(fixture['lan_url'] as String), sealDeskLanBadge);
    expect(sealDeskRecordId(fixture['plain_ip'] as String),
        'lan:${fixture['plain_ip']}');

    expect(sealDeskClassifyAddress(fixture['legacy_port'] as String),
        SealDeskRecordKind.legacy);
    expect(sealDeskRecordId(fixture['legacy_port'] as String),
        fixture['legacy_port']);
    expect(sealDeskDisplayAddress(fixture['legacy_port'] as String),
        '192.168.31.20');
    expect(sealDeskClassifyAddress(fixture['legacy_user'] as String),
        SealDeskRecordKind.legacy);
    expect(sealDeskRecordId(fixture['legacy_user'] as String),
        fixture['legacy_user']);
    expect(sealDeskDisplayAddress(fixture['legacy_user'] as String),
        fixture['legacy_user_display']);
    expect(sealDeskRecordBadge(fixture['legacy_user'] as String), sealDeskLanBadge);

    expect(sealDeskClassifyAddress('SEALDESK:V:1;'), isNull);
    expect(sealDeskClassifyAddress('not an address'), isNull);
    expect(() => sealDeskRecordId('not an address'), throwsFormatException);
  });

  test('LAN and tc records stay distinct and legacy rows are not migrated', () {
    final store = SealDeskConnectionRecordStore();
    final tailcat = fixture['tailcat'] as String;
    store.upsert(sealDeskTestConnect(
      address: tailcat,
      hostname: 'mini',
      connectedAtMillis: 10,
    ));
    store.upsert(sealDeskTestConnect(
      address: fixture['lan_url'] as String,
      hostname: 'mini',
      connectedAtMillis: 20,
    ));
    store.upsert(sealDeskTestConnect(
      address: fixture['legacy_port'] as String,
      hostname: 'old-pc',
      connectedAtMillis: 5,
    ));
    store.upsert(sealDeskTestConnect(
      address: '  ${fixture['legacy_user']}  ',
      connectedAtMillis: 6,
    ));

    final recent = store.listRecent();
    expect(recent.map((record) => record.id), [
      fixture['lan_id'],
      tailcat,
      fixture['legacy_user'],
      fixture['legacy_port'],
    ]);
    expect(recent.first.displayAddress, fixture['lan_ip']);
    expect(recent.first.badge, 'LAN');
    expect(recent[1].displayAddress, fixture['tailcat_short']);
    expect(recent[1].alias, 'mini');
    expect(store.get(fixture['legacy_port'] as String)!.displayAddress,
        '192.168.31.20');
    expect(store.get('lan:192.168.31.20'), isNull);

    store.upsert(sealDeskTestConnect(
      address: fixture['lan_url_rotated_key'] as String,
      hostname: 'mini',
      connectedAtMillis: 30,
    ));
    expect(store.listRecent().where((record) => record.kind == SealDeskRecordKind.lan),
        hasLength(1));
    expect(store.get(fixture['lan_id'] as String)!.address,
        fixture['lan_url_rotated_key']);
  });

  test('sorts newest first with id as a stable tie break', () {
    final store = SealDeskConnectionRecordStore();
    store.upsert(sealDeskTestConnect(
      address: '10.0.0.2',
      connectedAtMillis: 10,
    ));
    store.upsert(sealDeskTestConnect(
      address: '10.0.0.1',
      connectedAtMillis: 10,
    ));
    store.upsert(sealDeskTestConnect(
      address: '10.1.0.1',
      connectedAtMillis: 11,
    ));
    expect(store.listRecent().map((record) => record.id), [
      'lan:10.1.0.1',
      'lan:10.0.0.1',
      'lan:10.0.0.2',
    ]);
  });

  test('never stores a temporary password and offers remember only for permanent', () {
    expect(
        sealDeskOfferRememberPassword(usedPermanentPassword: false), isFalse);
    expect(sealDeskOfferRememberPassword(usedPermanentPassword: true), isTrue);
    expect(
        sealDeskPasswordToStore(
          usedPermanentPassword: false,
          rememberPassword: true,
          password: 'temp',
        ),
        isEmpty);
    expect(
        sealDeskPasswordToStore(
          usedPermanentPassword: true,
          rememberPassword: false,
          password: 'secret',
        ),
        isEmpty);

    final store = SealDeskConnectionRecordStore();
    final id = fixture['lan_id'] as String;
    store.upsert(sealDeskTestConnect(
      address: fixture['lan_url'] as String,
      password: 'secret',
      usedPermanentPassword: true,
      rememberPassword: true,
      connectedAtMillis: 1,
    ));
    expect(store.get(id)!.password, 'secret');

    store.upsert(sealDeskTestConnect(
      address: fixture['lan_url'] as String,
      password: 'temp',
      usedPermanentPassword: false,
      rememberPassword: true,
      connectedAtMillis: 2,
    ));
    expect(store.get(id)!.password, isEmpty);
    expect(store.get(id)!.toJson().containsKey('password'), isFalse);

    store.upsert(sealDeskTestConnect(
      address: fixture['lan_url'] as String,
      password: 'secret',
      usedPermanentPassword: true,
      rememberPassword: true,
      connectedAtMillis: 3,
    ));
    store.upsert(sealDeskTestConnect(
      address: fixture['lan_url'] as String,
      password: 'secret',
      usedPermanentPassword: true,
      rememberPassword: false,
      connectedAtMillis: 4,
    ));
    expect(store.get(id)!.password, isEmpty);
  });

  test('keeps alias edits and defaults a new alias to the hostname', () {
    final store = SealDeskConnectionRecordStore();
    final tailcat = fixture['tailcat'] as String;
    store.upsert(sealDeskTestConnect(
      address: tailcat,
      hostname: 'mini',
      connectedAtMillis: 1,
    ));
    expect(store.get(tailcat)!.alias, 'mini');
    store.upsert(sealDeskTestConnect(
      address: tailcat,
      hostname: 'mini',
      alias: 'living-room',
      connectedAtMillis: 2,
    ));
    store.upsert(sealDeskTestConnect(
      address: tailcat,
      hostname: 'renamed',
      connectedAtMillis: 3,
    ));
    expect(store.get(tailcat)!.alias, 'living-room');
    expect(store.get(tailcat)!.hostname, 'renamed');
  });

  test('delete removes one row and clear history keeps favorites only', () {
    final store = SealDeskConnectionRecordStore();
    store.upsert(sealDeskTestConnect(
      address: fixture['plain_ip'] as String,
      connectedAtMillis: 1,
      favorite: true,
    ));
    store.upsert(sealDeskTestConnect(
      address: fixture['legacy_port'] as String,
      connectedAtMillis: 2,
    ));
    store.upsert(sealDeskTestConnect(
      address: fixture['legacy_user'] as String,
      connectedAtMillis: 3,
      favorite: true,
    ));
    store.upsert(sealDeskTestConnect(
      address: fixture['tailcat'] as String,
      connectedAtMillis: 4,
    ));

    expect(store.delete('missing'), isFalse);
    expect(store.delete(fixture['legacy_port'] as String), isTrue);
    expect(store.get(fixture['legacy_port'] as String), isNull);

    store.clearHistory();
    expect(store.listRecent(), isEmpty);
    expect(store.listFavorites().map((record) => record.id), [
      fixture['legacy_user'],
      'lan:${fixture['plain_ip']}',
    ]);

    store.upsert(sealDeskTestConnect(
      address: fixture['plain_ip'] as String,
      connectedAtMillis: 8,
    ));
    expect(store.listRecent().map((record) => record.id),
        ['lan:${fixture['plain_ip']}']);
    expect(store.listFavorites(), hasLength(2));
    expect(store.get('lan:${fixture['plain_ip']}')!.favorite, isTrue);
  });

  test('round-trips json without an empty password field', () {
    final store = SealDeskConnectionRecordStore();
    store.upsert(sealDeskTestConnect(
      address: fixture['tailcat'] as String,
      hostname: 'mini',
      password: 'temp',
      connectedAtMillis: 9,
    ));
    final restored =
        SealDeskConnectionRecordStore.fromJson(store.toJson());
    final record = restored.get(fixture['tailcat'] as String)!;
    expect(record.password, isEmpty);
    expect(record.toJson().containsKey('password'), isFalse);
    expect(record.alias, 'mini');
    expect(record.inHistory, isTrue);
    expect(restored.toJson(), store.toJson());
    expect(
        () => SealDeskConnectionRecordStore.fromJson({
              'records': [
                {...record.toJson(), 'id': 'other'},
              ],
            }),
        throwsFormatException);
  });
}
