/// True when something is already accepting connections on this IPC socket.
/// A stale socket file is not alive: connect fails with `ConnectionRefused`.
pub fn ipc_socket_is_listening(path: &str) -> bool {
    use std::os::unix::net::UnixStream;
    UnixStream::connect(path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::ipc_socket_is_listening;

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
}
