import 'dart:convert';
import 'dart:typed_data';

import 'package:image/image.dart' as img;
import 'package:zxing2/qrcode.dart';

import 'sealdesk_pairing.dart';

const maxSealDeskQrImageBytes = 20 * 1024 * 1024;

SealDeskPairing decodeSealDeskQrImage(Uint8List bytes, {int? now}) {
  if (bytes.isEmpty || bytes.length > maxSealDeskQrImageBytes) {
    throw const FormatException(
        'Image is empty or larger than 20 MB. Choose a smaller QR screenshot');
  }
  img.Image? image;
  try {
    final decoder = img.findDecoderForData(bytes);
    final info = decoder?.startDecode(bytes);
    if (info == null) {
      throw const FormatException(
          'Unable to read the image. Choose a PNG or JPG screenshot');
    }
    if (info.width <= 0 ||
        info.height <= 0 ||
        info.width > 8192 ||
        info.height > 8192 ||
        info.width * info.height > 16 * 1024 * 1024) {
      throw const FormatException(
          'Image is too large. Crop to the QR code and try again');
    }
    image = decoder!.decodeFrame(0);
  } on FormatException {
    rethrow;
  } catch (_) {
    throw const FormatException(
        'Image is corrupt or the format is not supported');
  }
  if (image == null) {
    throw const FormatException(
        'Unable to read the image. Choose a PNG or JPG screenshot');
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
        throw const FormatException(
            'QR text encoding is not recognized. Use the pairing code text instead');
      }
      final signed = segments.single as Int8List;
      if (utf8.decode(signed, allowMalformed: true) != payload) {
        throw const FormatException(
            'QR text encoding is not recognized. Use the pairing code text instead');
      }
      try {
        payload = utf8.decode(Uint8List.fromList(signed));
      } on FormatException {
        throw const FormatException(
            'QR code is not valid UTF-8 text. Use the pairing code text instead');
      }
    }
  } on ReaderException {
    throw const FormatException(
        'No readable QR code was found in the image. Take a clearer screenshot');
  }
  return SealDeskPairing.parse(payload, now: now);
}
