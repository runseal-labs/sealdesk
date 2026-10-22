import 'sealdesk_pairing.dart';
import '../models/platform_model.dart';

const kSealDeskHint = 'tc… / lan: / SEALDESK: pairing code';
const kSealDeskCompatChip = 'Compatible with RustDesk · Unencrypted';
const kSealDeskEncryptedChip = 'Encrypted direct connection';
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
  final String? error;

  const SealDeskConnectTarget({
    required this.id,
    this.password,
    this.alias,
    required this.kind,
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
      error: kSealDeskInvalidInput,
    );
  }
  if (SealDeskPairing.isPairingCode(text)) {
    try {
      final pairing = SealDeskPairing.parse(text, now: now);
      return SealDeskConnectTarget(
        id: pairing.address,
        password: pairing.password,
        alias: pairing.name,
        kind: 'pairing',
      );
    } on FormatException catch (error) {
      return SealDeskConnectTarget(
        id: '',
        kind: 'invalid',
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
        error: kSealDeskInvalidInput,
      );
    }
    return SealDeskConnectTarget(
      id: normalized,
      kind: 'lan',
    );
  }
  if (SealDeskPairing.isAddress(text)) {
    return SealDeskConnectTarget(
      id: text,
      kind: 'tc',
    );
  }
  if (_ipAddress(text)) {
    return SealDeskConnectTarget(
      id: text,
      kind: 'ip',
    );
  }
  return const SealDeskConnectTarget(
    id: '',
    kind: 'invalid',
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
  if (matched != null) {
    if (!_ipv4(matched.group(1)!)) return false;
    final port = matched.group(2);
    return port == null || _port(port);
  }
  return _ipv6(value);
}

/// `host`, `host:port` is IPv4 above. IPv6 is `hextets` or `[hextets]:port`.
bool _ipv6(String value) {
  var host = value;
  String? port;
  if (value.startsWith('[')) {
    final end = value.indexOf(']');
    if (end <= 1 || value.indexOf('[', 1) != -1) return false;
    host = value.substring(1, end);
    final rest = value.substring(end + 1);
    if (rest.isEmpty) {
      port = null;
    } else if (rest.startsWith(':') && rest.length > 1) {
      port = rest.substring(1);
    } else {
      return false;
    }
  } else if (value.contains('%') || value.contains(']')) {
    return false;
  }
  if (port != null && !_port(port)) return false;
  return _ipv6Host(host);
}

bool _ipv6Host(String host) {
  if (host.isEmpty || host.contains(':::')) return false;
  final halves = host.split('::');
  if (halves.length > 2) return false;
  if (halves.length == 1) {
    final parts = host.split(':');
    return parts.length == 8 && parts.every(_hextet);
  }
  final left = halves[0].isEmpty ? const <String>[] : halves[0].split(':');
  final right = halves[1].isEmpty ? const <String>[] : halves[1].split(':');
  if (left.isEmpty && right.isEmpty) return host == '::';
  if (left.length + right.length >= 8) return false;
  return left.every(_hextet) && right.every(_hextet);
}

bool _hextet(String part) {
  return part.length <= 4 && RegExp(r'^[0-9a-fA-F]{1,4}$').hasMatch(part);
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
