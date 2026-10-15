import 'dart:typed_data';

import 'package:flutter_hbb/common/taildesk_pairing.dart';
import 'package:flutter_hbb/common/taildesk_qr_image.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:image/image.dart' as img;
import 'package:zxing2/qrcode.dart';

const address = 'tcABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdef_-';

img.Image qrImage(String text,
    {bool transparent = false, CharacterSetECI? characterSet}) {
  final hints = EncodeHints()
    ..put(EncodeHintType.characterSet, characterSet ?? CharacterSetECI.UTF8);
  final matrix =
      Encoder.encode(text, ErrorCorrectionLevel.m, hints: hints).matrix!;
  const scale = 6;
  final image = img.Image(
      width: (matrix.width + 8) * scale,
      height: (matrix.height + 8) * scale,
      numChannels: 4);
  img.fill(image, color: img.ColorRgba8(255, 255, 255, transparent ? 0 : 255));
  for (var row = 0; row < matrix.height; row++) {
    for (var column = 0; column < matrix.width; column++) {
      if (matrix.get(column, row) == 1) {
        img.fillRect(image,
            x1: (column + 4) * scale,
            y1: (row + 4) * scale,
            x2: (column + 5) * scale - 1,
            y2: (row + 5) * scale - 1,
            color: img.ColorRgba8(0, 0, 0, 255));
      }
    }
  }
  return image;
}

void main() {
  const pairing = TailDeskPairing(
      address: address, password: ' p;,:" 中文 ', name: '卧室电脑🖥', expiresAt: 200);

  test('PNG and JPEG images preserve Unicode names and password escaping', () {
    final image = qrImage(pairing.encode(now: 100));
    for (final bytes in [
      img.encodePng(image),
      img.encodeJpg(image, quality: 85)
    ]) {
      final decoded = decodeTailDeskQrImage(bytes, now: 100);
      expect(decoded.address, address);
      expect(decoded.password, pairing.password);
      expect(decoded.name, pairing.name);
      expect(decoded.expiresAt, pairing.expiresAt);
    }
  });

  test('transparent and rotated screenshots remain readable', () {
    final image = qrImage(pairing.encode(now: 100), transparent: true);
    for (final rotated in [
      image,
      img.copyRotate(image, angle: 90),
      img.copyRotate(image, angle: 180)
    ]) {
      final decoded = decodeTailDeskQrImage(img.encodePng(rotated), now: 100);
      expect(decoded.password, pairing.password);
    }
  });

  test('foreign QR payloads and expired pairing codes never connect', () {
    expect(
        () => decodeTailDeskQrImage(img.encodePng(qrImage('WIFI:S:network;;')),
            now: 100),
        throwsFormatException);
    expect(
        () => decodeTailDeskQrImage(
            img.encodePng(qrImage(pairing.encode(now: 100))),
            now: 200),
        throwsA(isA<FormatException>()
            .having((error) => error.message, 'reason', contains('已过期'))));
  });

  test('missing QR and malformed images produce actionable errors', () {
    final blank = img.Image(width: 200, height: 200);
    img.fill(blank, color: img.ColorRgb8(255, 255, 255));
    for (final bytes in [
      Uint8List(0),
      Uint8List.fromList([1, 2, 3]),
      img.encodePng(blank)
    ]) {
      expect(
          () => decodeTailDeskQrImage(bytes, now: 100), throwsFormatException);
    }
  });

  test('large files and dimensions are rejected before QR decoding', () {
    expect(() => decodeTailDeskQrImage(Uint8List(maxTailDeskQrImageBytes + 1)),
        throwsFormatException);
    final wide = img.Image(width: 8193, height: 1);
    expect(
        () => decodeTailDeskQrImage(img.encodePng(wide)),
        throwsA(isA<FormatException>()
            .having((error) => error.message, 'reason', contains('尺寸过大'))));
  });

  test('invalid UTF-8 is rejected with an actionable Chinese error', () {
    final image = qrImage('TAILDESK:V:1;A:$address;P:é;;',
        characterSet: CharacterSetECI.ISO8859_1);
    expect(() => decodeTailDeskQrImage(img.encodePng(image), now: 100),
        throwsA(isA<FormatException>().having((error) => error.message,
            'reason', contains('有效的 UTF-8'))));
  });
}
