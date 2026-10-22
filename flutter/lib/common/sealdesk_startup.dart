const sealDeskTrayHostEnv = 'SEALDESK_TRAY_HOST';

/// `1` is the only value that means this launch showed a tray icon.
/// Closing the main window then hides it. Any other value quits.
bool sealDeskTrayKeepsProcess(String trayHost) => trayHost == '1';

/// The install page is `kAppTypeMain` but does not handle
/// `on_url_scheme_received`. It must not own the Linux session-bus name or
/// the macOS `_url` socket, or a later plain launch hands off to it and exits
/// without opening a window.
bool sealDeskOwnsMainWindowUriServer(Iterable<String> bootArgs) =>
    !bootArgs.contains('--install');

/// A handoff asked to show the main window. A URI cold start hides that
/// window after startup; the hide must not win if the show already arrived.
bool sealDeskMainWindowShowRequested = false;

void sealDeskRequestMainWindowShow() {
  sealDeskMainWindowShowRequested = true;
}

/// Keep the main window hidden only for a connect/URI launch that has not
/// been asked to show itself.
bool sealDeskHideMainWindowOnStartup({
  required bool handledByUniLinks,
  required bool handledByArgs,
}) =>
    (handledByUniLinks || handledByArgs) && !sealDeskMainWindowShowRequested;

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
