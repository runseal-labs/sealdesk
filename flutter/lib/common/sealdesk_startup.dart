import 'sealdesk_control_env_stub.dart'
    if (dart.library.io) 'sealdesk_control_env_io.dart';

const sealDeskTrayHostEnv = 'SEALDESK_TRAY_HOST';
const sealDeskControlCodeEnv = 'SEALDESK_CONTROL_CODE';

/// `1` is the only value that means this launch showed a tray icon.
/// Closing the main window then hides it. Any other value quits.
bool sealDeskTrayKeepsProcess(String trayHost) => trayHost == '1';

/// The install page is `kAppTypeMain` but does not handle
/// `on_url_scheme_received`. It must not own the Linux session-bus name or
/// the macOS `_url` socket, or a later plain launch hands off to it and exits
/// without opening a window.
bool sealDeskOwnsMainWindowUriServer(Iterable<String> bootArgs) =>
    !bootArgs.contains('--install');

class SealDeskStartup {
  static String? _target;
  static bool _clearControlCode = false;

  /// [controlCode] is the value already read from the environment. Production
  /// leaves it null so the process environment is read instead.
  static void configure(List<String> arguments, {String? controlCode}) {
    final fromEnv = controlCode ?? readSealDeskControlCode(sealDeskControlCodeEnv);
    if (fromEnv != null && fromEnv.isNotEmpty) {
      _target = fromEnv;
      _clearControlCode = true;
      return;
    }
    _clearControlCode = false;
    if (arguments.length == 2 &&
        arguments.first == '--control' &&
        arguments[1] != '-') {
      _target = arguments[1];
      return;
    }
    _target = null;
  }

  static String? takeTarget() {
    final target = _target;
    _target = null;
    return target;
  }

  static void clearStagedControlCode(void Function(String key) clear) {
    if (!_clearControlCode) return;
    _clearControlCode = false;
    clear(sealDeskControlCodeEnv);
  }
}
