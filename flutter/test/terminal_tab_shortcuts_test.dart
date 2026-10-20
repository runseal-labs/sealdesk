import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sealdesk/models/terminal_tab_shortcuts.dart';

void main() {
  TerminalTabShortcutMatch? match(
    TargetPlatform platform,
    LogicalKeyboardKey key, {
    bool control = false,
    bool alt = false,
    bool meta = false,
    bool shift = false,
  }) {
    return matchTerminalTabShortcut(
      platform: platform,
      key: key,
      isDown: true,
      control: control,
      alt: alt,
      meta: meta,
      shift: shift,
    );
  }

  test('Ctrl+Tab cycles and Alt+arrows stay with the terminal', () {
    for (final platform in TargetPlatform.values) {
      expect(
        match(platform, LogicalKeyboardKey.tab, control: true)?.action,
        TerminalTabShortcutAction.next,
        reason: '$platform',
      );
      expect(
        match(
          platform,
          LogicalKeyboardKey.tab,
          control: true,
          shift: true,
        )?.action,
        TerminalTabShortcutAction.previous,
      );
      expect(
        match(platform, LogicalKeyboardKey.arrowLeft, alt: true),
        isNull,
      );
      expect(
        match(platform, LogicalKeyboardKey.arrowRight, alt: true),
        isNull,
      );
      expect(
        match(
          platform,
          LogicalKeyboardKey.tab,
          control: true,
          alt: true,
        ),
        isNull,
      );
    }
  });

  test('number keys follow Command on Apple and Control elsewhere', () {
    for (final platform in const [
      TargetPlatform.windows,
      TargetPlatform.linux,
      TargetPlatform.android,
      TargetPlatform.fuchsia,
    ]) {
      expect(
        match(platform, LogicalKeyboardKey.digit1, control: true)?.index,
        0,
      );
      expect(
        match(platform, LogicalKeyboardKey.digit9, control: true)?.index,
        8,
      );
      expect(match(platform, LogicalKeyboardKey.digit1, meta: true), isNull);
    }
    for (final platform in const [
      TargetPlatform.macOS,
      TargetPlatform.iOS,
    ]) {
      expect(
        match(platform, LogicalKeyboardKey.digit3, meta: true)?.index,
        2,
      );
      expect(
        match(platform, LogicalKeyboardKey.digit1, control: true),
        isNull,
      );
    }
  });
}
