import Cocoa
import FlutterMacOS

@_silgen_name("sealdesk_quit_from_app_menu")
func sealdesk_quit_from_app_menu()

@main
class AppDelegate: FlutterAppDelegate {
    var launched = false;
  override func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
      dummy_method_to_enforce_bundling()
    // https://github.com/leanflutter/window_manager/issues/214
    return false
  }

  // CM, updater, `quit_gui`, and logout reach here via `NSApp terminate:` and end this process only.
  override func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
      return .terminateNow
  }

  /// MainMenu.xib's Quit item sends `terminate:`. Rebind it so only that action
  /// is a global quit. Cmd+Q uses the same item. The nib is loaded before
  /// `applicationDidFinishLaunching`.
  private func bindApplicationMenuQuit() {
      guard let items = NSApp.mainMenu?.item(at: 0)?.submenu?.items else { return }
      for item in items where item.action == #selector(NSApplication.terminate(_:)) {
          item.target = self
          item.action = #selector(quitFromApplicationMenu(_:))
      }
  }

  @objc func quitFromApplicationMenu(_ sender: Any?) {
      sealdesk_quit_from_app_menu()
  }
    
    override func applicationShouldOpenUntitledFile(_ sender: NSApplication) -> Bool {
        if (launched) {
            handle_applicationShouldOpenUntitledFile();
        }
        return true
    }
    
    override func applicationDidFinishLaunching(_ aNotification: Notification) {
        launched = true;
        bindApplicationMenuQuit()
        NSApplication.shared.activate(ignoringOtherApps: true);
    }
}
