import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge.dart';

import '../common.dart';
import '../consts.dart';
import '../models/platform_model.dart';
import 'sealdesk_connect_target.dart';
export 'sealdesk_connect_target.dart' show kSealDeskCompatEnableWarning;
import 'sealdesk_connection_record.dart';
import 'sealdesk_pairing.dart';
import 'sealdesk_qr_file.dart';
import 'sealdesk_qr_image.dart';

const kSealDeskRecordsOption = 'sealdesk-connection-records';
String? _sealDeskRecordsRaw;

void ensureSealDeskRecordsLoaded() {
  String raw;
  try {
    raw = bind.mainGetLocalOption(key: kSealDeskRecordsOption);
  } catch (_) {
    return;
  }
  if (raw == _sealDeskRecordsRaw) return;
  _sealDeskRecordsRaw = raw;
  if (raw.isEmpty) return;
  try {
    final decoded = jsonDecode(raw);
    if (decoded is! Map) return;
    sealDeskConnectionRecords.replaceWith(
      SealDeskConnectionRecordStore.fromJson(
          Map<String, dynamic>.from(decoded)),
    );
  } catch (_) {}
}

Future<void> persistSealDeskRecords() async {
  final raw = jsonEncode(sealDeskConnectionRecords.toJson());
  _sealDeskRecordsRaw = raw;
  try {
    await bind.mainSetLocalOption(key: kSealDeskRecordsOption, value: raw);
  } catch (_) {}
}

List<String> sealDeskVisibleRecentIds(Iterable<String> ids) {
  ensureSealDeskRecordsLoaded();
  return sealDeskPreferredRecentIds(
    ids,
    currentAddress: (id) => sealDeskConnectionRecords.get(id)?.address,
  );
}

class SealDeskLanDevice {
  final String name;
  final String ip;
  final String address;
  final String protocol;
  final String platform;
  final bool identityChecked;

  const SealDeskLanDevice({
    required this.name,
    required this.ip,
    required this.address,
    required this.protocol,
    this.platform = '',
    this.identityChecked = true,
  });

  bool get encrypted => protocol != 'rustdesk';

  bool get needsIdentityConfirmation =>
      sealDeskLanNeedsIdentityConfirmation(protocol, identityChecked);

  bool get showsTerminal => platform != kPeerPlatformAndroid;

  factory SealDeskLanDevice.fromJson(Map<String, dynamic> json) {
    return SealDeskLanDevice(
      name: json['name'] as String? ?? '',
      ip: json['ip'] as String? ?? '',
      address: json['address'] as String? ?? '',
      protocol: json['protocol'] as String? ?? 'sealdesk',
      platform: json['platform'] as String? ?? '',
      identityChecked: json['identity_checked'] as bool? ?? true,
    );
  }
}

bool sealDeskLanNeedsIdentityConfirmation(String protocol, bool identityChecked) {
  return protocol != 'rustdesk' && !identityChecked;
}

Future<bool> confirmSealDeskUnverifiedLan(BuildContext context) async {
  final accepted = await showDialog<bool>(
    context: context,
    builder: (context) => AlertDialog(
      key: const ValueKey('sealdesk-unverified-lan'),
      title: Text(translate(kSealDeskUnverifiedIdentity)),
      content: Text(translate(kSealDeskUnverifiedIdentityPrompt)),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context, false),
          child: Text(translate('Cancel')),
        ),
        TextButton(
          key: const ValueKey('sealdesk-unverified-lan-continue'),
          onPressed: () => Navigator.pop(context, true),
          child: Text(translate('OK')),
        ),
      ],
    ),
  );
  return accepted == true;
}

class SealDeskLanDirectory extends ChangeNotifier {
  SealDeskLanDirectory._();
  static final instance = SealDeskLanDirectory._();

  List<SealDeskLanDevice> devices = [];
  bool searching = true;
  Timer? _timer;
  int _users = 0;
  bool _loading = false;

  void retain() {
    _users++;
    if (_timer == null) {
      _poll();
      _timer = Timer.periodic(const Duration(seconds: 3), (_) => _poll());
    }
  }

  void release() {
    if (_users > 0) _users--;
    if (_users == 0) {
      _timer?.cancel();
      _timer = null;
    }
  }

  SealDeskLanDevice? match(String peerId) {
    for (final device in devices) {
      if (device.address.isNotEmpty && device.address == peerId) return device;
      if (device.ip.isNotEmpty &&
          (device.ip == peerId || peerId.startsWith('${device.ip}:'))) {
        return device;
      }
      final ip = sealDeskLanIp(peerId);
      if (ip != null && ip == device.ip) return device;
    }
    return null;
  }

  Future<void> _poll() async {
    if (_loading) return;
    _loading = true;
    try {
      final json = await bind.mainGetOption(key: 'sealdesk-lan-peers');
      final decoded = json.isEmpty ? <dynamic>[] : jsonDecode(json) as List<dynamic>;
      final next = <SealDeskLanDevice>[];
      for (final item in decoded) {
        if (item is Map) {
          next.add(SealDeskLanDevice.fromJson(Map<String, dynamic>.from(item)));
        }
      }
      devices = next;
      searching = next.isEmpty;
    } catch (_) {
      searching = devices.isEmpty;
    } finally {
      _loading = false;
      notifyListeners();
    }
  }
}

Future<void> prepareSealDeskRecord(SealDeskConnectTarget target) async {
  if (!target.ok) return;
  ensureSealDeskRecordsLoaded();
  if (target.alias != null && target.alias!.isNotEmpty) {
    await bind.mainSetPeerAlias(id: target.id, alias: target.alias!);
  }
  await bind.mainSetPeerOption(
      id: target.id, key: kSealDeskPasswordKind, value: target.passwordKind);
  await bind.mainSetPeerOption(id: target.id, key: kSealDeskHideRecent, value: '');
  final permanent = target.passwordKind == kSealDeskPasswordPermanent;
  if (!permanent) {
    await bind.mainForgetPassword(id: target.id);
  }
  try {
    sealDeskConnectionRecords.upsert(SealDeskConnectEvent(
      address: target.id,
      alias: target.alias ?? '',
      password: target.password ?? '',
      usedPermanentPassword: permanent,
      rememberPassword: false,
      connectedAtMillis: DateTime.now().millisecondsSinceEpoch,
    ));
  } on FormatException {
    return;
  }
  await persistSealDeskRecords();
}

Future<void> connectSealDeskTarget(
  BuildContext context,
  SealDeskConnectTarget target, {
  bool isFileTransfer = false,
  bool isViewCamera = false,
  bool isTerminal = false,
  bool isTcpTunneling = false,
}) async {
  if (!target.ok) {
    _showConnectError(context, translate(target.error ?? kSealDeskInvalidInput));
    return;
  }
  await prepareSealDeskRecord(target);
  if (!context.mounted) return;
  await connect(
    context,
    target.id,
    password: target.password,
    isSharedPassword: false,
    isFileTransfer: isFileTransfer,
    isViewCamera: isViewCamera,
    isTerminal: isTerminal,
    isTcpTunneling: isTcpTunneling,
  );
}

Future<void> connectSealDeskInput(
  BuildContext context,
  String raw, {
  bool isFileTransfer = false,
  bool isViewCamera = false,
  bool isTerminal = false,
  bool isTcpTunneling = false,
  int? now,
}) {
  return connectSealDeskTarget(
    context,
    parseSealDeskConnectInput(raw, now: now),
    isFileTransfer: isFileTransfer,
    isViewCamera: isViewCamera,
    isTerminal: isTerminal,
    isTcpTunneling: isTcpTunneling,
  );
}

Future<void> connectSealDeskPairing(
    BuildContext context, SealDeskPairing pairing) {
  pairing.encode();
  final temporary = pairing.password != null && pairing.password!.isNotEmpty;
  return connectSealDeskTarget(
    context,
    SealDeskConnectTarget(
      id: pairing.address,
      password: pairing.password,
      alias: pairing.name,
      kind: 'pairing',
      passwordKind: temporary
          ? kSealDeskPasswordTemporary
          : kSealDeskPasswordPermanent,
    ),
  );
}

Future<void> importSealDeskStartup(BuildContext context, String input) async {
  final text = input.trim();
  if (text.isEmpty) return;
  if (SealDeskPairing.isPairingCode(text) ||
      text.toLowerCase().startsWith('lan:') ||
      text.startsWith('tc') ||
      RegExp(r'^\d{1,3}(?:\.\d{1,3}){3}').hasMatch(text)) {
    await connectSealDeskInput(context, text);
    return;
  }
  try {
    final pairing = await loadSealDeskQrFile(text);
    if (!context.mounted) return;
    await connectSealDeskPairing(context, pairing);
  } on FormatException catch (error) {
    if (context.mounted) _showConnectError(context, translate(error.message));
  }
}

Future<SealDeskPairing> pasteSealDeskClipboardImage() async {
  try {
    final reply = jsonDecode(await bind.mainGetSealdeskClipboardImage())
        as Map<String, dynamic>;
    final error = reply['error'];
    if (error is String) throw FormatException(error);
    final bytes = base64Decode(reply['png'] as String);
    return await compute(decodeSealDeskQrImage, bytes);
  } on FfiException catch (error) {
    throw FormatException(error.message);
  }
}

bool sealDeskHistoryVisible(String id) {
  if (!isSealDeskApp()) return true;
  try {
    return bind.mainGetPeerOptionSync(id: id, key: kSealDeskHideRecent) != 'Y';
  } catch (_) {
    return true;
  }
}

final sealDeskConnectionRecords = SealDeskConnectionRecordStore();

Future<void> clearSealDeskHistory(Iterable<String> ids) async {
  ensureSealDeskRecordsLoaded();
  sealDeskConnectionRecords.clearHistory();
  await persistSealDeskRecords();
  var hidden = false;
  try {
    await bind.mainHideSealdeskRecentPeers();
    hidden = true;
  } catch (_) {}
  if (!hidden) {
    for (final id in ids) {
      final trimmed = id.trim();
      if (trimmed.isEmpty) continue;
      await bind.mainSetPeerOption(
          id: trimmed, key: kSealDeskHideRecent, value: 'Y');
    }
  }
  try {
    bind.mainLoadRecentPeers();
    gFFI.recentPeersModel.bump();
  } catch (_) {}
}

Future<bool> confirmSealDeskCompatEnable(BuildContext context) async {
  final enabled = await showDialog<bool>(
    context: context,
    builder: (context) => AlertDialog(
      key: const ValueKey('sealdesk-compat-warning'),
      title: Text(translate('Allow RustDesk compatible connections')),
      content: Text(translate(kSealDeskCompatEnableWarning)),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context, false),
          child: Text(translate('Cancel')),
        ),
        TextButton(
          onPressed: () => Navigator.pop(context, true),
          child: Text(translate('OK')),
        ),
      ],
    ),
  );
  return enabled == true;
}

Future<void> maybeShowSealDeskCompatHint(BuildContext context) async {
  try {
    if (bind.mainGetLocalOption(key: kSealDeskCompatHintOption) == 'Y') return;
    await bind.mainSetLocalOption(key: kSealDeskCompatHintOption, value: 'Y');
  } catch (_) {
    return;
  }
  if (!context.mounted) return;
  final message = translate(kSealDeskCompatHint);
  final messenger = ScaffoldMessenger.maybeOf(context);
  if (messenger != null) {
    messenger.showSnackBar(SnackBar(content: Text(message)));
  } else {
    showToast(message);
  }
}

void _showConnectError(BuildContext context, String message) {
  showToast(message);
  final messenger = ScaffoldMessenger.maybeOf(context);
  messenger?.showSnackBar(SnackBar(
    content: Text(message, key: const ValueKey('sealdesk-connect-error')),
  ));
}
