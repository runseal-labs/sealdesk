import 'dart:convert';
import 'dart:io';

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

class SealDeskNativeHomePage extends StatelessWidget {
  final String? initialInput;

  const SealDeskNativeHomePage({super.key, this.initialInput});

  Future<SealDeskHostState> _loadHost() async {
    final address = await bind.mainGetOption(key: 'tailcat-addr');
    final method = await bind.mainGetOption(key: kOptionVerificationMethod);
    final approveMode = await bind.mainGetOption(key: kOptionApproveMode);
    final stopped = await mainGetBoolOption(kOptionStopService);
    final password = await bind.mainGetTemporaryPassword();
    final includesPassword =
        method != kUsePermanentPassword && approveMode != 'click';
    return SealDeskHostState(
      address: includesPassword && password.isEmpty ? '' : address,
      password: includesPassword ? password : null,
      name: Platform.localHostname,
      stopped: stopped,
      connectedNames: [],
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
      final reply = jsonDecode(await bind.mainGetSealdeskClipboardImage()) as Map<String, dynamic>;
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
        initialMode: bind.mainGetLocalOption(key: 'sealdesk-mode'),
        loadHost: _loadHost,
        initialInput: initialInput,
        pickImage: _pickImage,
        loadImageFile: loadSealDeskQrFile,
        pasteImage: _pasteImage,
        resetAddress: bind.isDisableSettings()
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
            throw const FormatException('未能切换被控服务，请稍后重试');
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
