import 'dart:io';

import 'package:flutter/foundation.dart';

import 'taildesk_pairing.dart';
import 'taildesk_qr_image.dart';

Future<TailDeskPairing> loadTailDeskQrFile(String path) async {
  final file = File(path);
  if (await file.length() > maxTailDeskQrImageBytes) {
    throw const FormatException('图片超过 20 MB，请选择较小的二维码截图');
  }
  final reader = await file.open();
  try {
    final bytes = await reader.read(maxTailDeskQrImageBytes + 1);
    return await compute(decodeTailDeskQrImage, bytes);
  } finally {
    await reader.close();
  }
}
