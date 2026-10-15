import 'dart:io';

import 'package:flutter_hbb/common/taildesk_pairing.dart';
import 'package:flutter_hbb/common/taildesk_qr_file.dart';
import 'package:flutter_hbb/common/taildesk_qr_image.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qr_flutter/qr_flutter.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  late Directory directory;
  setUp(() async {
    directory = await Directory.systemTemp.createTemp('taildesk-qr-file-');
  });
  tearDown(() async => directory.delete(recursive: true));

  test('actual PNG file decodes in an isolate with Unicode credentials intact',
      () async {
    const pairing = TailDeskPairing(
        address: 'tcABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdef_-',
        password: ' 密码; image ',
        name: '电脑');
    final bytes = await QrPainter(
            data: pairing.encode(),
            version: QrVersions.auto,
            errorCorrectionLevel: QrErrorCorrectLevel.M)
        .toImageData(400);
    final file = await File('${directory.path}/pairing.png')
        .writeAsBytes(bytes!.buffer.asUint8List());
    final decoded = await loadTailDeskQrFile(file.path);
    expect(decoded.address, pairing.address);
    expect(decoded.password, pairing.password);
    expect(decoded.name, pairing.name);
  });

  test('oversized and corrupt files are rejected with Chinese errors',
      () async {
    final file = File('${directory.path}/invalid.png');
    final handle = await file.open(mode: FileMode.write);
    await handle.truncate(maxTailDeskQrImageBytes + 1);
    await handle.close();
    await expectLater(
        loadTailDeskQrFile(file.path),
        throwsA(isA<FormatException>()
            .having((error) => error.message, 'reason', contains('超过 20 MB'))));
    await file.writeAsBytes([1, 2, 3]);
    await expectLater(
        loadTailDeskQrFile(file.path),
        throwsA(isA<FormatException>().having(
            (error) => error.message, 'reason', contains('图片已损坏或格式不受支持'))));
  });
}
