/// True when something is already accepting connections on this IPC socket.
/// A stale socket file is not alive: connect fails with `ConnectionRefused`.
///
/// `connect` blocks once the accept queue is full, so the probe gives up after
/// the same 1s budget `check_pid` uses. A timeout is not proof of a listener.
pub fn ipc_socket_is_listening(path: &str) -> bool {
    let path = path.to_owned();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let ok = std::os::unix::net::UnixStream::connect(&path).is_ok();
        let _ = tx.send(ok);
    });
    rx.recv_timeout(std::time::Duration::from_secs(1))
        .unwrap_or(false)
}

/// Lost the main IPC bind to a live listener. Never `process::exit` from that
/// path: macOS `--server` is hosting the tray on the main thread (LaunchAgent
/// treats exit 0 as a normal stop and will not restart it), and a UI process
/// that called `start_server(true)` is the GUI. Linux `--server` still ends,
/// because `start_server` returns to `core_main`.
pub fn should_exit_process_after_lost_ipc_bind(_is_server_process: bool) -> bool {
    false
}

/// `--server` must not replace a live IPC listener: binding fails closed and
/// `start_server` then sends `Data::Close`, which quits the UI. Linux also
/// `pkill`s `--tray` before the bind. Returns true when the caller should exit.
/// On macOS the GUI skips `--tray` while this `--server` exists, so run the
/// tray here unless another same-user `--server` or `--tray` already has it
/// (an earlier `--server` runs `start_tray` on its own main thread).
pub fn server_defers_to_live_ipc() -> bool {
    if !ipc_socket_is_listening(&hbb_common::config::Config::ipc_path("")) {
        return false;
    }
    hbb_common::log::info!("IPC listener already running; not replacing it");
    #[cfg(target_os = "macos")]
    if !crate::check_process("--server", true) && !crate::check_process("--tray", true) {
        crate::tray::start_tray();
    }
    true
}

#[cfg(test)]
mod tests {
    use super::{ipc_socket_is_listening, should_exit_process_after_lost_ipc_bind};

    #[test]
    fn live_socket_is_listening_and_a_stale_file_is_not() {
        let dir = std::env::temp_dir().join(format!(
            "sealdesk-ipc-listen-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ipc");
        let path = path.to_str().unwrap();
        assert!(!ipc_socket_is_listening(path));
        let listener = std::os::unix::net::UnixListener::bind(path).unwrap();
        assert!(ipc_socket_is_listening(path));
        drop(listener);
        assert!(
            !ipc_socket_is_listening(path),
            "a leftover socket file is not a listener"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lost_ipc_bind_does_not_exit_the_process() {
        assert!(!should_exit_process_after_lost_ipc_bind(false));
        assert!(!should_exit_process_after_lost_ipc_bind(true));
    }
}
