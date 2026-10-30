use std::path::{Path, PathBuf};

/// Set in the UI process before Flutter starts. `1` means × hides to the tray.
pub const TRAY_HOST_ENV: &str = "SEALDESK_TRAY_HOST";

/// Missing and every value other than `Y` stay off.
pub fn enabled(value: &str) -> bool {
    value == "Y"
}

/// Flutter treats `SEALDESK_TRAY_HOST=1` as "× hides". That is only true when a tray
/// icon can actually appear. `hide-tray=Y` makes `start_tray` return without one.
pub fn advertise_tray_host(platform_present: bool, hide_tray: &str) -> bool {
    platform_present && hide_tray != "Y"
}

/// Watcher, loadable indicator library, and builtin `hide-tray` not `Y`.
/// A missing library must not be advertised: closing the window then quits.
pub fn tray_host_usable(watcher: bool, library: bool, hide_tray: &str) -> bool {
    watcher && library && hide_tray != "Y"
}

/// A missing shortcut is already off. Any other delete error is returned so the
/// caller does not report success.
pub(crate) fn ignore_not_found(result: std::io::Result<()>) -> std::io::Result<()> {
    match result {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

pub fn linux_autostart_path(home: &Path, app_name: &str) -> PathBuf {
    home.join(".config")
        .join("autostart")
        .join(format!("{}.desktop", app_name.to_lowercase()))
}

/// `APPIMAGE` is the real file. `current_exe()` inside an AppImage is the mount path.
pub fn linux_login_program<'a>(appimage: Option<&'a str>, current_exe: &'a str) -> &'a str {
    match appimage {
        Some(path) if !path.is_empty() => path,
        _ => current_exe,
    }
}

pub fn linux_autostart_desktop(app_name: &str, exe: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName={app_name}\nExec={exec}\nNoDisplay=false\nX-GNOME-Autostart-enabled=true\n",
        exec = desktop_exec(exe),
    )
}

pub fn macos_login_label(app_name: &str) -> String {
    let mut label = String::from("com.runseal.");
    for c in app_name.chars() {
        if c.is_ascii_alphanumeric() {
            label.push(c.to_ascii_lowercase());
        }
    }
    label.push_str(".login");
    label
}

/// User LaunchAgent. Upstream's system agent keeps the process alive; this one
/// does not, so Quit is not immediately started again.
pub fn macos_login_plist(label: &str, exe: &str) -> String {
    let work = Path::new(exe)
        .parent()
        .map(|path| path.to_string_lossy().into_owned())
        .filter(|path| !path.is_empty())
        .unwrap_or_else(|| "/".to_owned());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>LimitLoadToSessionType</key>
    <array>
        <string>LoginWindow</string>
        <string>Aqua</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
        <string>--server</string>
    </array>
    <key>WorkingDirectory</key>
    <string>{work}</string>
</dict>
</plist>
"#,
        label = xml_escape(label),
        exe = xml_escape(exe),
        work = xml_escape(&work),
    )
}

fn desktop_exec(exe: &str) -> String {
    if exe.contains([' ', '\t', '\n', '"']) {
        format!(
            "\"{}\" --server",
            exe.replace('\\', "\\\\").replace('"', "\\\"")
        )
    } else {
        format!("{exe} --server")
    }
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_at_login_defaults_off() {
        assert!(!enabled(""));
        assert!(!enabled("N"));
        assert!(enabled("Y"));
    }

    #[test]
    fn hide_tray_is_not_a_tray_host() {
        assert!(advertise_tray_host(true, ""));
        assert!(advertise_tray_host(true, "N"));
        assert!(!advertise_tray_host(true, "Y"));
        assert!(!advertise_tray_host(false, ""));
        assert!(!advertise_tray_host(false, "Y"));
    }

    #[test]
    fn tray_host_needs_watcher_library_and_visible_tray() {
        assert!(tray_host_usable(true, true, ""));
        assert!(tray_host_usable(true, true, "N"));
        assert!(!tray_host_usable(true, true, "Y"));
        assert!(!tray_host_usable(true, false, ""));
        assert!(!tray_host_usable(false, true, ""));
        assert!(!tray_host_usable(false, false, ""));
        assert!(!tray_host_usable(false, false, "Y"));
        assert!(!tray_host_usable(true, false, "Y"));
    }

    #[test]
    fn missing_startup_shortcut_is_not_a_delete_error() {
        assert!(ignore_not_found(Ok(())).is_ok());
        assert!(ignore_not_found(Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "gone"
        )))
        .is_ok());
        assert!(ignore_not_found(Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "denied"
        )))
        .is_err());
    }

    #[test]
    fn linux_autostart_file_starts_the_server_in_the_user_config_dir() {
        let path = linux_autostart_path(Path::new("/home/user"), "SealDesk");
        assert_eq!(
            path,
            PathBuf::from("/home/user/.config/autostart/sealdesk.desktop")
        );
        let desktop = linux_autostart_desktop("SealDesk", "/usr/bin/sealdesk");
        assert!(desktop.contains("Exec=/usr/bin/sealdesk --server\n"));
        assert!(!desktop.contains("--tray"));
        let quoted = linux_autostart_desktop("SealDesk", "/opt/Seal Desk/sealdesk");
        assert!(quoted.contains("Exec=\"/opt/Seal Desk/sealdesk\" --server\n"));
    }

    #[test]
    fn appimage_login_uses_the_appimage_path_not_the_mount() {
        assert_eq!(
            linux_login_program(
                Some("/home/user/SealDesk.AppImage"),
                "/tmp/.mount_sealdesk/usr/bin/sealdesk"
            ),
            "/home/user/SealDesk.AppImage"
        );
        assert_eq!(
            linux_login_program(None, "/usr/bin/sealdesk"),
            "/usr/bin/sealdesk"
        );
        assert_eq!(
            linux_login_program(Some(""), "/usr/bin/sealdesk"),
            "/usr/bin/sealdesk"
        );
    }

    #[test]
    fn macos_login_agent_uses_server_and_does_not_keep_alive() {
        let label = macos_login_label("SealDesk");
        assert_eq!(label, "com.runseal.sealdesk.login");
        let plist = macos_login_plist(&label, "/Applications/SealDesk.app/Contents/MacOS/SealDesk");
        assert!(plist.contains("<string>--server</string>"));
        assert!(plist.contains("<string>/Applications/SealDesk.app/Contents/MacOS</string>"));
        assert!(!plist.contains("KeepAlive"));
        assert!(macos_login_plist("a&b", "/tmp/a<b").contains("&amp;"));
        assert!(macos_login_plist("a&b", "/tmp/a<b").contains("&lt;"));
    }
}
