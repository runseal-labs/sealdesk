use base::config::keys;
use hbb_common::config::{option2bool, Config};
use std::sync::Mutex;

static RUNNING: Mutex<bool> = Mutex::new(false);
static START_ERROR: Mutex<String> = Mutex::new(String::new());

pub(crate) fn start_error() -> String {
    START_ERROR.lock().unwrap().clone()
}

pub(crate) fn set_start_error(error: String) {
    *START_ERROR.lock().unwrap() = error;
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
    preserve_host_state(&mut options);
    Config::set_options(options);
}

pub(crate) fn sync_config(mut config: hbb_common::config::Config2) -> bool {
    let _running = RUNNING.lock().unwrap();
    preserve_host_state(&mut config.options);
    hbb_common::config::Config2::set(config)
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
pub(crate) fn request_mode(mode: &str) -> hbb_common::ResultType<()> {
    use hbb_common::{anyhow::anyhow, bail};
    use std::{
        sync::Once,
        time::{Duration, Instant},
    };

    if !matches!(mode, "host" | "control") {
        bail!("请选择被控此设备或控制其他设备");
    }
    if mode == "host" && hbb_common::config::is_outgoing_only() {
        bail!("当前应用配置仅允许控制其他设备");
    }
    match crate::ipc::get_config(keys::OPTION_SEALDESK_HOST_MODE) {
        Err(error) if daemon_absent(&error) => {
            set_mode(mode.to_owned());
            if mode == "control" {
                set_address(String::new());
                return Ok(());
            }
            static START: Once = Once::new();
            START.call_once(|| {
                std::thread::spawn(|| crate::start_server(true, false));
            });
        }
        Err(error) => return Err(error),
        Ok(_) => {}
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

#[cfg(all(test, not(any(target_os = "android", target_os = "ios"))))]
mod tests {
    use super::*;

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
}
