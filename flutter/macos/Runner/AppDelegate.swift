import Cocoa
import FlutterMacOS

@_silgen_name("sealdesk_quit_from_app_menu")
func sealdesk_quit_from_app_menu()

@_silgen_name("sealdesk_macos_quit_globally")
func sealdesk_macos_quit_globally(_ fromPlatformChannel: Int32) -> Int32

@main
class AppDelegate: FlutterAppDelegate {
    var launched = false;
    /// Set by the host method channel before `NSApplication.terminate`.
    /// Cmd+Q leaves this false and is the only AppKit path that quits globally.
    static var terminateFromPlatformChannel = false
  override func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
      dummy_method_to_enforce_bundling()
    // https://github.com/leanflutter/window_manager/issues/214
    return false
  }

  // Cmd+Q / application-menu Quit. The red button still hides: prevent-close
  // stays set and this is not a last-window terminate. Tray Quit kills sibling
  // processes and exits; do that before AppKit asks the window to close.
  // CM close uses the platform channel and must not reach `quit_app`.
  override func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
      let fromChannel: Int32 = AppDelegate.terminateFromPlatformChannel ? 1 : 0
      if sealdesk_macos_quit_globally(fromChannel) != 0 {
          sealdesk_quit_from_app_menu()
      }
      return .terminateNow
  }
    
    override func applicationShouldOpenUntitledFile(_ sender: NSApplication) -> Bool {
        if (launched) {
            handle_applicationShouldOpenUntitledFile();
        }
        return true
    }
    
    override func applicationDidFinishLaunching(_ aNotification: Notification) {
        launched = true;
        NSApplication.shared.activate(ignoringOtherApps: true);
    }
}
