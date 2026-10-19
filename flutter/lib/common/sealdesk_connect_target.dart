import 'sealdesk_pairing.dart';
import '../models/platform_model.dart';

const kSealDeskHint = 'tc… / lan: / SEALDESK: pairing code';
const kSealDeskCompatChip = 'Compatible with RustDesk · Unencrypted';
const kSealDeskEncryptedChip = 'Direct and encrypted connection';
const kSealDeskCompatHint = 'compat-lan-hint';
const kSealDeskClearHistory = 'Clear history';
const kSealDeskNoRecords = 'No records';
const kSealDeskNoFavorites = 'No favorites';
const kSealDeskSearching = 'Searching…';
const kSealDeskScanTooltip = 'Scan to connect';
const kSealDeskScreenTooltip = 'Recognize screen or clipboard';
const kSealDeskGalleryTooltip = 'Load QR image';
const kSealDeskInvalidInput =
    'Enter a tc address, LAN address, pairing code, or IP';
const kSealDeskLanTab = 'LAN';

const kSealDeskPasswordKind = 'sealdesk-password-kind';
const kSealDeskPasswordTemporary = 'temporary';
const kSealDeskPasswordPermanent = 'permanent';
const kSealDeskHideRecent = 'sealdesk-hide-recent';
const kSealDeskCompatHintOption = 'sealdesk-compat-lan-hint';

bool isSealDeskApp() {
  try {
    return bind.mainGetAppNameSync() == 'SealDesk';
  } catch (_) {
    return false;
  }
}

class SealDeskConnectTarget {
  final String id;
  final String? password;
  final String? alias;
  final String kind;
  final String passwordKind;
  final String? error;

  const SealDeskConnectTarget({
    required this.id,
    this.password,
    this.alias,
    required this.kind,
    required this.passwordKind,
    this.error,
  });

  bool get ok => error == null && id.isNotEmpty;
}

SealDeskConnectTarget parseSealDeskConnectInput(String raw, {int? now}) {
  final text = raw.trim();
  if (text.isEmpty) {
    return const SealDeskConnectTarget(
      id: '',
      kind: 'invalid',
      passwordKind: kSealDeskPasswordTemporary,
      error: kSealDeskInvalidInput,
    );
  }
  if (SealDeskPairing.isPairingCode(text)) {
    try {
      final pairing = SealDeskPairing.parse(text, now: now);
      final temporary =
          pairing.password != null && pairing.password!.isNotEmpty;
      return SealDeskConnectTarget(
        id: pairing.address,
        password: pairing.password,
        alias: pairing.name,
        kind: 'pairing',
        passwordKind: temporary
            ? kSealDeskPasswordTemporary
            : kSealDeskPasswordPermanent,
      );
    } on FormatException catch (error) {
      return SealDeskConnectTarget(
        id: '',
        kind: 'invalid',
        passwordKind: kSealDeskPasswordTemporary,
        error: error.message,
      );
    }
  }
  if (text.toLowerCase().startsWith('lan:')) {
    final normalized = 'lan:${text.substring(4)}';
    if (!_lanAddress(normalized)) {
      return const SealDeskConnectTarget(
        id: '',
        kind: 'invalid',
        passwordKind: kSealDeskPasswordTemporary,
        error: kSealDeskInvalidInput,
      );
    }
    return SealDeskConnectTarget(
      id: normalized,
      kind: 'lan',
      passwordKind: kSealDeskPasswordPermanent,
    );
  }
  if (SealDeskPairing.isAddress(text)) {
    return SealDeskConnectTarget(
      id: text,
      kind: 'tc',
      passwordKind: kSealDeskPasswordPermanent,
    );
  }
  if (_ipAddress(text)) {
    return SealDeskConnectTarget(
      id: text,
      kind: 'ip',
      passwordKind: kSealDeskPasswordPermanent,
    );
  }
  return const SealDeskConnectTarget(
    id: '',
    kind: 'invalid',
    passwordKind: kSealDeskPasswordTemporary,
    error: kSealDeskInvalidInput,
  );
}

bool sealDeskOfferRememberPassword(String? passwordKind) =>
    passwordKind == kSealDeskPasswordPermanent;

bool sealDeskMayStorePassword(
        {required String? passwordKind, required bool remember}) =>
    sealDeskOfferRememberPassword(passwordKind) && remember;

class SealDeskRecordLabel {
  final String primary;
  final String? secondary;
  final bool lanMark;

  const SealDeskRecordLabel({
    required this.primary,
    this.secondary,
    required this.lanMark,
  });

  String get line =>
      secondary == null || secondary!.isEmpty ? primary : '$primary  $secondary';
}

String sealDeskShortTc(String id) {
  if (!id.startsWith('tc') || id.length <= 12) return id;
  return '${id.substring(0, 6)}…${id.substring(id.length - 3)}';
}

String? sealDeskLanIp(String id) {
  final matched =
      RegExp(r'^lan:(\d{1,3}(?:\.\d{1,3}){3}):\d{1,5}/').firstMatch(id);
  return matched?.group(1);
}

bool sealDeskLegacyRecord(String id) =>
    id.contains('@') || RegExp(r':21118$').hasMatch(id);

/// LAN and tc stay distinct records because their ids are never merged.
bool sealDeskRecordsAreDistinct(String a, String b) => a != b;

SealDeskRecordLabel sealDeskRecordLabel({
  required String id,
  String alias = '',
  String hostname = '',
}) {
  final name = alias.isNotEmpty ? alias : hostname;
  if (id.startsWith('lan:')) {
    final ip = sealDeskLanIp(id) ?? id;
    return SealDeskRecordLabel(
      primary: name.isEmpty ? ip : name,
      secondary: name.isEmpty ? null : ip,
      lanMark: true,
    );
  }
  final endpoint = RegExp(r'^(\d{1,3}(?:\.\d{1,3}){3}):\d+$').firstMatch(id);
  if (endpoint != null) {
    final ip = endpoint.group(1)!;
    return SealDeskRecordLabel(
      primary: name.isEmpty ? ip : name,
      secondary: name.isEmpty ? null : ip,
      lanMark: true,
    );
  }
  if (id.contains('@')) {
    final host = id.split('@').last;
    return SealDeskRecordLabel(
      primary: name.isEmpty ? host : name,
      secondary: name.isEmpty ? null : host,
      lanMark: true,
    );
  }
  if (id.startsWith('tc')) {
    final short = sealDeskShortTc(id);
    return SealDeskRecordLabel(
      primary: name.isEmpty ? short : name,
      secondary: name.isEmpty ? null : short,
      lanMark: false,
    );
  }
  return SealDeskRecordLabel(
    primary: name.isEmpty ? id : name,
    secondary: name.isEmpty ? null : id,
    lanMark: false,
  );
}

bool _lanAddress(String value) {
  final matched =
      RegExp(r'^lan:(\d{1,3}(?:\.\d{1,3}){3}):(\d{1,5})/([A-Fa-f0-9]+)$')
          .firstMatch(value);
  if (matched == null) return false;
  return _ipv4(matched.group(1)!) && _port(matched.group(2)!);
}

bool _ipAddress(String value) {
  final matched =
      RegExp(r'^(\d{1,3}(?:\.\d{1,3}){3})(?::(\d{1,5}))?$').firstMatch(value);
  if (matched == null) return false;
  if (!_ipv4(matched.group(1)!)) return false;
  final port = matched.group(2);
  return port == null || _port(port);
}

bool _ipv4(String value) {
  final parts = value.split('.');
  if (parts.length != 4) return false;
  for (final part in parts) {
    final octet = int.tryParse(part);
    if (octet == null || octet > 255) return false;
  }
  return true;
}

bool _port(String value) {
  final port = int.tryParse(value);
  return port != null && port > 0 && port <= 65535;
}
