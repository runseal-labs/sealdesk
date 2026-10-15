import 'package:flutter/material.dart';
import 'package:image_picker/image_picker.dart';

import '../common.dart';
import '../common/taildesk_pairing.dart';
import '../common/taildesk_qr_file.dart';
import '../models/platform_model.dart';
import 'pages/taildesk_scan_page.dart';

Future<void> scanAndConnectTailDesk(BuildContext context) async {
  try {
    final pairing = await Navigator.of(context).push<TailDeskPairing>(
      MaterialPageRoute(
          builder: (_) => TailDeskScanPage(
                pickImage: () async {
                  final file = await ImagePicker()
                      .pickImage(source: ImageSource.gallery);
                  if (file == null) return null;
                  return loadTailDeskQrFile(file.path);
                },
              )),
    );
    if (!context.mounted || pairing == null) return;
    pairing.encode();
    if (pairing.name != null && pairing.name!.isNotEmpty) {
      await bind.mainSetPeerAlias(id: pairing.address, alias: pairing.name!);
    }
    if (!context.mounted) return;
    pairing.encode();
    await connect(context, pairing.address,
        password: pairing.password, isSharedPassword: false);
  } on FormatException catch (error) {
    if (context.mounted) showToast(error.message);
  } catch (_) {
    if (context.mounted) showToast('无法打开扫码或建立连接，请重试');
  }
}
