const sealDeskTrayHostEnv = 'SEALDESK_TRAY_HOST';

/// Linux SealDesk hides the main window only when a StatusNotifier host was
/// present at startup. Other platforms follow the upstream hide-on-close path.
bool sealDeskHidesMainWindowToTray({
  required bool linux,
  required bool sealDesk,
  required String trayHostEnv,
}) {
  return linux && sealDesk && trayHostEnv == '1';
}

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
