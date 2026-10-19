//! Tailcat tunnel (libs/tailcat-ffi, Go) in place of the rendezvous / relay servers.
//!
//! The controlled side runs a tailcat server whose address (`tc...`) is the only thing a
//! controller needs: it carries the WireGuard key, a pre-shared key and the DERP region used to
//! meet. WireGuard uses a direct UDP path when hole punching succeeds; otherwise DERP
//! continues relaying the encrypted session traffic. The address is a credential, share it as
//! a QR code.

use hbb_common::{bail, config::Config, log, ResultType};
use std::ffi::{c_char, c_int, CStr, CString};
#[cfg(any(target_os = "android", test))]
use std::net::Ipv4Addr;
#[cfg(target_os = "android")]
use std::sync::Mutex;

pub(crate) mod auth_limit;
#[cfg(test)]
mod build_target;
#[cfg(all(
    not(any(target_os = "android", target_os = "ios")),
    any(feature = "flutter", test)
))]
pub(crate) mod clipboard;
pub(crate) mod host;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) mod launch;
pub mod pairing;
#[cfg(all(
    not(any(target_os = "android", target_os = "ios")),
    any(feature = "flutter", test)
))]
pub(crate) mod screen_region;
pub(crate) mod transport;

#[cfg(all(
    test,
    not(feature = "flutter"),
    not(any(target_os = "android", target_os = "ios"))
))]
mod live_tests;

#[cfg(target_os = "android")]
extern "C" {
    fn TcSetAndroidInterfaces(payload: *const c_char);
}

#[cfg(target_os = "android")]
static ANDROID_INTERFACES: Mutex<String> = Mutex::new(String::new());

#[cfg(target_os = "android")]
pub(crate) fn set_android_interfaces(payload: &std::ffi::CStr) {
    if let Ok(text) = payload.to_str() {
        *ANDROID_INTERFACES.lock().unwrap() = text.to_owned();
    }
    unsafe { TcSetAndroidInterfaces(payload.as_ptr()) }
}

#[cfg(target_os = "android")]
pub(crate) fn android_private_ipv4() -> Vec<Ipv4Addr> {
    let payload = ANDROID_INTERFACES.lock().unwrap().clone();
    private_ipv4_from_interface_json(&payload)
}

/// `flags`: 1 = up, 4 = loopback. Addresses are `ip/prefix` strings.
#[cfg(any(target_os = "android", test))]
pub(crate) fn private_ipv4_from_interface_json(payload: &str) -> Vec<Ipv4Addr> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
        return Vec::new();
    };
    let Some(list) = value.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for iface in list {
        let flags = iface.get("flags").and_then(|v| v.as_u64()).unwrap_or(0);
        if flags & 1 == 0 || flags & 4 != 0 {
            continue;
        }
        let Some(addrs) = iface.get("addrs").and_then(|v| v.as_array()) else {
            continue;
        };
        for addr in addrs {
            let Some(text) = addr.as_str() else {
                continue;
            };
            let ip = text.split('/').next().unwrap_or("");
            let Ok(ip) = ip.parse::<Ipv4Addr>() else {
                continue;
            };
            if ip.is_private() && !out.contains(&ip) {
                out.push(ip);
            }
        }
    }
    out
}

extern "C" {
    fn TcFree(p: *mut c_char);
    fn TcServeStatus() -> *mut c_char;
    fn TcStopServe() -> *mut c_char;
    fn TcServeWithDERPMap(
        key_path: *const c_char,
        allow: *const c_char,
        target_port: c_int,
        token: *const c_char,
        derp_map_url: *const c_char,
    ) -> *mut c_char;
    fn TcClientPubkey(key_path: *const c_char) -> *mut c_char;
    fn TcForwardWithDERPMap(
        addr: *const c_char,
        key_path: *const c_char,
        timeout_ms: c_int,
        token: *const c_char,
        derp_map_url: *const c_char,
    ) -> *mut c_char;
    fn TcCloseForward(addr: *const c_char);
    fn TcPath(addr: *const c_char, timeout_ms: c_int) -> *mut c_char;
    fn TcQR(s: *const c_char) -> *mut c_char;
}

pub const APP_NAME: &str = "SealDesk";

/// Comma-separated `nodekey:...` list of controllers allowed in. Empty allows anyone holding
/// the address.
pub const OPTION_ALLOW: &str = "tailcat-allow";
/// This machine's tailcat address, published by the server for the UI.
pub const OPTION_ADDR: &str = "tailcat-addr";
/// Live controlled-side tunnel status JSON. Read through the service; not stored.
pub const OPTION_RELAY_STATUS: &str = "tailcat-relay-status";

fn cstring(s: &str) -> ResultType<CString> {
    Ok(CString::new(s)?)
}

fn take(p: *mut c_char) -> ResultType<String> {
    if p.is_null() {
        bail!("tailcat: null result");
    }
    // SAFETY: every Tc* export returns a NUL-terminated C.CString that we own.
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { TcFree(p) };
    match s.strip_prefix("ERR:") {
        Some(err) => bail!("tailcat: {err}"),
        None => Ok(s),
    }
}

fn server_key_path() -> String {
    Config::path("tailcat-server.json")
        .to_string_lossy()
        .into_owned()
}

fn client_key_path() -> String {
    Config::path("tailcat-client.json")
        .to_string_lossy()
        .into_owned()
}

#[cfg(target_os = "linux")]
fn secure_key_file(path: &str) -> ResultType<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

/// A tailcat address: `tc` + unpadded base64url. Always far longer than a RustDesk ID.
pub fn is_addr(s: &str) -> bool {
    s.len() > 40
        && s.starts_with("tc")
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Starts the controlled-side tunnel, forwarding every tunnel connection to
/// `127.0.0.1:target_port`. Blocks until DERP is reachable; returns this machine's address.
/// Idempotent.
pub fn serve(target_port: u16, token: &[u8; 32]) -> ResultType<String> {
    let key = cstring(&server_key_path())?;
    let allow = cstring(&Config::get_option(OPTION_ALLOW))?;
    let token = cstring(&hex::encode(token))?;
    let derp_map_url = cstring(&Config::get_option(
        base::config::keys::OPTION_TAILCAT_DERP_MAP_URL,
    ))?;
    let address = take(unsafe {
        TcServeWithDERPMap(
            key.as_ptr(),
            allow.as_ptr(),
            target_port as c_int,
            token.as_ptr(),
            derp_map_url.as_ptr(),
        )
    })?;
    #[cfg(target_os = "linux")]
    secure_key_file(key.to_str()?)?;
    Ok(address)
}

pub(crate) fn stop_serving() -> ResultType<()> {
    take(unsafe { TcStopServe() })?;
    Ok(())
}

/// Controlled-side tunnel status JSON from `TcServeStatus`. Empty when the
/// library returns nothing or an `ERR:` result. The pointer is freed with `TcFree`.
pub(crate) fn serve_status() -> String {
    match take(unsafe { TcServeStatus() }) {
        Ok(status) => status,
        Err(error) => {
            log::trace!("tailcat status unavailable: {error}");
            String::new()
        }
    }
}

/// This machine's controller identity, for the peer's allow list.
pub fn client_pubkey() -> ResultType<String> {
    let key = cstring(&client_key_path())?;
    let pubkey = take(unsafe { TcClientPubkey(key.as_ptr()) })?;
    #[cfg(target_os = "linux")]
    secure_key_file(key.to_str()?)?;
    Ok(pubkey)
}

/// Opens the tunnel to `addr` and returns a loopback `ip:port` that reaches the peer's RustDesk.
/// Blocking.
fn forward(addr: &str, timeout_ms: u64, token: &[u8; 32]) -> ResultType<LocalForward> {
    let a = cstring(addr)?;
    let key = cstring(&client_key_path())?;
    let token = cstring(&hex::encode(token))?;
    let timeout_ms = c_int::try_from(timeout_ms)?;
    let derp_map_url = cstring(&Config::get_option(
        base::config::keys::OPTION_TAILCAT_DERP_MAP_URL,
    ))?;
    let local = take(unsafe {
        TcForwardWithDERPMap(
            a.as_ptr(),
            key.as_ptr(),
            timeout_ms,
            token.as_ptr(),
            derp_map_url.as_ptr(),
        )
    })?;
    #[cfg(target_os = "linux")]
    secure_key_file(key.to_str()?)?;
    Ok(LocalForward(cstring(&local)?))
}

struct LocalForward(CString);

impl Drop for LocalForward {
    fn drop(&mut self) {
        unsafe { TcCloseForward(self.0.as_ptr()) };
    }
}

/// "direct <ip:port>" once hole punching worked, otherwise "derp". Blocking.
pub fn path(addr: &str, timeout_ms: u64) -> ResultType<String> {
    let a = cstring(addr)?;
    take(unsafe { TcPath(a.as_ptr(), timeout_ms as c_int) })
}

fn path_is_direct(path: &str) -> Option<bool> {
    if path == "derp" {
        Some(false)
    } else if path
        .strip_prefix("direct ")
        .is_some_and(|endpoint| !endpoint.is_empty())
    {
        Some(true)
    } else {
        None
    }
}

pub(crate) async fn watch_path(addr: &str, mut update: impl FnMut(bool)) {
    use hbb_common::tokio::{
        task::spawn_blocking,
        time::{sleep, Duration},
    };

    let mut previous = None;
    loop {
        let address = addr.to_owned();
        match spawn_blocking(move || path(&address, 1_000)).await {
            Ok(Ok(path)) => {
                if let Some(direct) = path_is_direct(&path) {
                    if previous != Some(direct) {
                        update(direct);
                        previous = Some(direct);
                    }
                }
            }
            Ok(Err(error)) => log::trace!("tailcat path probe: {error}"),
            Err(error) => log::trace!("tailcat path task: {error}"),
        }
        sleep(Duration::from_secs(3)).await;
    }
}

/// `s` as a QR code drawn with Unicode half blocks.
pub fn qr_text(s: &str) -> ResultType<String> {
    let a = cstring(s)?;
    take(unsafe { TcQR(a.as_ptr()) })
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn current_pairing_code(lifetime_secs: u64) -> ResultType<String> {
    use std::time::{SystemTime, UNIX_EPOCH};

    if lifetime_secs == 0 {
        bail!("Pairing lifetime must be greater than zero");
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let expires_at = now
        .checked_add(lifetime_secs)
        .ok_or_else(|| hbb_common::anyhow::anyhow!("Pairing expiry timestamp is too large"))?;
    let options = crate::ipc::get_options();
    let address = options.get(OPTION_ADDR).filter(|address| is_addr(address));
    let Some(address) = address else {
        bail!("No tailcat address yet; start the SealDesk server and try again");
    };
    let server_password = crate::ipc::get_config("temporary-password")?
        .ok_or_else(|| hbb_common::anyhow::anyhow!("SealDesk server did not return a password"))?;
    let password = if options
        .get(base::config::keys::OPTION_APPROVE_MODE)
        .map(String::as_str)
        == Some("click")
        || options
            .get(base::config::keys::OPTION_VERIFICATION_METHOD)
            .map(String::as_str)
            == Some("use-permanent-password")
        || server_password.is_empty()
    {
        None
    } else {
        Some(server_password)
    };
    Ok(pairing::PairingCode {
        address: address.to_owned(),
        password,
        name: Some(crate::common::hostname()),
        expires_at: Some(expires_at),
    }
    .encode_at(now)?)
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn print_pairing_code(args: &[String]) -> ResultType<()> {
    if args.len() > 2 {
        bail!("Usage: --tc-pair [lifetime-seconds] (default: 600)");
    }
    let lifetime_secs = match args.get(1) {
        Some(value) => value.parse::<u64>()?,
        None => 600,
    };
    let code = current_pairing_code(lifetime_secs)?;
    let qr = qr_text(&code)?;
    eprintln!("提示：过期时间由控制端检查，不会在被控端撤销凭证；刷新临时密码后，之前打印的临时密码配对码不能用于新的密码登录。只接受点击确认时不附带临时密码。");
    println!("{qr}\n{code}");
    Ok(())
}

pub async fn connect(addr: &str, timeout_ms: u64) -> ResultType<hbb_common::Stream> {
    use hbb_common::tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpStream,
        task::spawn_blocking,
        time::{timeout, Duration},
    };

    let addr = addr.to_owned();
    let token = transport::local_token();
    let local = spawn_blocking(move || forward(&addr, timeout_ms, &token)).await??;
    let address = local.0.to_str()?;
    let mut stream = timeout(Duration::from_secs(5), TcpStream::connect(address)).await??;
    stream.set_nodelay(true)?;
    timeout(Duration::from_secs(1), stream.write_all(&token)).await??;
    let mut acknowledgement = [0];
    timeout(
        Duration::from_secs(2),
        stream.read_exact(&mut acknowledgement),
    )
    .await??;
    if acknowledgement != [1] {
        bail!("Local tailcat forward rejected authentication");
    }
    let local_addr = stream.local_addr()?;
    log::info!("tailcat tunnel ready");
    Ok(hbb_common::Stream::from(stream, local_addr))
}

#[cfg(test)]
mod tests {
    use super::path_is_direct;

    #[cfg(target_os = "linux")]
    #[test]
    fn secure_key_file_sets_private_mode() {
        use std::{fs, os::unix::fs::PermissionsExt};

        let path =
            std::env::temp_dir().join(format!("rustdesk-tailcat-mode-test-{}", std::process::id()));
        fs::write(&path, b"key").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        super::secure_key_file(path.to_str().unwrap()).unwrap();

        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn path_status_distinguishes_direct_relay_and_unknown() {
        assert_eq!(path_is_direct("direct 192.0.2.1:1234"), Some(true));
        assert_eq!(path_is_direct("direct [2001:db8::1]:1234"), Some(true));
        assert_eq!(path_is_direct("derp"), Some(false));
        assert_eq!(path_is_direct("direct "), None);
        assert_eq!(path_is_direct(""), None);
        assert_eq!(path_is_direct("ERR:no active tunnel"), None);
    }
}
