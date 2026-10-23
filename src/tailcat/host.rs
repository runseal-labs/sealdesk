use base::config::keys;
use hbb_common::config::{option2bool, Config};
use std::sync::Mutex;

static RUNNING: Mutex<bool> = Mutex::new(false);
static START_ERROR: Mutex<String> = Mutex::new(String::new());

pub(crate) fn start_error() -> String {
    START_ERROR.lock().unwrap().clone()
}

pub(crate) fn set_start_error(error: String) {
    let scrubbed = scrub_init_error(&error);
    if !scrubbed.is_empty() {
        hbb_common::log::error!("Tailcat sharing failed: {scrubbed}");
    }
    *START_ERROR.lock().unwrap() = scrubbed;
}

pub(crate) fn enabled() -> bool {
    Config::get_option(keys::OPTION_SEALDESK_HOST_MODE) == "host"
        && !option2bool(
            keys::OPTION_STOP_SERVICE,
            &Config::get_option(keys::OPTION_STOP_SERVICE),
        )
}

pub(crate) fn set_running(running: bool) {
    *RUNNING.lock().unwrap() = running;
}

pub(crate) fn set_address(address: String) {
    let running = RUNNING.lock().unwrap();
    let address = if *running && enabled() {
        address
    } else {
        String::new()
    };
    Config::set_option(super::OPTION_ADDR.to_owned(), address);
}

pub(crate) fn begin_start() -> bool {
    let mut running = RUNNING.lock().unwrap();
    if !enabled() {
        return false;
    }
    set_start_error(String::new());
    *running = true;
    true
}

pub(crate) fn set_mode(mode: String) {
    let _running = RUNNING.lock().unwrap();
    if mode == "control" {
        set_start_error(String::new());
    }
    if mode == "host" {
        Config::set_option(keys::OPTION_STOP_SERVICE.to_owned(), String::new());
    }
    Config::set_option(keys::OPTION_SEALDESK_HOST_MODE.to_owned(), mode);
}

fn preserve_host_state(options: &mut std::collections::HashMap<String, String>) {
    for key in [keys::OPTION_SEALDESK_HOST_MODE, super::OPTION_ADDR] {
        let value = Config::get_option(key);
        if value.is_empty() {
            options.remove(key);
        } else {
            options.insert(key.to_owned(), value);
        }
    }
}

pub(crate) fn set_options(mut options: std::collections::HashMap<String, String>) {
    let _running = RUNNING.lock().unwrap();
    let compat_was = crate::lan::rustdesk_compat_enabled();
    preserve_host_state(&mut options);
    Config::set_options(options);
    crate::lan::note_rustdesk_compat(compat_was);
}

pub(crate) fn sync_config(mut config: hbb_common::config::Config2) -> bool {
    let _running = RUNNING.lock().unwrap();
    let compat_was = crate::lan::rustdesk_compat_enabled();
    preserve_host_state(&mut config.options);
    let updated = hbb_common::config::Config2::set(config);
    crate::lan::note_rustdesk_compat(compat_was);
    updated
}

pub(crate) fn mode() -> &'static str {
    let running = RUNNING.lock().unwrap();
    if Config::get_option(keys::OPTION_SEALDESK_HOST_MODE) == "control" {
        if *running {
            "stopping"
        } else {
            "control"
        }
    } else {
        "host"
    }
}

pub(crate) fn reset_key() -> hbb_common::ResultType<()> {
    let running = RUNNING.lock().unwrap();
    if *running || enabled() {
        hbb_common::bail!("被控服务尚未停止，无法重置地址，请稍后重试");
    }
    match std::fs::remove_file(super::server_key_path()) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Config::set_option(super::OPTION_ADDR.to_owned(), String::new());
    Ok(())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) fn reset_address() -> hbb_common::ResultType<()> {
    let mode = crate::ipc::get_config(keys::OPTION_SEALDESK_HOST_MODE)?;
    if !matches!(mode.as_deref(), Some("host" | "control")) {
        hbb_common::bail!("无法确认被控服务状态，请稍后重试");
    }
    let stopped = crate::ipc::get_config(keys::OPTION_STOP_SERVICE)?
        .ok_or_else(|| hbb_common::anyhow::anyhow!("无法确认被控服务开关，请稍后重试"))?;
    let resume_host =
        mode.as_deref() == Some("host") && !option2bool(keys::OPTION_STOP_SERVICE, &stopped);
    request_mode("control")?;
    crate::ipc::set_config(keys::OPTION_SEALDESK_RESET_ADDRESS, "Y".to_owned())?;
    if resume_host {
        request_mode("host")?;
    }
    Ok(())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn daemon_absent(error: &hbb_common::anyhow::Error) -> bool {
    error
        .downcast_ref::<std::io::Error>()
        .map(|error| {
            matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            )
        })
        .unwrap_or(false)
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) fn query_mode() -> hbb_common::ResultType<String> {
    match crate::ipc::get_config(keys::OPTION_SEALDESK_HOST_MODE) {
        Ok(Some(mode)) => Ok(mode),
        Err(error) if daemon_absent(&error) => Ok("control".to_owned()),
        _ => hbb_common::bail!("无法确认被控服务状态，请稍后重试"),
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[derive(Debug)]
pub(crate) enum HostRequest {
    /// No IPC server yet and the user is controlling only. Do not call `start_server`.
    ControlWithoutServer,
    /// No IPC server yet and the share card asked for host. `request_mode` spawns
    /// `start_server(true, false)` for this variant only.
    StartServer,
    /// A server is already running; switch it through IPC.
    ConfigureRunningDaemon,
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) fn host_request_action(
    mode: &str,
    outgoing_only: bool,
    daemon_absent: bool,
) -> hbb_common::ResultType<HostRequest> {
    use hbb_common::bail;
    if !matches!(mode, "host" | "control") {
        bail!("请选择被控此设备或控制其他设备");
    }
    if mode == "host" && outgoing_only {
        bail!("当前应用配置仅允许控制其他设备");
    }
    if !daemon_absent {
        return Ok(HostRequest::ConfigureRunningDaemon);
    }
    if mode == "control" {
        Ok(HostRequest::ControlWithoutServer)
    } else {
        Ok(HostRequest::StartServer)
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) fn request_mode(mode: &str) -> hbb_common::ResultType<()> {
    use hbb_common::anyhow::anyhow;
    use std::{
        sync::Once,
        time::{Duration, Instant},
    };

    let outgoing_only = hbb_common::config::is_outgoing_only();
    // Reject outgoing-only and unknown modes before any IPC attempt.
    host_request_action(mode, outgoing_only, true)?;
    let absent = match crate::ipc::get_config(keys::OPTION_SEALDESK_HOST_MODE) {
        Err(error) if daemon_absent(&error) => true,
        // Only the first probe is marked. A later timeout (set/confirm) must
        // stay an error so startup does not skip `set_mode`.
        Err(error) if cfg!(unix) && peer_did_not_answer(&error) => {
            return Err(error.context(StartupProbeUnanswered));
        }
        Err(error) => return Err(error),
        Ok(_) => false,
    };
    match host_request_action(mode, outgoing_only, absent)? {
        HostRequest::ControlWithoutServer => {
            set_mode(mode.to_owned());
            set_address(String::new());
            return Ok(());
        }
        HostRequest::StartServer => {
            set_mode(mode.to_owned());
            static START: Once = Once::new();
            START.call_once(|| {
                std::thread::spawn(|| crate::start_server(true, false));
            });
        }
        HostRequest::ConfigureRunningDaemon => {}
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match crate::ipc::set_config(keys::OPTION_SEALDESK_HOST_MODE, mode.to_owned()) {
            Ok(()) => break,
            Err(error) if daemon_absent(&error) && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(error) => return Err(error),
        }
    }
    loop {
        if crate::ipc::get_config(keys::OPTION_SEALDESK_HOST_MODE)?.as_deref() == Some(mode) {
            set_mode(mode.to_owned());
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(anyhow!("切换被控服务超时，请稍后重试"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Ordinary host startup continues when the first host-mode probe times out,
/// so this process can open its window. Unix only: on Windows that timeout is
/// also a live pipe with no free instance, and continuing makes
/// `start_server(false)` fall through to a competing `first_pipe_instance`
/// bind, which calls `process::exit`. Control-only launches and any later
/// timeout return the error unchanged.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) fn request_startup_mode(mode: &str) -> hbb_common::ResultType<()> {
    finish_startup_request(mode, request_mode(mode))
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn finish_startup_request(
    mode: &str,
    result: hbb_common::ResultType<()>,
) -> hbb_common::ResultType<()> {
    if let Err(error) = &result {
        if host_startup_swallows_probe(cfg!(unix), mode, error) {
            hbb_common::log::error!(
                "Existing instance did not answer host-mode IPC within the timeout ({error}); starting this instance"
            );
            return Ok(());
        }
    }
    result
}

/// `unix_ipc` is false on Windows. Swallowing the probe there is followed by
/// `start_server(false)`, whose connect hits the same timeout and then binds
/// a second pipe server.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn host_startup_swallows_probe(
    unix_ipc: bool,
    mode: &str,
    error: &hbb_common::anyhow::Error,
) -> bool {
    unix_ipc && mode == "host" && error.downcast_ref::<StartupProbeUnanswered>().is_some()
}

/// Marks the initial host-mode probe timeout. Display stays "deadline has elapsed".
#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[derive(Debug)]
struct StartupProbeUnanswered;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
impl std::fmt::Display for StartupProbeUnanswered {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("deadline has elapsed")
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
impl std::error::Error for StartupProbeUnanswered {}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn peer_did_not_answer(error: &hbb_common::anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<hbb_common::tokio::time::error::Elapsed>()
            .is_some()
    })
}

#[cfg(all(target_os = "linux", feature = "flutter"))]
pub(crate) fn stop_for_exit() {
    if let Err(error) = request_mode("control") {
        hbb_common::log::warn!("Could not stop sharing while closing SealDesk: {error}");
    }
    let children = std::mem::take(&mut *crate::server::CHILD_PROCESS.lock().unwrap());
    for mut child in children {
        if matches!(child.try_wait(), Ok(Some(_))) {
            continue;
        }
        if let Err(error) = child.kill() {
            hbb_common::log::warn!("Could not stop SealDesk child {}: {error}", child.id());
        } else if let Err(error) = child.wait() {
            hbb_common::log::warn!("Could not reap SealDesk child {}: {error}", child.id());
        }
    }
}

/// Live `TcServeStatus` JSON, with the single startup error attached when Go
/// left `init_error` empty. Empty when status is unavailable.
pub(crate) fn relay_status() -> String {
    let status = super::serve_status();
    if status.is_empty() {
        return String::new();
    }
    let merged = merge_relay_status(&status, &start_error());
    if !serde_json::from_str::<serde_json::Value>(&merged)
        .ok()
        .is_some_and(|value| value.is_object())
    {
        return String::new();
    }
    log_relay_last_error(&merged);
    merged
}

fn log_relay_last_error(status: &str) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(status) else {
        return;
    };
    let current = value
        .get("last_error")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .trim();
    static LAST: Mutex<String> = Mutex::new(String::new());
    static THROTTLE: hbb_common::log_throttle::LogThrottle =
        hbb_common::log_throttle::LogThrottle::new(std::time::Duration::from_secs(60));
    let mut last = LAST.lock().unwrap();
    if current.is_empty() {
        last.clear();
        return;
    }
    if *last == current {
        return;
    }
    // Remember the text only after it is logged, so a change inside the
    // interval is retried instead of dropped.
    let Some(n) = THROTTLE.due() else {
        return;
    };
    last.clear();
    last.push_str(current);
    drop(last);
    if n > 1 {
        hbb_common::log::error!("Relay unreachable: {current} (x{n})");
    } else {
        hbb_common::log::error!("Relay unreachable: {current}");
    }
}

/// Adds `rust_init_error` when `status` is a JSON object whose `init_error` is
/// missing or empty. Any other input is returned unchanged.
pub(crate) fn merge_relay_status(status: &str, rust_init_error: &str) -> String {
    if status.is_empty() || rust_init_error.is_empty() {
        return status.to_owned();
    }
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(status) else {
        return status.to_owned();
    };
    {
        let Some(object) = value.as_object_mut() else {
            return status.to_owned();
        };
        let go_init_error_empty = match object.get("init_error") {
            Some(serde_json::Value::String(text)) => text.is_empty(),
            Some(serde_json::Value::Null) | None => true,
            Some(_) => false,
        };
        if !go_init_error_empty {
            return status.to_owned();
        }
        object.insert(
            "rust_init_error".to_owned(),
            serde_json::Value::String(rust_init_error.to_owned()),
        );
    }
    serde_json::to_string(&value).unwrap_or_else(|_| status.to_owned())
}

fn scrub_init_error(message: &str) -> String {
    if message.is_empty() {
        return String::new();
    }
    let path = super::server_key_path();
    let without_path = if path.is_empty() {
        message.to_owned()
    } else {
        message.replace(&path, "")
    };
    let scrubbed = redact_typed_secrets(&without_path);
    if scrubbed.is_empty() {
        "[redacted]".to_owned()
    } else {
        scrubbed
    }
}

fn redact_typed_secrets(message: &str) -> String {
    let bytes = message.as_bytes();
    let mut out = String::with_capacity(message.len());
    let mut index = 0;
    while index < bytes.len() {
        if let Some(end) = secret_end(bytes, index) {
            out.push_str("[redacted]");
            index = end;
            continue;
        }
        let ch = message[index..].chars().next().unwrap_or('\u{FFFD}');
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}

fn secret_end(bytes: &[u8], index: usize) -> Option<usize> {
    const PREFIXES: [&[u8]; 3] = [b"nodekey:", b"privkey:", b"discokey:"];
    for prefix in PREFIXES {
        if bytes[index..].len() >= prefix.len()
            && bytes[index..index + prefix.len()].eq_ignore_ascii_case(prefix)
        {
            let mut end = index + prefix.len();
            let hex_start = end;
            while end < bytes.len() && bytes[end].is_ascii_hexdigit() {
                end += 1;
            }
            if end > hex_start {
                return Some(end);
            }
        }
    }
    if bytes.len() >= index + 2 && bytes[index] == b't' && bytes[index + 1] == b'c' {
        let mut end = index + 2;
        while end < bytes.len() && is_tailcat_addr_byte(bytes[end]) {
            end += 1;
        }
        if end - index >= 18 {
            return Some(end);
        }
    }
    None
}

fn is_tailcat_addr_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
}

#[cfg(all(test, not(any(target_os = "android", target_os = "ios"))))]
mod tests {
    use super::*;

    #[test]
    fn explicit_host_request_starts_the_server_only_if_the_daemon_is_absent() {
        assert!(matches!(
            host_request_action("control", false, true).unwrap(),
            HostRequest::ControlWithoutServer
        ));
        assert!(matches!(
            host_request_action("host", false, true).unwrap(),
            HostRequest::StartServer
        ));
        assert!(host_request_action("host", true, true).is_err());
        assert!(host_request_action("host", true, false).is_err());
        assert!(matches!(
            host_request_action("host", false, false).unwrap(),
            HostRequest::ConfigureRunningDaemon
        ));
        assert!(matches!(
            host_request_action("control", false, false).unwrap(),
            HostRequest::ConfigureRunningDaemon
        ));
        let outgoing = host_request_action("host", true, true)
            .unwrap_err()
            .to_string();
        assert!(outgoing.contains("仅允许控制"));
        assert!(!outgoing.to_ascii_lowercase().contains("start_server"));
    }

    #[test]
    fn only_missing_or_refused_daemons_count_as_inactive() {
        for kind in [
            std::io::ErrorKind::NotFound,
            std::io::ErrorKind::ConnectionRefused,
        ] {
            assert!(daemon_absent(&std::io::Error::from(kind).into()));
        }
        for kind in [
            std::io::ErrorKind::PermissionDenied,
            std::io::ErrorKind::TimedOut,
        ] {
            assert!(!daemon_absent(&std::io::Error::from(kind).into()));
        }
        assert!(!daemon_absent(&hbb_common::anyhow::anyhow!(
            "invalid response"
        )));
    }

    fn deadline_elapsed() -> hbb_common::anyhow::Error {
        let runtime = hbb_common::tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        runtime
            .block_on(async {
                hbb_common::tokio::time::timeout(
                    std::time::Duration::ZERO,
                    std::future::pending::<()>(),
                )
                .await
                .unwrap_err()
            })
            .into()
    }

    #[test]
    fn unanswered_peer_continues_host_startup() {
        let elapsed = deadline_elapsed();
        assert!(peer_did_not_answer(&elapsed));
        assert!(!peer_did_not_answer(&hbb_common::anyhow::anyhow!(
            "refused"
        )));

        let probe = elapsed.context(StartupProbeUnanswered);
        assert_eq!(probe.to_string(), "deadline has elapsed");
        assert!(host_startup_swallows_probe(true, "host", &probe));
        assert!(!host_startup_swallows_probe(false, "host", &probe));
        assert!(!host_startup_swallows_probe(true, "control", &probe));
        assert_eq!(
            finish_startup_request("host", Err(probe)).is_ok(),
            cfg!(unix)
        );

        assert!(!host_startup_swallows_probe(
            true,
            "host",
            &deadline_elapsed()
        ));
        assert!(finish_startup_request("host", Err(deadline_elapsed())).is_err());
        assert!(finish_startup_request(
            "control",
            Err(deadline_elapsed().context(StartupProbeUnanswered))
        )
        .is_err());
        assert!(finish_startup_request("host", Ok(())).is_ok());
    }
}

#[cfg(test)]
mod relay_status_tests {
    use super::merge_relay_status;

    #[test]
    fn rust_init_error_fills_only_an_empty_go_init_error() {
        let with_go =
            r#"{"running":false,"state":"stopped","init_error":"bind","init_stage":"start"}"#;
        assert_eq!(merge_relay_status(with_go, "serve: tailcat: bind"), with_go);

        let empty_go = r#"{"running":false,"state":"stopped","init_error":"","init_stage":""}"#;
        let merged = merge_relay_status(empty_go, "serve: say \"hi\"");
        let value: serde_json::Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(value["init_error"], "");
        assert_eq!(value["state"], "stopped");
        assert_eq!(value["rust_init_error"], "serve: say \"hi\"");

        let missing = r#"{"running":true,"state":"connecting"}"#;
        let merged = merge_relay_status(missing, "serve: boom");
        let value: serde_json::Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(value["running"], true);
        assert_eq!(value["rust_init_error"], "serve: boom");

        let null_go = r#"{"state":"stopped","init_error":null}"#;
        let merged = merge_relay_status(null_go, "serve: nil");
        let value: serde_json::Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(value["init_error"], serde_json::Value::Null);
        assert_eq!(value["rust_init_error"], "serve: nil");
    }

    #[test]
    fn empty_rust_init_error_does_not_add_a_field() {
        let status = r#"{"init_error":"","state":"stopped"}"#;
        assert_eq!(merge_relay_status(status, ""), status);
    }

    #[test]
    fn non_object_status_is_left_unchanged() {
        assert_eq!(merge_relay_status("", "serve: x"), "");
        assert_eq!(merge_relay_status("not-json", "serve: x"), "not-json");
        assert_eq!(merge_relay_status("[1]", "serve: x"), "[1]");
        assert_eq!(
            merge_relay_status(r#"{"init_error":1}"#, "serve: x"),
            r#"{"init_error":1}"#
        );
    }

    #[test]
    fn scrub_redacts_nodekey_privkey_and_tailcat_addresses() {
        let message = "dial tcABCDEFGHIJKLMNOPQRSTUVWXYZ012345 as nodekey:00112233445566778899aabbccddeeff and PRIVKEY:ffee then discokey:ab; tcp left";
        let got = super::redact_typed_secrets(message);
        assert!(!got.to_ascii_lowercase().contains("nodekey:0011"));
        assert!(!got.contains("00112233"));
        assert!(!got.to_ascii_lowercase().contains("privkey:ff"));
        assert!(!got.to_ascii_lowercase().contains("discokey:ab"));
        assert!(!got.contains("tcABCDEFGHIJKLMNOPQRSTUVWXYZ012345"));
        assert!(
            got.contains("dial [redacted] as [redacted] and [redacted] then [redacted]; tcp left")
        );
        assert_eq!(super::redact_typed_secrets(""), "");
        assert_eq!(super::redact_typed_secrets("nodekey:"), "nodekey:");
        assert_eq!(super::redact_typed_secrets("nodekey:zz"), "nodekey:zz");
        assert_eq!(super::redact_typed_secrets("tcshort"), "tcshort");
        assert_eq!(
            super::redact_typed_secrets("tcABCDEFGHIJKLMNOP"),
            "[redacted]"
        );
        assert_eq!(
            super::redact_typed_secrets("\u{9519}\u{8bef} nodekey:ab \u{7ed3}\u{675f}"),
            "\u{9519}\u{8bef} [redacted] \u{7ed3}\u{675f}"
        );
    }
}
