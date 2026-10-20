/// On-screen terminal keys above the phone keyboard.
///
/// Layout and behavior follow the hapi quick-key rows (two equal groups, sticky
/// one-shot Ctrl/Alt, long-press alternates). Sequences and interactions are
/// reimplemented here; the TypeScript sources are not copied.
enum TerminalQuickModifier { ctrl, alt }

class TerminalQuickKeyPopup {
  const TerminalQuickKeyPopup({
    required this.label,
    required this.sequence,
    required this.description,
  });

  final String label;
  final String sequence;
  final String description;
}

class TerminalQuickKeySpec {
  const TerminalQuickKeySpec({
    required this.label,
    required this.description,
    this.sequence,
    this.modifier,
    this.popup,
  });

  final String label;
  final String description;
  final String? sequence;
  final TerminalQuickModifier? modifier;
  final TerminalQuickKeyPopup? popup;

  bool get isModifier => modifier != null;
}

const terminalQuickKeyLongPress = Duration(milliseconds: 500);

const terminalQuickKeyRows = <List<TerminalQuickKeySpec>>[
  [
    TerminalQuickKeySpec(
      label: 'Esc',
      sequence: '\x1B',
      description: 'Escape',
    ),
    TerminalQuickKeySpec(
      label: '/',
      sequence: '/',
      description: 'Forward slash',
      popup: TerminalQuickKeyPopup(
        label: '?',
        sequence: '?',
        description: 'Question mark',
      ),
    ),
    TerminalQuickKeySpec(
      label: '-',
      sequence: '-',
      description: 'Hyphen',
      popup: TerminalQuickKeyPopup(
        label: '|',
        sequence: '|',
        description: 'Pipe',
      ),
    ),
    TerminalQuickKeySpec(
      label: 'Home',
      sequence: '\x1B[H',
      description: 'Terminal home key',
    ),
    TerminalQuickKeySpec(
      label: '↑',
      sequence: '\x1B[A',
      description: 'Arrow up',
    ),
    TerminalQuickKeySpec(
      label: 'End',
      sequence: '\x1B[F',
      description: 'End',
    ),
    TerminalQuickKeySpec(
      label: 'PgUp',
      sequence: '\x1B[5~',
      description: 'Page up',
    ),
  ],
  [
    TerminalQuickKeySpec(
      label: 'Tab',
      sequence: '\t',
      description: 'Tab',
    ),
    TerminalQuickKeySpec(
      label: 'Ctrl',
      description: 'Control',
      modifier: TerminalQuickModifier.ctrl,
    ),
    TerminalQuickKeySpec(
      label: 'Alt',
      description: 'Alternate',
      modifier: TerminalQuickModifier.alt,
    ),
    TerminalQuickKeySpec(
      label: '←',
      sequence: '\x1B[D',
      description: 'Arrow left',
    ),
    TerminalQuickKeySpec(
      label: '↓',
      sequence: '\x1B[B',
      description: 'Arrow down',
    ),
    TerminalQuickKeySpec(
      label: '→',
      sequence: '\x1B[C',
      description: 'Arrow right',
    ),
    TerminalQuickKeySpec(
      label: 'PgDn',
      sequence: '\x1B[6~',
      description: 'Page down',
    ),
  ],
];

List<List<String>> terminalQuickKeyLabels() => [
      for (final row in terminalQuickKeyRows)
        [for (final key in row) key.label],
    ];

/// Sticky Ctrl/Alt for the quick-key row.
///
/// A tap turns one modifier on and the other off. The active modifier stays
/// locked until another tap clears it or the next real sequence consumes it.
class TerminalQuickModifierState {
  const TerminalQuickModifierState({this.ctrl = false, this.alt = false});

  final bool ctrl;
  final bool alt;

  bool get isLocked => ctrl || alt;

  TerminalQuickModifierState toggle(TerminalQuickModifier modifier) {
    switch (modifier) {
      case TerminalQuickModifier.ctrl:
        return TerminalQuickModifierState(ctrl: !ctrl);
      case TerminalQuickModifier.alt:
        return TerminalQuickModifierState(alt: !alt);
    }
  }

  TerminalQuickModifierState cleared() => const TerminalQuickModifierState();
}

/// Alt prefixes ESC. Ctrl rewrites one ASCII character in the @–_ range to a
/// C0 control. Multi-character sequences only receive the Alt prefix.
String applyQuickKeyModifiers(
  String sequence, {
  required bool ctrl,
  required bool alt,
}) {
  var modified = sequence;
  if (alt && modified.isNotEmpty) {
    modified = '\x1B$modified';
  }
  if (ctrl && modified.length == 1) {
    final code = modified.toUpperCase().codeUnitAt(0);
    if (code >= 64 && code <= 95) {
      modified = String.fromCharCode(code - 64);
    }
  }
  return modified;
}

/// One-shot sticky modifiers reset after a real sequence is sent.
bool shouldResetQuickKeyModifiers(
  String sequence, {
  required bool ctrl,
  required bool alt,
}) {
  if (sequence.isEmpty) return false;
  return ctrl || alt;
}

/// Applies sticky modifiers and reports whether they should clear.
({String sequence, bool reset}) dispatchQuickKeySequence(
  String sequence, {
  required TerminalQuickModifierState modifiers,
}) {
  return (
    sequence: applyQuickKeyModifiers(
      sequence,
      ctrl: modifiers.ctrl,
      alt: modifiers.alt,
    ),
    reset: shouldResetQuickKeyModifiers(
      sequence,
      ctrl: modifiers.ctrl,
      alt: modifiers.alt,
    ),
  );
}
