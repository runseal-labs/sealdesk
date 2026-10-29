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
pub(crate) mod history;
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
/// CGNAT addresses kept from the raw interface list. Discovery JSON drops VPN
/// and point-to-point interfaces, so Tailscale 100.64.0.0/10 would otherwise
/// never reach the TCP 21118 bind.
#[cfg(target_os = "android")]
static ANDROID_CGNAT: Mutex<Vec<Ipv4Addr>> = Mutex::new(Vec::new());

#[cfg(target_os = "android")]
pub(crate) fn set_android_interfaces(payload: &std::ffi::CStr) {
    if let Ok(text) = payload.to_str() {
        *ANDROID_INTERFACES.lock().unwrap() = android_lan_interface_json(text);
        *ANDROID_CGNAT.lock().unwrap() = crate::lan::cgnat_v4_from_interface_json(text);
    }
    unsafe { TcSetAndroidInterfaces(payload.as_ptr()) }
}

#[cfg(target_os = "android")]
pub(crate) fn android_cgnat_ipv4() -> Vec<Ipv4Addr> {
    ANDROID_CGNAT.lock().unwrap().clone()
}

#[cfg(target_os = "android")]
pub(crate) fn android_interface_json() -> String {
    ANDROID_INTERFACES.lock().unwrap().clone()
}

#[cfg(target_os = "android")]
pub(crate) fn android_private_ipv4() -> Vec<Ipv4Addr> {
    let payload = ANDROID_INTERFACES.lock().unwrap().clone();
    private_ipv4_from_interface_json(&payload)
}

#[cfg(target_os = "android")]
pub(crate) fn android_private_nets() -> Vec<(Ipv4Addr, u8)> {
    let payload = ANDROID_INTERFACES.lock().unwrap().clone();
    private_nets_from_interface_json(&payload)
}

/// Go `net.Flags` in the Android interface JSON: 1 up, 4 loopback, 8 point-to-point.
/// `tun*`, `ppp*`, and `wg*` are VPN names. Other `TRANSPORT_VPN` interfaces are
/// tagged `vpn: true` and removed by `android_lan_interface_json`.
pub(crate) fn android_iface_usable(name: &str, flags: u64) -> bool {
    if flags & 1 == 0 || flags & 4 != 0 || flags & 8 != 0 {
        return false;
    }
    !android_vpn_iface_name(name)
}

/// Copy stored for LAN discovery. `TcSetAndroidInterfaces` keeps the original
/// payload so netmon still sees VPN up/down.
#[cfg(any(target_os = "android", test))]
pub(crate) fn android_lan_interface_json(payload: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
        return payload.to_owned();
    };
    let Some(list) = value.as_array() else {
        return payload.to_owned();
    };
    let kept: Vec<serde_json::Value> = list
        .iter()
        .filter(|iface| {
            let name = iface.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let flags = iface.get("flags").and_then(|v| v.as_u64()).unwrap_or(0);
            android_iface_usable(name, flags)
                && !iface.get("vpn").and_then(|v| v.as_bool()).unwrap_or(false)
        })
        .cloned()
        .collect();
    serde_json::to_string(&kept).unwrap_or_else(|_| payload.to_owned())
}

pub(crate) fn android_vpn_iface_name(name: &str) -> bool {
    name.get(..3)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("tun"))
        || name
            .get(..3)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("ppp"))
        || name
            .get(..2)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("wg"))
}

/// Addresses are `ip/prefix`. Prefix is kept so LAN dials can require the same subnet.
#[cfg(any(target_os = "android", test))]
pub(crate) fn private_nets_from_interface_json(payload: &str) -> Vec<(Ipv4Addr, u8)> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
        return Vec::new();
    };
    let Some(list) = value.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for iface in list {
        let name = iface.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let flags = iface.get("flags").and_then(|v| v.as_u64()).unwrap_or(0);
        if !android_iface_usable(name, flags) {
            continue;
        }
        let Some(addrs) = iface.get("addrs").and_then(|v| v.as_array()) else {
            continue;
        };
        for addr in addrs {
            let Some(text) = addr.as_str() else {
                continue;
            };
            let mut parts = text.split('/');
            let Some(ip) = parts.next() else {
                continue;
            };
            let Ok(ip) = ip.parse::<Ipv4Addr>() else {
                continue;
            };
            let prefix = parts
                .next()
                .and_then(|value| value.parse::<u8>().ok())
                .unwrap_or(0);
            if ip.is_private()
                && prefix > 0
                && prefix <= 32
                && !out.iter().any(|(seen, _)| *seen == ip)
            {
                out.push((ip, prefix));
            }
        }
    }
    out
}

/// `flags` follow Go `net.Flags`. Addresses are `ip/prefix` strings.
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
        let name = iface.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let flags = iface.get("flags").and_then(|v| v.as_u64()).unwrap_or(0);
        if !android_iface_usable(name, flags) {
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
    fn TcNodePublic(addr: *const c_char) -> *mut c_char;
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
    fn TcSetProbeUi(share: c_int, background: c_int, screen: c_int);
    fn TcSetProbeIncoming(incoming: c_int);
}

pub const APP_NAME: &str = "SealDesk";

/// Comma-separated `nodekey:...` list of controllers allowed in. Empty allows anyone holding
/// the address.
pub const OPTION_ALLOW: &str = "tailcat-allow";
/// This machine's tailcat address, published by the server for the UI.
pub const OPTION_ADDR: &str = "tailcat-addr";
/// Live controlled-side tunnel status JSON. Read through the service; not stored.
pub const OPTION_RELAY_STATUS: &str = "tailcat-relay-status";
/// UI lifecycle for the self-probe. Not stored. `share`, `background`, `screen` are 0 or 1.
pub(crate) const OPTION_PROBE_DEMAND: &str = "tailcat-probe-demand";

fn flag_int(on: bool) -> c_int {
    if on {
        1
    } else {
        0
    }
}

fn parse_probe_ui(value: &str) -> Option<(bool, bool, bool)> {
    let mut share = None;
    let mut background = None;
    let mut screen = None;
    if value.is_empty() {
        return None;
    }
    for part in value.split(',') {
        let (key, flag) = part.trim().split_once('=')?;
        let on = match flag.trim() {
            "1" => true,
            "0" => false,
            _ => return None,
        };
        match key.trim() {
            "share" => share = Some(on),
            "background" => background = Some(on),
            "screen" => screen = Some(on),
            _ => return None,
        }
    }
    Some((share?, background?, screen?))
}

fn apply_probe_ui(value: &str) -> bool {
    let Some((share, background, screen)) = parse_probe_ui(value) else {
        return false;
    };
    // SAFETY: the arguments are plain C ints. The Go side only stores flags.
    unsafe {
        TcSetProbeUi(flag_int(share), flag_int(background), flag_int(screen));
    }
    true
}

/// Records whether any controlled session is up. Called from the server process.
pub(crate) fn set_probe_incoming_from_sessions() {
    let incoming = !crate::server::AUTHED_CONNS.lock().unwrap().is_empty();
    // SAFETY: the argument is 0 or 1. Go does not call back into Rust.
    unsafe { TcSetProbeIncoming(flag_int(incoming)) }
}

pub(crate) fn set_probe_ui(value: &str) {
    if !apply_probe_ui(value) {
        log::debug!("ignored tailcat probe demand");
    }
}

/// Forwards share-page and lifecycle flags to the process that runs the probe.
pub(crate) fn publish_probe_ui(value: &str) {
    if parse_probe_ui(value).is_none() {
        log::debug!("ignored tailcat probe demand");
        return;
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        set_probe_ui(value);
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        if let Err(error) = crate::ipc::set_config(OPTION_PROBE_DEMAND, value.to_owned()) {
            // The share page republishes once a second. A stopped service would
            // otherwise append this line on every poll for as long as it stays open.
            hbb_common::throttled_log!(
                std::time::Duration::from_secs(60),
                debug,
                "tailcat probe demand was not delivered: {error}"
            );
        }
    }
}

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

pub(crate) fn server_key_path() -> String {
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

/// Node secret from the controlled side's tailcat key file, or `None` when
/// the file is missing or unreadable. Callers drop it when the possession
/// proof is sealed.
pub(crate) fn node_private_key() -> Option<zeroize::Zeroizing<[u8; 32]>> {
    node_private_key_at(&server_key_path())
}

pub(crate) fn node_private_key_at(path: &str) -> Option<zeroize::Zeroizing<[u8; 32]>> {
    use zeroize::Zeroize;

    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let encoded = value.get("Private")?.as_str()?;
    let hex_key = encoded.strip_prefix("privkey:")?;
    let mut bytes = hex::decode(hex_key).ok()?;
    if bytes.len() != 32 {
        bytes.zeroize();
        return None;
    }
    let mut key = zeroize::Zeroizing::new([0u8; 32]);
    key.copy_from_slice(&bytes);
    bytes.zeroize();
    Some(key)
}

/// Curve25519 node public key embedded in a tailcat address.
/// `None` when the address cannot be read. Used only to open a possession proof.
pub(crate) fn node_public(addr: &str) -> Option<[u8; 32]> {
    let c = cstring(addr).ok()?;
    // SAFETY: `addr` is a NUL-terminated C string. The Go side parses it and
    // returns a hex public key or an `ERR:` string. It does not call Rust.
    let text = take(unsafe { TcNodePublic(c.as_ptr()) }).ok()?;
    let bytes = hex::decode(text).ok()?;
    if bytes.len() != 32 {
        return None;
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Some(out)
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

/// `None` until disco reports DERP (`false`) or a direct endpoint (`true`).
pub(crate) fn initial_tailcat_direct() -> Option<bool> {
    None
}

pub(crate) fn tailcat_stream_type(direct: Option<bool>) -> &'static str {
    match direct {
        Some(true) => "Tailcat/P2P",
        Some(false) => "Tailcat/DERP",
        None => "Tailcat",
    }
}

pub(crate) fn direct_flag(direct: Option<bool>) -> &'static str {
    match direct {
        Some(true) => "true",
        Some(false) => "false",
        None => "unknown",
    }
}

/// Direct flag to publish for this disco report, if it is new.
pub(crate) fn publish_path_change(previous: Option<bool>, report: &str) -> Option<bool> {
    let direct = path_is_direct(report)?;
    if previous == Some(direct) {
        None
    } else {
        Some(direct)
    }
}

pub(crate) async fn watch_path(addr: &str, mut update: impl FnMut(bool)) {
    use hbb_common::tokio::{
        task::spawn_blocking,
        time::{sleep, Duration},
    };

    let mut previous = initial_tailcat_direct();
    loop {
        let address = addr.to_owned();
        match spawn_blocking(move || path(&address, 1_000)).await {
            Ok(Ok(path)) => {
                if let Some(direct) = publish_path_change(previous, &path) {
                    update(direct);
                    previous = Some(direct);
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
    #[test]
    fn probe_ui_demand_requires_all_three_flags() {
        assert_eq!(
            super::parse_probe_ui("share=1,background=0,screen=0"),
            Some((true, false, false))
        );
        assert_eq!(
            super::parse_probe_ui("share=0,background=1,screen=1"),
            Some((false, true, true))
        );
        assert_eq!(super::parse_probe_ui("share=1,background=0"), None);
        assert_eq!(super::parse_probe_ui("share=2,background=0,screen=0"), None);
        assert_eq!(
            super::parse_probe_ui("share=1,background=0,screen=0,extra=1"),
            None
        );
        assert_eq!(super::parse_probe_ui(""), None);
    }

    fn path_status_distinguishes_direct_relay_and_unknown() {
        assert_eq!(path_is_direct("direct 192.0.2.1:1234"), Some(true));
        assert_eq!(path_is_direct("direct [2001:db8::1]:1234"), Some(true));
        assert_eq!(path_is_direct("derp"), Some(false));
        assert_eq!(path_is_direct("direct "), None);
        assert_eq!(path_is_direct(""), None);
        assert_eq!(path_is_direct("ERR:no active tunnel"), None);
    }

    #[test]
    fn path_state_moves_from_unknown_through_derp_and_p2p() {
        fn apply(state: &mut Option<bool>, report: &str) -> Option<bool> {
            let published = super::publish_path_change(*state, report);
            if let Some(direct) = published {
                *state = Some(direct);
            }
            published
        }

        assert_eq!(super::initial_tailcat_direct(), None);
        assert_eq!(super::tailcat_stream_type(None), "Tailcat");
        assert_eq!(super::direct_flag(None), "unknown");

        let mut state = super::initial_tailcat_direct();
        assert_eq!(apply(&mut state, ""), None);
        assert_eq!(state, None);

        assert_eq!(apply(&mut state, "derp"), Some(false));
        assert_eq!(super::tailcat_stream_type(state), "Tailcat/DERP");
        assert_eq!(super::direct_flag(state), "false");
        assert_eq!(apply(&mut state, "derp"), None);

        assert_eq!(apply(&mut state, "direct 192.0.2.1:9"), Some(true));
        assert_eq!(super::tailcat_stream_type(state), "Tailcat/P2P");
        assert_eq!(super::direct_flag(state), "true");

        assert_eq!(apply(&mut state, "derp"), Some(false));
        assert_eq!(state, Some(false));
        assert_eq!(super::tailcat_stream_type(state), "Tailcat/DERP");
        assert_eq!(super::direct_flag(state), "false");
    }

    #[test]
    fn android_interface_json_keeps_private_prefixes() {
        let payload =
            r#"[{"flags":1,"addrs":["192.168.1.8/24","8.8.8.8/32","10.1.2.3/8","127.0.0.1/8"]}]"#;
        let nets = super::private_nets_from_interface_json(payload);
        assert_eq!(
            nets,
            vec![
                ("192.168.1.8".parse().unwrap(), 24),
                ("10.1.2.3".parse().unwrap(), 8),
            ]
        );
        let down = r#"[{"flags":0,"addrs":["192.168.1.8/24"]}]"#;
        assert!(super::private_nets_from_interface_json(down).is_empty());
    }

    #[test]
    fn android_interface_json_skips_vpn_point_to_point_and_loopback() {
        let payload = r#"[
            {"name":"tun0","flags":9,"addrs":["10.8.0.2/24"]},
            {"name":"ppp0","flags":1,"addrs":["10.9.0.2/24"]},
            {"name":"wg0","flags":1,"addrs":["10.7.0.2/32"]},
            {"name":"wlan0","flags":1,"addrs":["192.168.4.2/24"]},
            {"name":"lo","flags":5,"addrs":["127.0.0.1/8"]},
            {"name":"down0","flags":0,"addrs":["192.168.9.2/24"]}
        ]"#;
        assert_eq!(
            super::private_ipv4_from_interface_json(payload),
            vec!["192.168.4.2".parse::<std::net::Ipv4Addr>().unwrap()]
        );
        assert_eq!(
            super::private_nets_from_interface_json(payload),
            vec![("192.168.4.2".parse::<std::net::Ipv4Addr>().unwrap(), 24)]
        );
        assert!(super::android_vpn_iface_name("TUN1"));
        assert!(super::android_vpn_iface_name("Wg0"));
        assert!(!super::android_vpn_iface_name("wlan0"));
        assert!(!super::android_iface_usable("wlan0", 0));
    }

    #[test]
    fn android_lan_interface_json_drops_transport_vpn_and_point_to_point() {
        let payload = r#"[
            {"name":"wlan0","flags":17,"addrs":["192.168.4.2/24"]},
            {"name":"tun0","flags":1,"vpn":true,"addrs":["10.8.0.2/24"]},
            {"name":"ipsec0","flags":17,"vpn":true,"addrs":["10.66.0.8/24"]},
            {"name":"rmnet0","flags":9,"addrs":["10.1.1.2/30"]},
            {"name":"lo","flags":5,"addrs":["127.0.0.1/8"]},
            {"name":"down0","flags":0,"addrs":["192.168.9.2/24"]}
        ]"#;
        let stored = super::android_lan_interface_json(payload);
        assert_eq!(
            super::private_nets_from_interface_json(&stored),
            vec![("192.168.4.2".parse::<std::net::Ipv4Addr>().unwrap(), 24)]
        );
        assert!(!stored.contains("ipsec0"));
        assert!(!stored.contains("tun0"));
        assert!(!stored.contains("rmnet0"));
    }
}
