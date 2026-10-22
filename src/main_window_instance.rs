//! One main window per user session on Linux and macOS.
//!
//! The D-Bus name and the macOS `_url` socket are created only after Flutter
//! installs the event callback. Two launches that both reach that point used
//! to open two windows. A process that will show the main window takes this
//! lock before `StartUi`. A later launch exits only after the running window
//! accepts the activation; otherwise it keeps the lock and shows the window
//! itself. `--install`, `--cm`, `--server`, and `--tray` do not take the lock.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MainWindowClaim {
    Acquired,
    HeldByOther,
    /// The lock could not be opened. Starting the window is safer than exiting.
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MainWindowStartup {
    BecomeMain,
    HandedOff,
}

/// `None` means another live process holds the lock but is not ready yet.
pub(crate) fn decide_main_window_startup(
    delivered: bool,
    claim: MainWindowClaim,
) -> Option<MainWindowStartup> {
    if delivered {
        return Some(MainWindowStartup::HandedOff);
    }
    match claim {
        MainWindowClaim::Acquired | MainWindowClaim::Unavailable => {
            Some(MainWindowStartup::BecomeMain)
        }
        MainWindowClaim::HeldByOther => None,
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[cfg_attr(not(feature = "flutter"), allow(dead_code))]
pub(crate) fn resolve_main_window_activation(
    mut deliver: impl FnMut() -> bool,
) -> MainWindowStartup {
    loop {
        let delivered = deliver();
        let claim = if delivered {
            MainWindowClaim::HeldByOther
        } else {
            try_claim_main_window()
        };
        if let Some(action) = decide_main_window_startup(delivered, claim) {
            return action;
        }
        hbb_common::throttled_log!(
            Duration::from_secs(2),
            info,
            "waiting for the main window to handle this launch"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod lock {
    use hbb_common::{config::APP_NAME, libc, log};
    use std::{
        fs::{File, OpenOptions},
        io::{self, ErrorKind},
        os::unix::{fs::PermissionsExt, io::AsRawFd},
        path::{Path, PathBuf},
        sync::Mutex,
    };

    static HELD: Mutex<Option<File>> = Mutex::new(None);

    #[cfg_attr(not(feature = "flutter"), allow(dead_code))]
    pub(crate) fn try_claim_main_window() -> super::MainWindowClaim {
        try_claim_lock_at(&main_window_lock_path())
    }

    pub(crate) fn try_claim_lock_at(path: &Path) -> super::MainWindowClaim {
        let mut held = HELD.lock().unwrap();
        if held.is_some() {
            return super::MainWindowClaim::Acquired;
        }
        let file = match open_lock_file(path) {
            Ok(file) => file,
            Err(err) => {
                log::warn!("failed to open main-window lock {}: {err}", path.display());
                return super::MainWindowClaim::Unavailable;
            }
        };
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc == 0 {
            *held = Some(file);
            return super::MainWindowClaim::Acquired;
        }
        let err = io::Error::last_os_error();
        if err.kind() == ErrorKind::WouldBlock {
            super::MainWindowClaim::HeldByOther
        } else {
            log::warn!("main-window lock failed: {err}");
            super::MainWindowClaim::Unavailable
        }
    }

    #[cfg_attr(not(feature = "flutter"), allow(dead_code))]
    fn main_window_lock_path() -> PathBuf {
        let uid = unsafe { libc::geteuid() };
        let name = APP_NAME.read().unwrap();
        PathBuf::from(format!("/tmp/{name}-{uid}/main-window.lock"))
    }

    fn open_lock_file(path: &Path) -> io::Result<File> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;
        let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
        Ok(file)
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) use lock::try_claim_main_window;

#[cfg(test)]
mod tests {
    use super::{decide_main_window_startup, MainWindowClaim, MainWindowStartup};

    #[test]
    fn exit_only_when_the_running_window_accepted_the_launch() {
        assert_eq!(
            decide_main_window_startup(true, MainWindowClaim::HeldByOther),
            Some(MainWindowStartup::HandedOff)
        );
        assert_eq!(
            decide_main_window_startup(false, MainWindowClaim::Acquired),
            Some(MainWindowStartup::BecomeMain)
        );
        assert_eq!(
            decide_main_window_startup(false, MainWindowClaim::Unavailable),
            Some(MainWindowStartup::BecomeMain)
        );
        assert_eq!(
            decide_main_window_startup(false, MainWindowClaim::HeldByOther),
            None
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn main_window_lock_is_exclusive_across_processes() {
        let dir = std::env::temp_dir().join(format!("sealdesk-lock-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("main-window.lock");
        assert_eq!(
            super::lock::try_claim_lock_at(&path),
            MainWindowClaim::Acquired
        );
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "main_window_instance::tests::main_window_lock_probe_child",
                "--exact",
            ])
            .env("SEALDESK_LOCK_PROBE", &path)
            .status()
            .unwrap();
        assert!(status.success(), "probe status {status}");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn main_window_lock_probe_child() {
        let Ok(path) = std::env::var("SEALDESK_LOCK_PROBE") else {
            return;
        };
        assert_ne!(
            super::lock::try_claim_lock_at(std::path::Path::new(&path)),
            MainWindowClaim::Acquired,
            "child acquired a lock the parent holds"
        );
    }
}
