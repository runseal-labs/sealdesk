import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sealdesk/models/rustdesk_terminal.dart';
import 'package:sealdesk/models/terminal_copy_shortcut.dart';
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
          _payload(LogicalKeyboardKey.keyC, control: true, shift: true),
          isNull,
        );
        expect(
          _payload(LogicalKeyboardKey.keyV, control: true),
          '\x16',
        );
        expect(
          _payload(LogicalKeyboardKey.keyA, control: true),
          '\x01',
        );
      } finally {
        debugDefaultTargetPlatformOverride = null;
      }

      debugDefaultTargetPlatformOverride = TargetPlatform.windows;
      try {
        expect(_payload(LogicalKeyboardKey.keyV, control: true), '\x16');
        expect(_payload(LogicalKeyboardKey.keyA, control: true), '\x01');
        expect(_payload(LogicalKeyboardKey.keyC, control: true), '\x03');
        expect(
          _payload(LogicalKeyboardKey.keyV, control: true, shift: true),
          isNull,
        );
      } finally {
        debugDefaultTargetPlatformOverride = null;
      }

      debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
      try {
        expect(_payload(LogicalKeyboardKey.keyC, meta: true), isNull);
        expect(_payload(LogicalKeyboardKey.keyV, meta: true), isNull);
        expect(_payload(LogicalKeyboardKey.keyA, meta: true), isNull);
        // Plain Ctrl+letter is left to xterm's own Ctrl handler on macOS.
        expect(_payload(LogicalKeyboardKey.keyC, control: true), isNull);
        expect(
          _payload(LogicalKeyboardKey.keyC, control: true, shift: true),
          '\x03',
        );
      } finally {
        debugDefaultTargetPlatformOverride = null;
      }
    });

    test('encodes Ctrl+Shift letters and non-AltGr Ctrl+Alt', () {
      expect(
        _payload(LogicalKeyboardKey.keyA, control: true, shift: true),
        '\x01',
      );
      expect(
        _payload(LogicalKeyboardKey.keyR, control: true, shift: true),
        '\x12',
      );
      expect(
        _payload(LogicalKeyboardKey.keyZ, control: true, shift: true),
        '\x1A',
      );
      expect(
        _payload(LogicalKeyboardKey.keyX, control: true, alt: true),
        '\x1B\x18',
      );
      expect(
        _payload(
          LogicalKeyboardKey.keyC,
          control: true,
          alt: true,
          character: 'c',
        ),
        '\x1B\x03',
      );
    });

    for (final platform in const [
      TargetPlatform.windows,
      TargetPlatform.macOS,
      TargetPlatform.linux,
      TargetPlatform.android,
      TargetPlatform.iOS,
    ]) {
      test('encodes letter chords on $platform', () {
        debugDefaultTargetPlatformOverride = platform;
        try {
          // macOS leaves plain Ctrl+letter to xterm; the view test below
          // checks those still reach the PTY as C0 bytes.
          String? plain(String byte) =>
              platform == TargetPlatform.macOS ? null : byte;
          for (final (key, byte) in const [
            (LogicalKeyboardKey.keyC, '\x03'),
            (LogicalKeyboardKey.keyD, '\x04'),
            (LogicalKeyboardKey.keyA, '\x01'),
            (LogicalKeyboardKey.keyR, '\x12'),
            (LogicalKeyboardKey.keyZ, '\x1A'),
          ]) {
            expect(_payload(key, control: true), plain(byte));
          }
          expect(
            _payload(LogicalKeyboardKey.keyB, control: true, shift: true),
            '\x02',
          );
          expect(_payload(LogicalKeyboardKey.keyX, alt: true), '\x1Bx');
          expect(_payload(LogicalKeyboardKey.keyX, meta: true), '\x1Bx');
          final apple = platform == TargetPlatform.macOS ||
              platform == TargetPlatform.iOS;
          expect(
            _payload(LogicalKeyboardKey.keyC, control: true, shift: true),
            apple ? '\x03' : isNull,
          );
          expect(
            isTerminalCopyChord(
              platform: platform,
              key: LogicalKeyboardKey.keyC,
              control: !apple,
              meta: apple,
              alt: false,
              shift: !apple,
            ),
            isTrue,
          );
          expect(
            isTerminalPasteChord(
              platform: platform,
              key: LogicalKeyboardKey.keyV,
              control: true,
              meta: false,
              alt: false,
              shift: false,
            ),
            isFalse,
          );
          expect(
            isTerminalSelectAllChord(
              platform: platform,
              key: LogicalKeyboardKey.keyA,
              control: true,
              meta: false,
              alt: false,
              shift: false,
            ),
            isFalse,
          );
        } finally {
          debugDefaultTargetPlatformOverride = null;
        }
      });
    }

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

  testWidgets('macOS plain Ctrl+C goes through xterm and yields to the IME',
      (tester) async {
    final output = <String>[];
    final terminal = Terminal(platform: TerminalTargetPlatform.macos)
      ..onOutput = output.add;
    final controller = TerminalController();
    final focus = FocusNode();
    KeyEventResult handle(KeyEvent event) =>
        terminalFocusedDesktopKeyEvent(terminal, controller, focus, event);
    FocusManager.instance.addEarlyKeyEventHandler(handle);
    addTearDown(() {
      FocusManager.instance.removeEarlyKeyEventHandler(handle);
      focus.dispose();
      controller.dispose();
    });
    await tester.pumpWidget(MaterialApp(
      home: TerminalMouseInteraction(terminal,
          controller: controller,
          focusNode: focus,
          autofocus: true,
          onKeyEvent: terminalDesktopKeyHandler(terminal, controller)),
    ));
    await tester.pump();
    expect(focus.hasPrimaryFocus, isTrue);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyC);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    expect(output, ['\x03']);
    output.clear();
    // While the IME has marked text, the chord belongs to the IME, as in
    // Terminal.app and iTerm2.
    tester.testTextInput.updateEditingValue(const TextEditingValue(
      text: 'c',
      composing: TextRange(start: 0, end: 1),
    ));
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyC);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    expect(output, isEmpty);
    focus.unfocus();
    await tester.pump();
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyC);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    expect(output, isEmpty);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

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

  for (final platform in const [
    TargetPlatform.windows,
    TargetPlatform.macOS,
    TargetPlatform.linux,
    TargetPlatform.android,
    TargetPlatform.iOS,
  ]) {
    testWidgets('view encodes Esc, arrows, and chords on $platform',
        (tester) async {
      debugDefaultTargetPlatformOverride = platform;
      try {
        final output = <String>[];
        final terminal = Terminal(
          platform: sealDeskTerminalPlatform(platform),
          mouseHandler: const WheelButtonFixMouseHandler(),
        )..onOutput = output.add;
        final controller = TerminalController();
        addTearDown(controller.dispose);
        await tester.pumpWidget(MaterialApp(
          home: SizedBox(
            width: 400,
            height: 160,
            child: TerminalMouseInteraction(
              terminal,
              controller: controller,
              autofocus: true,
              shortcuts: platformTerminalShortcuts(platform),
              onKeyEvent: platform == TargetPlatform.android ||
                      platform == TargetPlatform.iOS
                  ? terminalPhysicalKeyHandler(terminal, controller)
                  : terminalDesktopKeyHandler(terminal, controller),
            ),
          ),
        ));
        await tester.tap(find.byType(TerminalView));
        await tester.pump(kDoubleTapTimeout);

        await tester.sendKeyEvent(LogicalKeyboardKey.escape);
        await tester.pump();
        expect(output, ['\x1B'], reason: '$platform escape');
        output.clear();

        await tester.sendKeyEvent(LogicalKeyboardKey.arrowUp);
        await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
        await tester.sendKeyEvent(LogicalKeyboardKey.arrowLeft);
        await tester.sendKeyEvent(LogicalKeyboardKey.arrowRight);
        await tester.pump();
        expect(output, ['\x1B[A', '\x1B[B', '\x1B[D', '\x1B[C']);
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

        await tester.sendKeyDownEvent(LogicalKeyboardKey.altLeft);
        await tester.sendKeyEvent(LogicalKeyboardKey.arrowLeft);
        await tester.sendKeyUpEvent(LogicalKeyboardKey.altLeft);
        final altLeft =
            platform == TargetPlatform.macOS ? '\x1Bb' : '\x1B[1;5D';
        expect(output, [altLeft], reason: '$platform alt+left');
      } finally {
        debugDefaultTargetPlatformOverride = null;
      }
    });
  }

  test('host platform is stored on the terminal', () {
    for (final platform in const [
      TargetPlatform.windows,
      TargetPlatform.macOS,
      TargetPlatform.linux,
      TargetPlatform.android,
      TargetPlatform.iOS,
    ]) {
      final terminal = SealDeskTerminal(
        platform: sealDeskTerminalPlatform(platform),
        clipboardWritePermission: () => TerminalClipboardWritePermission.denied,
        onClipboardWrite: (_) async => false,
      );
      expect(terminal.platform, sealDeskTerminalPlatform(platform));
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
