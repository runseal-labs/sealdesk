import 'dart:convert';
import 'dart:typed_data';

import 'package:image/image.dart' as img;
import 'package:zxing2/qrcode.dart';

import 'sealdesk_pairing.dart';

const maxSealDeskQrImageBytes = 20 * 1024 * 1024;

SealDeskPairing decodeSealDeskQrImage(Uint8List bytes, {int? now}) {
  if (bytes.isEmpty || bytes.length > maxSealDeskQrImageBytes) {
    throw const FormatException('图片为空或超过 20 MB，请选择较小的二维码截图');
  }
  img.Image? image;
  try {
    final decoder = img.findDecoderForData(bytes);
    final info = decoder?.startDecode(bytes);
    if (info == null) {
      throw const FormatException('无法读取图片，请选择 PNG 或 JPG 截图');
    }
    if (info.width <= 0 ||
        info.height <= 0 ||
        info.width > 8192 ||
        info.height > 8192 ||
        info.width * info.height > 16 * 1024 * 1024) {
      throw const FormatException('图片尺寸过大，请裁剪到二维码区域后重试');
    }
    image = decoder!.decodeFrame(0);
  } on FormatException {
    rethrow;
  } catch (_) {
    throw const FormatException('图片已损坏或格式不受支持');
  }
  if (image == null) {
    throw const FormatException('无法读取图片，请选择 PNG 或 JPG 截图');
  }
  final pixels = Int32List(image.width * image.height);
  var index = 0;
  for (final pixel in image) {
    final alpha = pixel.aNormalized;
    final background = 255 * (1 - alpha);
    final red = (pixel.rNormalized * 255 * alpha + background).round();
    final green = (pixel.gNormalized * 255 * alpha + background).round();
    final blue = (pixel.bNormalized * 255 * alpha + background).round();
    pixels[index++] = red << 16 | green << 8 | blue;
  }
  final source = RGBLuminanceSource(image.width, image.height, pixels);
  String payload;
  try {
    final hints = DecodeHints()
      ..put(DecodeHintType.tryHarder)
      ..put(DecodeHintType.characterSet, 'UTF-8');
    final result = QRCodeReader()
        .decode(BinaryBitmap(HybridBinarizer(source)), hints: hints);
    payload = result.text;
    if (payload.contains('\uFFFD')) {
      final segments = result.resultMetadata[ResultMetadataType.byteSegments];
      if (segments is! List ||
          segments.length != 1 ||
          segments.single is! Int8List) {
        throw const FormatException('二维码文字编码无法识别，请改用配对码文本');
      }
      final signed = segments.single as Int8List;
      if (utf8.decode(signed, allowMalformed: true) != payload) {
        throw const FormatException('二维码文字编码无法识别，请改用配对码文本');
      }
      try {
        payload = utf8.decode(Uint8List.fromList(signed));
      } on FormatException {
        throw const FormatException('二维码不是有效的 UTF-8 文本，请改用配对码文本');
      }
    }
  } on ReaderException {
    throw const FormatException('未在图片中找到可识别的二维码，请重新截取清晰图片');
  }
  return SealDeskPairing.parse(payload, now: now);
}
