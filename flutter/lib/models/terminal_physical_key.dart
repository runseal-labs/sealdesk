import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:xterm/xterm.dart';

import 'input_modifier_utils.dart';
import 'terminal_copy_shortcut.dart';
import 'terminal_tab_shortcuts.dart';

/// Encodes a physical Ctrl/Alt/Meta chord into terminal bytes.
///
/// Returns null when the event should stay on xterm's normal path: key-up,
/// unmodified typing, keys xterm already owns (arrows, copy, paste), or an
/// AltGr character. Ctrl+Shift+letter uses the same C0 byte as Ctrl+letter.
/// Ctrl+Alt is encoded unless the layout already produced a different glyph.
String? terminalPhysicalComboPayload({
  required LogicalKeyboardKey key,
  required bool isDown,
  required bool control,
  required bool alt,
  required bool meta,
  required bool shift,
  String? character,
  bool capsLock = false,
  Map<ShortcutActivator, Intent>? shortcuts,
}) {
  if (!isDown) return null;
  if (!control && !alt && !meta) return null;
  if (_isModifierKey(key)) return null;
  // AltGr arrives as Ctrl+Alt plus a glyph the key would not produce alone
  // (for example Ctrl+Alt+Q -> '@'). A real Ctrl+Alt chord has no glyph, or
  // the same letter the key normally produces.
  if (control &&
      alt &&
      !meta &&
      _isAltGrCharacter(
        key: key,
        shift: shift,
        capsLock: capsLock,
        character: character,
      )) {
    return null;
  }
  if (_matchesTerminalShortcut(
    key: key,
    control: control,
    alt: alt,
    meta: meta,
    shift: shift,
    shortcuts: shortcuts,
  )) {
    return null;
  }

  final altLike = alt || meta;
  // Alt/Meta should send the glyph the layout produced (Shift+1 -> '!').
  // Ctrl keeps the logical letter so a control chord is not rewritten from a
  // character the IME already turned into a control byte.
  final produced = _isPrintableCharacter(character) ? character : null;
  final base = altLike
      ? (produced ?? _baseCharacter(key, shift, capsLock: capsLock))
      : _baseCharacter(key, shift, capsLock: capsLock);
  if (base != null) {
    final encoded = applyTerminalInputModifiers(
      base,
      ctrlLocked: control,
      altLocked: altLike,
    );
    // Ctrl on an unmapped key must not forward the plain character.
    if (control && !altLike && encoded == base) {
      return _controlCharacter(character);
    }
    if (encoded != base || altLike) return encoded;
  }
  return _controlCharacter(character);
}

FocusOnKeyEventCallback terminalPhysicalKeyHandler(
  Terminal terminal,
  TerminalController controller, {
  FocusOnKeyEventCallback? fallback,
}) {
  final copy = terminalCopyHandler(
    terminal,
    controller,
    fallback: fallback,
  );
  return (node, event) {
    final copied = copy(node, event);
    if (copied != KeyEventResult.ignored) return copied;
    final keyboard = HardwareKeyboard.instance;
    final payload = terminalPhysicalComboPayload(
      key: event.logicalKey,
      character: event.character,
      isDown: event is KeyDownEvent || event is KeyRepeatEvent,
      control: keyboard.isControlPressed,
      alt: keyboard.isAltPressed,
      meta: keyboard.isMetaPressed,
      shift: keyboard.isShiftPressed,
      capsLock: keyboard.lockModesEnabled.contains(KeyboardLockMode.capsLock),
    );
    if (payload == null) return KeyEventResult.ignored;
    terminal.textInput(payload);
    return KeyEventResult.handled;
  };
}

/// Desktop terminal keys: tab chords stay in the window, everything else uses
/// [terminalPhysicalKeyHandler].
FocusOnKeyEventCallback terminalDesktopKeyHandler(
  Terminal terminal,
  TerminalController controller, {
  FocusOnKeyEventCallback? fallback,
}) {
  final encode = terminalPhysicalKeyHandler(
    terminal,
    controller,
    fallback: fallback,
  );
  return (node, event) {
    final keyboard = HardwareKeyboard.instance;
    final tabShortcut = matchTerminalTabShortcut(
      platform: defaultTargetPlatform,
      key: event.logicalKey,
      isDown: event is KeyDownEvent || event is KeyRepeatEvent,
      control: keyboard.isControlPressed,
      alt: keyboard.isAltPressed,
      meta: keyboard.isMetaPressed,
      shift: keyboard.isShiftPressed,
    );
    if (tabShortcut != null) return KeyEventResult.handled;
    return encode(node, event);
  };
}

bool _isModifierKey(LogicalKeyboardKey key) {
  return key == LogicalKeyboardKey.control ||
      key == LogicalKeyboardKey.controlLeft ||
      key == LogicalKeyboardKey.controlRight ||
      key == LogicalKeyboardKey.alt ||
      key == LogicalKeyboardKey.altLeft ||
      key == LogicalKeyboardKey.altRight ||
      key == LogicalKeyboardKey.meta ||
      key == LogicalKeyboardKey.metaLeft ||
      key == LogicalKeyboardKey.metaRight ||
      key == LogicalKeyboardKey.shift ||
      key == LogicalKeyboardKey.shiftLeft ||
      key == LogicalKeyboardKey.shiftRight;
}

bool _matchesTerminalShortcut({
  required LogicalKeyboardKey key,
  required bool control,
  required bool alt,
  required bool meta,
  required bool shift,
  Map<ShortcutActivator, Intent>? shortcuts,
}) {
  final map = shortcuts ?? platformTerminalShortcuts();
  for (final activator in map.keys) {
    if (activator is! SingleActivator) continue;
    if (activator.trigger == key &&
        activator.control == control &&
        activator.alt == alt &&
        activator.meta == meta &&
        activator.shift == shift) {
      return true;
    }
  }
  return false;
}

String? _baseCharacter(LogicalKeyboardKey key, bool shift,
    {bool capsLock = false}) {
  if (key == LogicalKeyboardKey.space) return ' ';
  final label = key.keyLabel;
  if (label.length != 1) return null;
  final code = label.codeUnitAt(0);
  if (code >= 0x41 && code <= 0x5A) {
    final lower = String.fromCharCode(code + 0x20);
    return shift != capsLock ? label : lower;
  }
  if (code >= 0x20 && code < 0x7F) return label;
  return null;
}

String? _controlCharacter(String? character) {
  if (!_isSingleCodeUnit(character)) return null;
  final code = character!.codeUnitAt(0);
  if (code < 0x20 || code == 0x7F) return character;
  return null;
}

bool _isAltGrCharacter({
  required LogicalKeyboardKey key,
  required bool shift,
  required bool capsLock,
  required String? character,
}) {
  if (!_isPrintableCharacter(character)) return false;
  final own = _baseCharacter(key, shift, capsLock: capsLock);
  if (own == null) return true;
  return character!.toLowerCase() != own.toLowerCase();
}

bool _isPrintableCharacter(String? character) {
  if (!_isSingleCodeUnit(character)) return false;
  final code = character!.codeUnitAt(0);
  return code >= 0x20 && code != 0x7F;
}

bool _isSingleCodeUnit(String? character) =>
    character != null && character.length == 1;
