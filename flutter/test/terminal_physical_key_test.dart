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
            hardwareKeyboardOnly: true,
          ),
        ),
      ));
      await tester.tap(find.byType(TerminalView));
      await tester.pump(kDoubleTapTimeout);

      await tester.sendKeyEvent(LogicalKeyboardKey.keyA);
      expect(output, ['a']);
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
}) {
  return terminalPhysicalComboPayload(
    key: key,
    isDown: true,
    control: control,
    alt: alt,
    meta: meta,
    shift: shift,
    character: character,
  );
}
