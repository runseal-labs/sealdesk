const sealDeskTrayHostEnv = 'SEALDESK_TRAY_HOST';

/// `1` is the only value that means this launch showed a tray icon.
/// Closing the main window then hides it. Any other value quits.
bool sealDeskTrayKeepsProcess(String trayHost) => trayHost == '1';

class SealDeskStartup {
  static String? _target;

  static void configure(List<String> arguments) {
    _target = arguments.length == 2 && arguments.first == '--control'
        ? arguments[1]
        : null;
  }

  static String? takeTarget() {
    final target = _target;
    _target = null;
    return target;
  }
}
