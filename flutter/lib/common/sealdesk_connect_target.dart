import 'sealdesk_pairing.dart';
import '../models/platform_model.dart';

const kSealDeskHint = 'tc… / lan: / SEALDESK: pairing code';
const kSealDeskCompatChip = 'Compatible with RustDesk · Unencrypted';
const kSealDeskEncryptedChip = 'Encrypted direct connection';
const kSealDeskUnverifiedIdentity = 'Identity not verified';
const kSealDeskUnverifiedIdentityPrompt =
    'This device did not complete an identity check. Connect only if you trust it on this network.';
const kSealDeskCompatHint = 'compat-lan-hint';
const kSealDeskCompatEnableWarning = 'compat-lan-enable-warning';
const kSealDeskClearHistory = 'Clear history';
const kSealDeskClearHistoryTitle = 'Clear connection history?';
const kSealDeskClearHistoryBody = 'Favorites are kept.';
const kSealDeskNoRecords = 'No records';
const kSealDeskNoFavorites = 'No favorites';
const kSealDeskSearching = 'Searching…';
const kSealDeskScanTooltip = 'Scan to connect';
const kSealDeskScreenTooltip = 'Recognize screen or clipboard QR';
const kSealDeskGalleryTooltip = 'Recognize QR from gallery';
const kSealDeskImageTooltip = 'Recognize QR from image';
const kSealDeskInvalidInput = 'Invalid connection address';
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

String? sealDeskLanIp(String id) {
  final matched =
      RegExp(r'^lan:(\d{1,3}(?:\.\d{1,3}){3}):\d{1,5}/').firstMatch(id);
  return matched?.group(1);
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
