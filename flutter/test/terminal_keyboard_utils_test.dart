import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sealdesk/mobile/terminal_keyboard_utils.dart';
import 'package:sealdesk/mobile/widgets/terminal_quick_key_bar.dart';

void main() {
  group('hapi quick key layout', () {
    test('keeps the contributed two-row order', () {
      expect(terminalQuickKeyRows, hasLength(2));
      expect(
        terminalQuickKeyLabels(),
        [
          ['Esc', '/', '-', 'Home', '↑', 'End', 'PgUp'],
          ['Tab', 'Ctrl', 'Alt', '←', '↓', '→', 'PgDn'],
        ],
      );
      expect(terminalQuickKeyRows[0][1].popup?.label, '?');
      expect(terminalQuickKeyRows[0][1].popup?.sequence, '?');
      expect(terminalQuickKeyRows[0][2].popup?.label, '|');
      expect(terminalQuickKeyRows[0][2].popup?.sequence, '|');
      expect(terminalQuickKeyRows[1][1].modifier, TerminalQuickModifier.ctrl);
      expect(terminalQuickKeyRows[1][2].modifier, TerminalQuickModifier.alt);
      expect(
        terminalQuickKeyRows.expand((row) => row).map((key) => key.label),
        isNot(contains('Ctrl+C')),
      );
    });

    test('one-shot sticky modifiers reset, and a second tap unlocks', () {
      const idle = TerminalQuickModifierState();
      final ctrl = idle.toggle(TerminalQuickModifier.ctrl);
      expect(ctrl.ctrl, isTrue);
      expect(ctrl.alt, isFalse);
      expect(ctrl.isLocked, isTrue);

      final dispatched = dispatchQuickKeySequence('c', modifiers: ctrl);
      expect(dispatched.sequence, '\x03');
      expect(dispatched.reset, isTrue);

      final unlocked = ctrl.toggle(TerminalQuickModifier.ctrl);
      expect(unlocked.ctrl, isFalse);
      expect(unlocked.isLocked, isFalse);
      final plain = dispatchQuickKeySequence('c', modifiers: unlocked);
      expect(plain.sequence, 'c');
      expect(plain.reset, isFalse);

      final alt = ctrl.toggle(TerminalQuickModifier.alt);
      expect(alt.ctrl, isFalse);
      expect(alt.alt, isTrue);
      final altLetter = dispatchQuickKeySequence('x', modifiers: alt);
      expect(altLetter.sequence, '\x1Bx');
      expect(altLetter.reset, isTrue);

      final arrow = dispatchQuickKeySequence('\x1B[D', modifiers: alt);
      expect(arrow.sequence, '\x1B\x1B[D');
      expect(arrow.reset, isTrue);

      final slash = dispatchQuickKeySequence('/', modifiers: ctrl);
      expect(slash.sequence, '/');
      expect(slash.reset, isTrue);
    });
  });

  testWidgets('quick key row order, toggle, and long press', (tester) async {
    final sequences = <String>[];
    var modifiers = const TerminalQuickModifierState();
    Future<void> pump() {
      return tester.pumpWidget(MaterialApp(
        home: StatefulBuilder(
          builder: (context, setState) {
            return TerminalQuickKeyBar(
              ctrlActive: modifiers.ctrl,
              altActive: modifiers.alt,
              onToggleModifier: (modifier) {
                setState(() {
                  modifiers = modifiers.toggle(modifier);
                });
              },
              onSequence: (sequence) {
                final dispatched = dispatchQuickKeySequence(
                  sequence,
                  modifiers: modifiers,
                );
                sequences.add(dispatched.sequence);
                if (dispatched.reset) {
                  setState(() => modifiers = modifiers.cleared());
                }
              },
            );
          },
        ),
      ));
    }

    await pump();
    expect(
      tester
          .widgetList<Text>(find.byType(Text))
          .map((text) => text.data)
          .toList(),
      [
        'Esc',
        '/',
        '-',
        'Home',
        '↑',
        'End',
        'PgUp',
        'Tab',
        'Ctrl',
        'Alt',
        '←',
        '↓',
        '→',
        'PgDn',
      ],
    );

    await tester.tap(find.byKey(const ValueKey('terminal-quick-Ctrl')));
    await tester.pump();
    expect(modifiers.ctrl, isTrue);
    await tester.tap(find.byKey(const ValueKey('terminal-quick-Ctrl')));
    await tester.pump();
    expect(modifiers.isLocked, isFalse);

    await tester.tap(find.byKey(const ValueKey('terminal-quick-Ctrl')));
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('terminal-quick-Alt')));
    await tester.pump();
    expect(modifiers.ctrl, isFalse);
    expect(modifiers.alt, isTrue);

    await tester.tap(find.byKey(const ValueKey('terminal-quick-/')));
    await tester.pump();
    expect(sequences, ['\x1B/']);
    expect(modifiers.isLocked, isFalse);

    await tester.longPress(find.byKey(const ValueKey('terminal-quick-/')));
    await tester.pump();
    expect(sequences.last, '?');

    await tester.longPress(find.byKey(const ValueKey('terminal-quick--')));
    await tester.pump();
    expect(sequences.last, '|');
  });
}
