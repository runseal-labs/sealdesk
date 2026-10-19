import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sealdesk/models/terminal_mouse_handler.dart';
import 'package:sealdesk/models/terminal_physical_key.dart';
import 'package:xterm/xterm.dart';

void main() {
  group('terminalPhysicalComboPayload', () {
    test('Ctrl+C is 0x03 and Alt+x is ESC x', () {
      expect(
        _payload(LogicalKeyboardKey.keyC, control: true),
        '\x03',
      );
      expect(
        _payload(LogicalKeyboardKey.keyD, control: true),
        '\x04',
      );
      expect(
        _payload(LogicalKeyboardKey.keyX, alt: true),
        '\x1Bx',
      );
      expect(
        _payload(LogicalKeyboardKey.keyX, alt: true, shift: true),
        '\x1BX',
      );
      expect(
        _payload(LogicalKeyboardKey.keyX, meta: true),
        '\x1Bx',
      );
      expect(
        _payload(
          LogicalKeyboardKey.digit1,
          alt: true,
          shift: true,
          character: '!',
        ),
        '\x1B!',
      );
      expect(
        _payload(
          LogicalKeyboardKey.keyC,
          alt: true,
          capsLock: true,
        ),
        '\x1BC',
      );
      expect(
        _payload(
          LogicalKeyboardKey.keyC,
          control: true,
          character: '\x03',
        ),
        '\x03',
      );
    });

    test('leaves platform paste and select-all shortcuts to xterm', () {
      debugDefaultTargetPlatformOverride = TargetPlatform.linux;
      try {
        expect(
          _payload(LogicalKeyboardKey.keyV, control: true, shift: true),
          isNull,
        );
        expect(
          _payload(LogicalKeyboardKey.keyV, control: true),
          '\x16',
        );
      } finally {
        debugDefaultTargetPlatformOverride = null;
      }

      debugDefaultTargetPlatformOverride = TargetPlatform.windows;
      try {
        expect(_payload(LogicalKeyboardKey.keyV, control: true), isNull);
        expect(_payload(LogicalKeyboardKey.keyA, control: true), isNull);
        expect(_payload(LogicalKeyboardKey.keyC, control: true), '\x03');
      } finally {
        debugDefaultTargetPlatformOverride = null;
      }
    });

    test('does not rewrite an AltGr printable character', () {
      expect(
        _payload(
          LogicalKeyboardKey.keyQ,
          control: true,
          alt: true,
          character: '@',
        ),
        isNull,
      );
    });
  });

  testWidgets('desktop terminal view passes physical chords through',
      (tester) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    try {
      final output = <String>[];
      final terminal =
          Terminal(mouseHandler: const WheelButtonFixMouseHandler())
            ..onOutput = output.add;
      final controller = TerminalController();
      addTearDown(controller.dispose);
      await tester.pumpWidget(MaterialApp(
        home: SizedBox(
          width: 400,
          height: 120,
          child: TerminalMouseInteraction(
            terminal,
            controller: controller,
            onKeyEvent: terminalPhysicalKeyHandler(terminal, controller),
          ),
        ),
      ));
      await tester.tap(find.byType(TerminalView));
      await tester.pump(kDoubleTapTimeout);

      expect(tester.testTextInput.hasAnyClients, isTrue);
      tester.testTextInput.enterText('a中');
      await tester.pump();
      expect(output, ['a中']);
      output.clear();

      tester.testTextInput.updateEditingValue(const TextEditingValue(
        text: 'ni',
        composing: TextRange(start: 0, end: 2),
      ));
      await tester.pump();
      expect(output, isEmpty);
      tester.testTextInput.updateEditingValue(const TextEditingValue(
        text: '你',
        composing: TextRange.empty,
      ));
      await tester.pump();
      expect(output, ['你']);
      output.clear();

      await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
      await tester.sendKeyEvent(LogicalKeyboardKey.keyC);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
      expect(output, ['\x03']);
      output.clear();

      await tester.sendKeyDownEvent(LogicalKeyboardKey.altLeft);
      await tester.sendKeyEvent(LogicalKeyboardKey.keyX);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.altLeft);
      expect(output, ['\x1Bx']);
      output.clear();

      await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
      await tester.sendKeyEvent(LogicalKeyboardKey.keyX);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
      expect(output, ['\x1Bx']);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });
}

String? _payload(
  LogicalKeyboardKey key, {
  bool control = false,
  bool alt = false,
  bool meta = false,
  bool shift = false,
  String? character,
  bool capsLock = false,
}) {
  return terminalPhysicalComboPayload(
    key: key,
    isDown: true,
    control: control,
    alt: alt,
    meta: meta,
    shift: shift,
    character: character,
    capsLock: capsLock,
  );
}
