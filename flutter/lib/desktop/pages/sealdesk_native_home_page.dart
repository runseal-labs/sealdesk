import 'dart:convert';
import 'dart:io';

import 'package:device_info_plus/device_info_plus.dart';
import 'package:file_picker/file_picker.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';

import 'package:flutter_rust_bridge/flutter_rust_bridge.dart';

import '../../common.dart';
import '../../common/sealdesk_pairing.dart';
import '../../common/sealdesk_qr_file.dart';
import '../../common/sealdesk_qr_image.dart';
import '../../consts.dart';
import '../../models/platform_model.dart';
import '../../models/server_model.dart' show kUsePermanentPassword;
import 'sealdesk_home_page.dart';
import '../../common/sealdesk_lan_panel.dart';

String? _androidShareName;

Future<String> _shareDeviceName() async {
  if (!isAndroid) return Platform.localHostname;
  final cached = _androidShareName;
  if (cached != null && cached.isNotEmpty) return cached;
  try {
    final info = await DeviceInfoPlugin().androidInfo;
    final name = '${info.brand}-${info.model}'.trim();
    if (name.isNotEmpty && name != '-') {
      _androidShareName = name;
      return name;
    }
  } catch (_) {}
  return Platform.localHostname;
}

class SealDeskNativeHomePage extends StatelessWidget {
  final String? initialInput;
  final Widget? footer;
  final bool hostOnly;

  const SealDeskNativeHomePage({
    super.key,
    this.initialInput,
    this.footer,
    this.hostOnly = false,
  });

  Future<SealDeskHostState> _loadHost() async {
    final address = await bind.mainGetOption(key: 'tailcat-addr');
    final method = await bind.mainGetOption(key: kOptionVerificationMethod);
    final approveMode = await bind.mainGetOption(key: kOptionApproveMode);
    var stopped = await mainGetBoolOption(kOptionStopService);
    // Screen capture is the Android share opt-in. Until it is on, do not
    // present an empty address as a pairing code that is about to appear.
    if (isAndroid && !gFFI.serverModel.isStart) stopped = true;
    final password = await bind.mainGetTemporaryPassword();
    final connectedCount = int.tryParse(
            await bind.mainGetOption(key: 'sealdesk-connected-count')) ??
        0;
    final includesPassword =
        method != kUsePermanentPassword && approveMode != 'click';
    return SealDeskHostState(
      address: includesPassword && password.isEmpty ? '' : address,
      password: includesPassword ? password : null,
      name: await _shareDeviceName(),
      stopped: stopped,
      connectedCount: connectedCount,
    );
  }

  Future<SealDeskPairing?> _pickImage() async {
    final result = await FilePicker.platform.pickFiles(
      type: FileType.custom,
      allowedExtensions: ['png', 'jpg', 'jpeg'],
    );
    if (result == null) return null;
    final path = result.files.single.path;
    if (path == null) throw const FormatException('无法读取所选图片，请重新选择');
    return loadSealDeskQrFile(path);
  }

  Future<SealDeskPairing> _pasteImage() async {
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

  @override
  Widget build(BuildContext context) => SealDeskHomePage(
        hostOnly: hostOnly,
        footer: footer,
        lanPanelBuilder: (host) => SealDeskLanPanel(host: host, connectPeer: (address) async {
          await connect(context, address, isSharedPassword: false);
        }),
        initialMode: hostOnly
            ? 'host'
            : bind.mainGetLocalOption(key: 'sealdesk-mode'),
        loadHost: _loadHost,
        initialInput: initialInput,
        pickImage: _pickImage,
        loadImageFile: loadSealDeskQrFile,
        pasteImage: _pasteImage,
        resetAddress: isAndroid || bind.isDisableSettings()
            ? null
            : () async {
                final error = await bind.mainResetSealdeskAddress();
                if (error.isNotEmpty) throw FormatException(error);
              },
        refreshPassword: () async {
          await bind.mainUpdateTemporaryPassword();
        },
        saveMode: (mode) async {
          await bind.mainSetLocalOption(key: 'sealdesk-mode', value: mode);
          final applied = await bind.mainGetOption(key: 'sealdesk-host-mode');
          if (applied != mode ||
              bind.mainGetLocalOption(key: 'sealdesk-mode') != mode) {
            throw FormatException(translate('Switch failed, please try again'));
          }
        },
        connect: (pairing) async {
          if (pairing.name != null && pairing.name!.isNotEmpty) {
            await bind.mainSetPeerAlias(
                id: pairing.address, alias: pairing.name!);
          }
          if (!context.mounted) return;
          pairing.encode();
          await connect(context, pairing.address,
              password: pairing.password, isSharedPassword: false);
        },
      );
}
