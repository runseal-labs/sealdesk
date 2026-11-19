import Cocoa
import FlutterMacOS

@_silgen_name("sealdesk_quit_from_app_menu")
func sealdesk_quit_from_app_menu()

@_silgen_name("sealdesk_macos_terminate_kills_siblings")
func sealdesk_macos_terminate_kills_siblings(_ fromAppMenu: Int32) -> Int32

@main
class AppDelegate: FlutterAppDelegate {
    var launched = false;
    /// Set by the host method channel before `NSApplication.terminate`.
    /// CM close and the no-tray main-window fallback set this. It is not a menu Quit.
    static var terminateFromPlatformChannel = false
    /// Set only by the application-menu Quit item (Cmd+Q is that item's key equivalent).
    /// `quit_gui` / `NSApp terminate:` and session logout leave this false.
    static var terminateFromAppMenu = false
  override func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
      dummy_method_to_enforce_bundling()
    // https://github.com/leanflutter/window_manager/issues/214
    return false
  }

  // Global Quit is the application-menu action only. The red button still hides:
  // prevent-close stays set and this is not a last-window terminate. Tray Quit
  // calls `quit_app` itself. Every other `NSApp terminate:` (updater, relaunch,
  // CM fallback, logout) must not kill the process just started.
  override func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
      let fromMenu: Int32 =
          (AppDelegate.terminateFromAppMenu && !AppDelegate.terminateFromPlatformChannel) ? 1 : 0
      if sealdesk_macos_terminate_kills_siblings(fromMenu) != 0 {
          sealdesk_quit_from_app_menu()
      }
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
      AppDelegate.terminateFromAppMenu = true
      NSApp.terminate(sender)
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
