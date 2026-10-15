import 'dart:io';

import 'package:flutter/foundation.dart';

import 'sealdesk_pairing.dart';
import 'sealdesk_qr_image.dart';

Future<SealDeskPairing> loadSealDeskQrFile(String path) async {
  final file = File(path);
  if (await file.length() > maxSealDeskQrImageBytes) {
    throw const FormatException('图片超过 20 MB，请选择较小的二维码截图');
  }
  final reader = await file.open();
  try {
    final bytes = await reader.read(maxSealDeskQrImageBytes + 1);
    return await compute(decodeSealDeskQrImage, bytes);
  } finally {
    await reader.close();
  }
}
