import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sealdesk/common/widgets/id_text.dart';

void main() {
  test('ellipsizeDeviceId keeps short values and caps long ones', () {
    expect(ellipsizeDeviceId('123 456 789'), '123 456 789');
    expect(ellipsizeDeviceId('a' * kDeviceIdTitleMaxChars),
        'a' * kDeviceIdTitleMaxChars);
    final long = 'tcp:192.168.1.20:21116/${'x' * 40}';
    final shown = ellipsizeDeviceId(long);
    expect(shown.length, kDeviceIdTitleMaxChars);
    expect(shown.endsWith('…'), isTrue);
    expect(shown.substring(0, shown.length - 1),
        long.substring(0, kDeviceIdTitleMaxChars - 1));
    expect(ellipsizeDeviceId('line\nmore', maxChars: 8), 'line mo…');
    expect(ellipsizeDeviceId('abcd', maxChars: 1), '…');
    expect(ellipsizeDeviceId('abcd', maxChars: 0), isEmpty);
  });

  testWidgets('IdText ellipsizes a long id and keeps the full string',
      (tester) async {
    const id = 'tcp:10.0.0.8:21118/tcABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789';
    await tester.pumpWidget(
        const MaterialApp(home: IdText(id, style: TextStyle(fontSize: 22))));
    final text = tester.widget<Text>(find.byType(Text));
    expect(text.data, id);
    expect(text.maxLines, 1);
    expect(text.softWrap, isFalse);
    expect(text.overflow, TextOverflow.ellipsis);
    final paragraph = tester.renderObject<RenderParagraph>(find.byType(Text));
    expect(paragraph.didExceedMaxLines, isTrue);
    expect(paragraph.size.width, lessThanOrEqualTo(kDeviceIdMaxWidth));
  });

  testWidgets('a formatted numeric id stays whole at the local id size',
      (tester) async {
    const id = '123 456 789';
    await tester.pumpWidget(const MaterialApp(
        home: IdText(id,
            style: TextStyle(fontSize: 25, fontWeight: FontWeight.bold))));
    final paragraph = tester.renderObject<RenderParagraph>(find.byType(Text));
    expect(paragraph.didExceedMaxLines, isFalse);
    expect(tester.widget<Text>(find.byType(Text)).data, id);
  });

  testWidgets('IdText does not widen a row past its cap', (tester) async {
    const id = 'tcABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefEXTRA';
    await tester.pumpWidget(MaterialApp(
      home: Scaffold(
        body: Row(
          children: [
            IdText(id),
            const Text('tail'),
          ],
        ),
      ),
    ));
    final idBox = tester.getRect(find.byType(IdText));
    final tail = tester.getTopLeft(find.text('tail'));
    expect(idBox.width, lessThanOrEqualTo(kDeviceIdMaxWidth + 1));
    expect(tail.dx, lessThanOrEqualTo(kDeviceIdMaxWidth + 1));
    expect(tester.widget<Text>(find.byType(Text).first).data, id);
  });
}
