import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sealdesk/common.dart';

double _contrast(Color foreground, Color background) {
  final lighter = foreground.computeLuminance();
  final darker = background.computeLuminance();
  final hi = lighter > darker ? lighter : darker;
  final lo = lighter > darker ? darker : lighter;
  return (hi + 0.05) / (lo + 0.05);
}

void main() {
  Future<void> openMenu(
    WidgetTester tester,
    ThemeData theme,
    List<String> keys,
    List<String> values,
    ValueChanged<String> onChanged,
  ) async {
    await tester.pumpWidget(MaterialApp(
      theme: theme,
      home: Scaffold(
        body: ComboBox(
          keys: keys,
          values: values,
          initialKey: keys.first,
          onChanged: onChanged,
        ),
      ),
    ));
    await tester.pump();
    await tester.tap(find.byType(DropdownButton<String>));
    await tester.pumpAndSettle();
  }

  testWidgets('settings menu text contrasts with the menu background',
      (tester) async {
    final cases = <(ThemeData, List<String>, List<String>)>[
      (
        MyTheme.lightTheme,
        ['light', 'dark', 'system'],
        ['Light', 'Dark', 'Follow System'],
      ),
      (
        MyTheme.darkTheme,
        ['light', 'dark', 'system'],
        ['浅色', '深色', '跟随系统'],
      ),
    ];
    for (final item in cases) {
      String? selected;
      await openMenu(
          tester, item.$1, item.$2, item.$3, (key) => selected = key);
      final background = tester
          .widget<DropdownButton<String>>(find.byType(DropdownButton<String>))
          .dropdownColor;
      expect(background, item.$1.dialogBackgroundColor);
      for (final label in item.$3) {
        final texts = tester.widgetList<Text>(find.text(label));
        expect(texts, isNotEmpty);
        for (final text in texts) {
          final color = text.style?.color;
          expect(color, isNotNull, reason: label);
          expect(_contrast(color!, background!), greaterThan(4.5),
              reason: label);
          expect(text.maxLines, 1);
        }
      }
      await tester.tap(find.text(item.$3[1]).last);
      await tester.pumpAndSettle();
      expect(selected, item.$2[1]);
    }
  });
}
