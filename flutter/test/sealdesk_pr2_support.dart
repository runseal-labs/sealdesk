import 'dart:convert';
import 'dart:io';

import 'package:sealdesk/common/sealdesk_connection_record.dart';

Map<String, dynamic> loadSealDeskPr2Fixture() => jsonDecode(
      File('test/fixtures/sealdesk_pr2_records.json').readAsStringSync(),
    ) as Map<String, dynamic>;

SealDeskConnectEvent sealDeskTestConnect({
  required String address,
  required int connectedAtMillis,
  String hostname = '',
  String alias = '',
  String password = '',
  bool usedPermanentPassword = false,
  bool rememberPassword = false,
  bool? favorite,
}) {
  return SealDeskConnectEvent(
    address: address,
    hostname: hostname,
    alias: alias,
    password: password,
    usedPermanentPassword: usedPermanentPassword,
    rememberPassword: rememberPassword,
    connectedAtMillis: connectedAtMillis,
    favorite: favorite,
  );
}
