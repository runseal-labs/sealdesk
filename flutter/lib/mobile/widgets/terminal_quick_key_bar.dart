import 'package:flutter/material.dart';
import 'package:sealdesk/mobile/terminal_keyboard_utils.dart';

/// Two rows of terminal keys shown above the soft keyboard.
class TerminalQuickKeyBar extends StatelessWidget {
  const TerminalQuickKeyBar({
    super.key,
    required this.ctrlActive,
    required this.altActive,
    required this.onToggleModifier,
    required this.onSequence,
    this.localize,
    this.enabled = true,
  });

  final bool ctrlActive;
  final bool altActive;
  final ValueChanged<TerminalQuickModifier> onToggleModifier;
  final ValueChanged<String> onSequence;
  final String Function(String key)? localize;
  final bool enabled;

  String _text(String key) => localize?.call(key) ?? key;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Material(
      color: Theme.of(context).scaffoldBackgroundColor,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 2),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            for (var rowIndex = 0;
                rowIndex < terminalQuickKeyRows.length;
                rowIndex++) ...[
              if (rowIndex > 0) const SizedBox(height: 2),
              Row(
                children: [
                  for (final key in terminalQuickKeyRows[rowIndex])
                    Expanded(
                      child: Padding(
                        padding: const EdgeInsets.symmetric(horizontal: 1),
                        child: _QuickKeyButton(
                          spec: key,
                          enabled: enabled,
                          active: (key.modifier == TerminalQuickModifier.ctrl &&
                                  ctrlActive) ||
                              (key.modifier == TerminalQuickModifier.alt &&
                                  altActive),
                          tooltip: _tooltip(key),
                          semanticsLabel: _text(key.description),
                          idleBackground: scheme.surfaceContainerHighest,
                          idleForeground: scheme.onSurfaceVariant,
                          onTap: () => _activate(key, longPress: false),
                          onLongPress: key.popup == null || key.isModifier
                              ? null
                              : () => _activate(key, longPress: true),
                        ),
                      ),
                    ),
                ],
              ),
            ],
          ],
        ),
      ),
    );
  }

  String _tooltip(TerminalQuickKeySpec key) {
    final base = _text(key.description);
    final popup = key.popup;
    if (popup == null) return base;
    return '$base (${_text('Long press')}: ${_text(popup.description)})';
  }

  void _activate(TerminalQuickKeySpec key, {required bool longPress}) {
    if (!enabled) return;
    final modifier = key.modifier;
    if (modifier != null) {
      if (!longPress) onToggleModifier(modifier);
      return;
    }
    final sequence =
        longPress ? (key.popup?.sequence ?? key.sequence) : key.sequence;
    if (sequence == null || sequence.isEmpty) return;
    onSequence(sequence);
  }
}

class _QuickKeyButton extends StatelessWidget {
  const _QuickKeyButton({
    required this.spec,
    required this.enabled,
    required this.active,
    required this.tooltip,
    required this.semanticsLabel,
    required this.idleBackground,
    required this.idleForeground,
    required this.onTap,
    required this.onLongPress,
  });

  final TerminalQuickKeySpec spec;
  final bool enabled;
  final bool active;
  final String tooltip;
  final String semanticsLabel;
  final Color idleBackground;
  final Color idleForeground;
  final VoidCallback onTap;
  final VoidCallback? onLongPress;

  @override
  Widget build(BuildContext context) {
    final fontFamily = Theme.of(context).textTheme.bodyMedium?.fontFamily;
    final labelStyle = TextStyle(fontSize: 12, fontFamily: fontFamily);
    return Semantics(
      button: true,
      label: semanticsLabel,
      toggled: spec.isModifier ? active : null,
      hint: spec.popup == null ? null : tooltip,
      child: Tooltip(
        message: tooltip,
        child: ElevatedButton(
          key: ValueKey<String>('terminal-quick-${spec.label}'),
          onPressed: enabled ? onTap : null,
          onLongPress: enabled ? onLongPress : null,
          style: ElevatedButton.styleFrom(
            minimumSize: const Size(0, 36),
            padding: EdgeInsets.zero,
            tapTargetSize: MaterialTapTargetSize.shrinkWrap,
            visualDensity: VisualDensity.compact,
            textStyle: labelStyle,
            backgroundColor: active ? Colors.blue : idleBackground,
            foregroundColor: active ? Colors.white : idleForeground,
            disabledBackgroundColor: idleBackground,
            disabledForegroundColor: idleForeground.withOpacity(0.5),
          ),
          child: Text(
            spec.label,
            maxLines: 1,
            softWrap: false,
            style: labelStyle,
          ),
        ),
      ),
    );
  }
}
