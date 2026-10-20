// PR2 connection-record contract. UI (connection page) must call these
// functions instead of reimplementing LAN/tc split, sorting, or password rules.
//
// Badge text is [sealDeskLanBadge] ("LAN") and is not an i18n key.
// Tab label uses translate("LAN"), which is "局域网" in zh-cn.

const String sealDeskLanBadge = 'LAN';

enum SealDeskRecordKind { tailcat, lan, legacy }

class SealDeskConnectEvent {
  final String address;
  final String hostname;
  final String alias;
  final String password;
  final bool rememberPassword;
  final int connectedAtMillis;
  final bool? favorite;

  const SealDeskConnectEvent({
    required this.address,
    this.hostname = '',
    this.alias = '',
    this.password = '',
    this.rememberPassword = false,
    required this.connectedAtMillis,
    this.favorite,
  });
}

class SealDeskConnectionRecord {
  final String id;
  final SealDeskRecordKind kind;
  final String alias;
  final String hostname;
  final String address;
  final int connectedAtMillis;
  final bool favorite;
  final bool inHistory;
  final String password;

  const SealDeskConnectionRecord({
    required this.id,
    required this.kind,
    required this.alias,
    required this.hostname,
    required this.address,
    required this.connectedAtMillis,
    required this.favorite,
    required this.inHistory,
    required this.password,
  });

  String get displayAddress => sealDeskDisplayAddress(address);

  /// "LAN" for LAN and legacy IP-style rows; null for Tailcat.
  String? get badge => sealDeskRecordBadge(address);

  Map<String, dynamic> toJson() {
    final json = <String, dynamic>{
      'id': id,
      'kind': kind.name,
      'alias': alias,
      'hostname': hostname,
      'address': address,
      'connectedAtMillis': connectedAtMillis,
      'favorite': favorite,
      'inHistory': inHistory,
    };
    if (password.isNotEmpty) json['password'] = password;
    return json;
  }
}

class SealDeskConnectionRecordStore {
  final List<SealDeskConnectionRecord> _records;

  SealDeskConnectionRecordStore({List<SealDeskConnectionRecord>? records})
      : _records = List<SealDeskConnectionRecord>.of(records ?? const []);

  void replaceWith(SealDeskConnectionRecordStore other) {
    _records
      ..clear()
      ..addAll(other._records);
  }

  factory SealDeskConnectionRecordStore.fromJson(Map<String, dynamic> json) {
    final raw = json['records'];
    if (raw is! List) {
      throw const FormatException('Invalid connection address');
    }
    final records = <SealDeskConnectionRecord>[];
    final seen = <String>{};
    for (final item in raw) {
      if (item is! Map) {
        throw const FormatException('Invalid connection address');
      }
      final record = _recordFromJson(Map<String, dynamic>.from(item));
      if (!seen.add(record.id)) {
        throw const FormatException('Invalid connection address');
      }
      records.add(record);
    }
    return SealDeskConnectionRecordStore(records: records);
  }

  Map<String, dynamic> toJson() => <String, dynamic>{
        'records': _records.map((record) => record.toJson()).toList(),
      };

  SealDeskConnectionRecord? get(String id) {
    for (final record in _records) {
      if (record.id == id) return record;
    }
    return null;
  }

  /// Insert or refresh a history row. A password is kept only when remember is set.
  /// The same host's LAN address and tc address stay two records.
  SealDeskConnectionRecord upsert(SealDeskConnectEvent event) {
    final address = event.address.trim();
    final kind = sealDeskClassifyAddress(address);
    if (kind == null) {
      throw const FormatException('Invalid connection address');
    }
    final id = sealDeskRecordId(address);
    final previous = get(id);
    final hostname = event.hostname.trim().isNotEmpty
        ? event.hostname.trim()
        : (previous?.hostname ?? '');
    final alias = event.alias.trim().isNotEmpty
        ? event.alias.trim()
        : (previous != null && previous.alias.isNotEmpty)
            ? previous.alias
            : (hostname.isNotEmpty ? hostname : sealDeskDisplayAddress(address));
    final record = SealDeskConnectionRecord(
      id: id,
      kind: kind,
      alias: alias,
      hostname: hostname,
      address: address,
      connectedAtMillis: event.connectedAtMillis,
      favorite: event.favorite ?? previous?.favorite ?? false,
      inHistory: true,
      password: sealDeskPasswordToStore(
        rememberPassword: event.rememberPassword,
        password: event.password,
      ),
    );
    _records.removeWhere((item) => item.id == id);
    _records.add(record);
    return record;
  }

  /// History rows, newest connection first. Id ascending breaks ties.
  List<SealDeskConnectionRecord> listRecent() =>
      _sorted((record) => record.inHistory);

  /// Favorites are independent of history. Clear history does not remove them.
  List<SealDeskConnectionRecord> listFavorites() =>
      _sorted((record) => record.favorite);

  /// Removes one row from history and favorites. Returns false if missing.
  bool delete(String id) {
    final before = _records.length;
    _records.removeWhere((record) => record.id == id);
    return _records.length != before;
  }

  /// Drops history. Favorite rows remain and disappear from [listRecent].
  void clearHistory() {
    final kept = <SealDeskConnectionRecord>[];
    for (final record in _records) {
      if (!record.inHistory && !record.favorite) continue;
      if (!record.favorite) continue;
      kept.add(SealDeskConnectionRecord(
        id: record.id,
        kind: record.kind,
        alias: record.alias,
        hostname: record.hostname,
        address: record.address,
        connectedAtMillis: record.connectedAtMillis,
        favorite: true,
        inHistory: false,
        password: record.password,
      ));
    }
    _records
      ..clear()
      ..addAll(kept);
  }

  List<SealDeskConnectionRecord> _sorted(
      bool Function(SealDeskConnectionRecord record) include) {
    final items = _records.where(include).toList();
    items.sort((a, b) {
      final byTime = b.connectedAtMillis.compareTo(a.connectedAtMillis);
      if (byTime != 0) return byTime;
      return a.id.compareTo(b.id);
    });
    return items;
  }
}

List<String> sealDeskPreferredRecentIds(
  Iterable<String> ids, {
  String? Function(String canonicalId)? currentAddress,
}) {
  final groups = <String, List<String>>{};
  final order = <String>[];
  for (final id in ids) {
    String key;
    try {
      key = sealDeskRecordId(id);
    } on FormatException {
      key = id;
    }
    if (!groups.containsKey(key)) order.add(key);
    groups.putIfAbsent(key, () => []).add(id);
  }
  final preferred = <String>[];
  for (final key in order) {
    final group = groups[key]!;
    final current = currentAddress?.call(key);
    if (current != null && group.contains(current)) {
      preferred.add(current);
    } else {
      preferred.add(group.first);
    }
  }
  return preferred;
}

/// Empty unless the user opted in.
String sealDeskPasswordToStore({
  required bool rememberPassword,
  required String password,
}) {
  if (!rememberPassword) return '';
  return password;
}

SealDeskRecordKind? sealDeskClassifyAddress(String address) {
  final value = address.trim();
  if (value.length > 40 && _tailcat.hasMatch(value)) {
    return SealDeskRecordKind.tailcat;
  }
  if (_lan.hasMatch(value) || _ipv4.hasMatch(value)) {
    return SealDeskRecordKind.lan;
  }
  if (_legacyPort.hasMatch(value) || _userHost.hasMatch(value)) {
    return SealDeskRecordKind.legacy;
  }
  return null;
}

/// Tailcat and legacy ids are the address itself. LAN ids are `lan:<ip>`.
String sealDeskRecordId(String address) {
  final value = address.trim();
  final kind = sealDeskClassifyAddress(value);
  if (kind == null) {
    throw const FormatException('Invalid connection address');
  }
  if (kind == SealDeskRecordKind.lan) {
    final ip = _lanIp(value);
    if (ip == null) {
      throw const FormatException('Invalid connection address');
    }
    return 'lan:$ip';
  }
  return value;
}

/// `tc8f3a…c21` for long addresses. Strings of 12 characters or fewer stay whole.
String sealDeskShortAddress(String address) {
  if (address.length <= 12) return address;
  return '${address.substring(0, 6)}…${address.substring(address.length - 3)}';
}

String sealDeskDisplayAddress(String address) {
  final value = address.trim();
  switch (sealDeskClassifyAddress(value)) {
    case SealDeskRecordKind.tailcat:
      return sealDeskShortAddress(value);
    case SealDeskRecordKind.lan:
      return _lanIp(value) ?? value;
    case SealDeskRecordKind.legacy:
      final port = _legacyPort.firstMatch(value);
      if (port != null) return port.group(1)!;
      final at = value.lastIndexOf('@');
      if (at >= 0 && _userHost.hasMatch(value)) return value.substring(at + 1);
      return value;
    case null:
      return value;
  }
}

String? sealDeskRecordBadge(String address) {
  final kind = sealDeskClassifyAddress(address);
  if (kind == SealDeskRecordKind.lan || kind == SealDeskRecordKind.legacy) {
    return sealDeskLanBadge;
  }
  return null;
}

final RegExp _tailcat = RegExp(r'^tc[A-Za-z0-9_-]+$');
final RegExp _ipv4 = RegExp(
    r'^(?:25[0-5]|2[0-4]\d|1?\d?\d)(?:\.(?:25[0-5]|2[0-4]\d|1?\d?\d)){3}$');
final RegExp _lan = RegExp(
    r'^lan:((?:25[0-5]|2[0-4]\d|1?\d?\d)(?:\.(?:25[0-5]|2[0-4]\d|1?\d?\d)){3}):(\d+)/([0-9a-fA-F]+)$');
final RegExp _legacyPort = RegExp(
    r'^((?:25[0-5]|2[0-4]\d|1?\d?\d)(?:\.(?:25[0-5]|2[0-4]\d|1?\d?\d)){3}):(\d+)$');
final RegExp _userHost = RegExp(r'^[^@\s]+@[^@\s:]+$');

String? _lanIp(String value) {
  final lan = _lan.firstMatch(value);
  if (lan != null) return lan.group(1);
  if (_ipv4.hasMatch(value)) return value;
  return null;
}

SealDeskConnectionRecord _recordFromJson(Map<String, dynamic> json) {
  final address = json['address'];
  final id = json['id'];
  final kindName = json['kind'];
  final alias = json['alias'];
  final hostname = json['hostname'];
  final connectedAt = json['connectedAtMillis'];
  final favorite = json['favorite'];
  final inHistory = json['inHistory'];
  final password = json['password'] ?? '';
  if (address is! String ||
      id is! String ||
      kindName is! String ||
      alias is! String ||
      hostname is! String ||
      connectedAt is! int ||
      favorite is! bool ||
      inHistory is! bool ||
      password is! String) {
    throw const FormatException('Invalid connection address');
  }
  final kind = sealDeskClassifyAddress(address);
  if (kind == null || kind.name != kindName || sealDeskRecordId(address) != id) {
    throw const FormatException('Invalid connection address');
  }
  return SealDeskConnectionRecord(
    id: id,
    kind: kind,
    alias: alias,
    hostname: hostname,
    address: address,
    connectedAtMillis: connectedAt,
    favorite: favorite,
    inHistory: inHistory,
    password: password,
  );
}
