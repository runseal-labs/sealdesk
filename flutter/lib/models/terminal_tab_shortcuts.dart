import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// Desktop terminal tab chords.
///
/// Ctrl+Tab / Ctrl+Shift+Tab cycle tabs. Cmd+1–9 on Apple platforms and
/// Ctrl+1–9 elsewhere jump to a tab. Alt+arrows are not tab chords.
enum TerminalTabShortcutAction { next, previous, select }

class TerminalTabShortcutMatch {
  const TerminalTabShortcutMatch._(this.action, [this.index]);

  const TerminalTabShortcutMatch.next()
      : this._(TerminalTabShortcutAction.next);

  const TerminalTabShortcutMatch.previous()
      : this._(TerminalTabShortcutAction.previous);

  const TerminalTabShortcutMatch.select(int index)
      : this._(TerminalTabShortcutAction.select, index);

  final TerminalTabShortcutAction action;
  final int? index;
}

const _tabNumberKeys = <LogicalKeyboardKey>[
  LogicalKeyboardKey.digit1,
  LogicalKeyboardKey.digit2,
  LogicalKeyboardKey.digit3,
  LogicalKeyboardKey.digit4,
  LogicalKeyboardKey.digit5,
  LogicalKeyboardKey.digit6,
  LogicalKeyboardKey.digit7,
  LogicalKeyboardKey.digit8,
  LogicalKeyboardKey.digit9,
];

bool terminalTabShortcutsUseMeta(TargetPlatform platform) {
  return platform == TargetPlatform.macOS || platform == TargetPlatform.iOS;
}

/// Returns the tab action for this chord, or null when the terminal should
/// receive the key (including every Alt+arrow chord).
TerminalTabShortcutMatch? matchTerminalTabShortcut({
  required TargetPlatform platform,
  required LogicalKeyboardKey key,
  required bool isDown,
  required bool control,
  required bool alt,
  required bool meta,
  required bool shift,
}) {
  if (!isDown || alt) return null;
  if (key == LogicalKeyboardKey.tab && control && !meta) {
    return shift
        ? const TerminalTabShortcutMatch.previous()
        : const TerminalTabShortcutMatch.next();
  }
  final numberIndex = _tabNumberKeys.indexOf(key);
  if (numberIndex < 0) return null;
  final usesMeta = terminalTabShortcutsUseMeta(platform);
  final numberModifier = usesMeta ? meta : control;
  if (!numberModifier) return null;
  return TerminalTabShortcutMatch.select(numberIndex);
}
