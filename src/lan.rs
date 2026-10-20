//! SealDesk LAN discovery uses RustDesk's PeerDiscovery query/reply protocol.
//! Native sessions use signed ephemeral key exchange and the existing split-key stream cipher.
use hbb_common::{
    anyhow::{anyhow, Context},
    bail,
    config::Config,
    log,
    protobuf::Message as _,
    rendezvous_proto::{rendezvous_message, PeerDiscovery, RendezvousMessage},
    sodiumoxide::{
        crypto::{auth, box_, scalarmult::curve25519, secretbox, sign},
        randombytes,
    },
    tcp, tokio, ResultType, Stream,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};
pub(crate) const TCP_PORT: u16 = 21118;
const DISCOVERY_PORT: u16 = 21119;
const DOMAIN: &[u8] = b"sealdesk-lan-v1";
const FRAME_LIMIT: usize = 4096;
/// How long a LAN accept waits for the SealDesk preface before treating the peer as RustDesk.
pub const PREFACE_WAIT: Duration = Duration::from_millis(1500);
pub(crate) const OPTION_RUSTDESK_COMPAT: &str = "sealdesk-rustdesk-compat";
static PREFACE_WAIT_OVERRIDE: Mutex<Option<Duration>> = Mutex::new(None);

fn preface_wait() -> Duration {
    PREFACE_WAIT_OVERRIDE
        .lock()
        .unwrap()
        .unwrap_or(PREFACE_WAIT)
}

#[cfg(test)]
fn set_preface_wait_for_test(wait: Option<Duration>) {
    *PREFACE_WAIT_OVERRIDE.lock().unwrap() = wait;
}

pub(crate) const OPTION_LAN_DISCOVERY: &str = "enable-lan-discovery";
pub(crate) const OPTION_DIRECT_SERVER: &str = "direct-server";
/// Higher identity waits this long so both sides do not open LAN sockets together.
pub(crate) const LAN_RACE_WAIT: Duration = Duration::from_millis(400);
/// LAN should answer quickly. A miss falls back to tailcat instead of stalling it.
pub(crate) const LAN_UPGRADE_TIMEOUT: Duration = Duration::from_millis(1200);

pub(crate) fn rustdesk_compat_enabled() -> bool {
    // Opt-in only. `option2bool` treats a missing value as off for this key.
    hbb_common::config::option2bool(
        OPTION_RUSTDESK_COMPAT,
        &Config::get_option(OPTION_RUSTDESK_COMPAT),
    )
}

pub(crate) fn lan_discovery_enabled() -> bool {
    hbb_common::config::option2bool(
        OPTION_LAN_DISCOVERY,
        &Config::get_option(OPTION_LAN_DISCOVERY),
    )
}

pub(crate) fn direct_server_enabled() -> bool {
    hbb_common::config::option2bool(
        OPTION_DIRECT_SERVER,
        &Config::get_option(OPTION_DIRECT_SERVER),
    )
}

/// TCP and UDP listeners for one serve round. Sharing has to be on, and each
/// switch controls its own socket. Neither listener means bind nothing.
pub(crate) fn serve_listeners(sharing: bool, direct: bool, discovery: bool) -> (bool, bool) {
    (sharing && direct, sharing && discovery)
}

fn compat_notify() -> &'static tokio::sync::Notify {
    static NOTIFY: std::sync::OnceLock<tokio::sync::Notify> = std::sync::OnceLock::new();
    NOTIFY.get_or_init(tokio::sync::Notify::new)
}

pub(crate) fn compat_disabled_notify() -> &'static tokio::sync::Notify {
    compat_notify()
}

pub(crate) fn note_rustdesk_compat(was_enabled: bool) {
    if was_enabled && !rustdesk_compat_enabled() {
        compat_notify().notify_waiters();
    }
}

fn sealdesk_discovery(query: &PeerDiscovery) -> bool {
    query.misc == String::from_utf8_lossy(DOMAIN)
        || hex::decode(&query.id)
            .map(|bytes| bytes.len() == 32)
            .unwrap_or(false)
}

fn answer_discovery_ping(query: &PeerDiscovery) -> bool {
    query.cmd == "ping" && (sealdesk_discovery(query) || rustdesk_compat_enabled())
}
static READY: AtomicBool = AtomicBool::new(false);
static TRUST_LOCK: Mutex<()> = Mutex::new(());
static REVOKED: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());
pub fn generation(key: &str) -> u64 {
    *REVOKED.lock().unwrap().get(key).unwrap_or(&0)
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Peer {
    pub name: String,
    pub address: String,
    pub ip: String,
    pub protocol: String,
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub tailcat: String,
    /// Hex MAC from the discovery hint. Empty when the peer did not prove a tailcat binding.
    #[serde(default)]
    pub mac: String,
    #[serde(default)]
    pub nonce: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Trusted {
    pub key: String,
    pub name: String,
    pub permissions: [bool; 8],
    /// Persistent terminal approval. Missing on older trust files, so it stays off.
    #[serde(default)]
    pub terminal: bool,
}
#[derive(Serialize, Deserialize)]
struct Hint {
    version: u32,
    key: String,
    port: u16,
    /// Discovery nonce covered by `mac`. Not a tailcat credential.
    #[serde(default)]
    nonce: String,
    /// PSK MAC binding `key` and `port` to the tailcat address. The address itself is never sent.
    #[serde(default)]
    mac: String,
}

const TAILCAT_BIND: &[u8] = b"sealdesk-lan-tailcat-bind-v1";

fn advertised_tcp_port(direct: bool) -> u16 {
    if direct {
        TCP_PORT
    } else {
        0
    }
}

fn compat_pong_id(direct: bool, ip: IpAddr) -> Option<String> {
    if !direct {
        return None;
    }
    Some(SocketAddr::new(ip, advertised_tcp_port(true)).to_string())
}

fn binding_message(lan_key_hex: &str, port: u16, nonce: &[u8]) -> Option<Vec<u8>> {
    if port == 0 || nonce.len() != 32 {
        return None;
    }
    let key = hex::decode(lan_key_hex).ok()?;
    if key.len() != sign::PUBLICKEYBYTES {
        return None;
    }
    let mut msg = Vec::with_capacity(TAILCAT_BIND.len() + key.len() + 2 + nonce.len());
    msg.extend_from_slice(TAILCAT_BIND);
    msg.extend_from_slice(&key);
    msg.extend_from_slice(&port.to_be_bytes());
    msg.extend_from_slice(nonce);
    Some(msg)
}

/// MAC over the LAN public key, TCP port, and discovery nonce, keyed by the
/// tailcat address the controller already holds. The address is a credential
/// and stays off the wire.
fn tailcat_binding_mac(tailcat: &str, lan_key_hex: &str, port: u16, nonce: &[u8]) -> String {
    let Some(msg) = binding_message(lan_key_hex, port, nonce) else {
        return String::new();
    };
    if !crate::tailcat::is_addr(tailcat) {
        return String::new();
    }
    let mut state = auth::State::init(tailcat.as_bytes());
    state.update(&msg);
    hex::encode(state.finalize().as_ref())
}

fn tailcat_binding_ok(
    tailcat: &str,
    lan_key_hex: &str,
    port: u16,
    nonce_hex: &str,
    mac_hex: &str,
) -> bool {
    if !crate::tailcat::is_addr(tailcat) {
        return false;
    }
    let (Ok(nonce), Ok(presented)) = (hex::decode(nonce_hex), hex::decode(mac_hex)) else {
        return false;
    };
    let (Some(msg), Some(presented)) = (
        binding_message(lan_key_hex, port, &nonce),
        auth::Tag::from_slice(&presented),
    ) else {
        return false;
    };
    let mut state = auth::State::init(tailcat.as_bytes());
    state.update(&msg);
    state.finalize() == presented
}

fn sealdesk_hint(lan_key: &str, direct: bool, tailcat: &str) -> Hint {
    let port = advertised_tcp_port(direct);
    let nonce = randombytes::randombytes(32);
    let mac = tailcat_binding_mac(tailcat, lan_key, port, &nonce);
    Hint {
        version: 1,
        key: lan_key.to_owned(),
        port,
        nonce: hex::encode(nonce),
        mac,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LanLoginKind {
    Desktop,
    FileTransfer,
    Camera,
    Terminal,
    PortForward,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LanServiceGate {
    pub file: bool,
    pub camera: bool,
    pub terminal: bool,
    pub tunnel: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LanRemembered {
    pub permissions: [bool; 8],
    pub terminal: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LanLoginDecision {
    Continue,
    RejectPermission(&'static str),
    RejectCompatDesktopOnly,
    AllowRemembered([bool; 8]),
    NeedsConfirmation,
}

const FILE_PERMISSION: usize = 3;

/// Encrypted `lan:` follows the same enable-* gates as tailcat. Plaintext
/// compatibility stays desktop-only. Remembered desktop permissions do not
/// cover terminal unless that bit was stored, and it defaults to off.
pub fn decide_lan_login(
    encrypted: bool,
    legacy: bool,
    kind: LanLoginKind,
    gate: LanServiceGate,
    remembered: Option<LanRemembered>,
) -> LanLoginDecision {
    if !encrypted && !legacy {
        return LanLoginDecision::Continue;
    }
    if let Some(message) = service_denial(kind, gate) {
        return LanLoginDecision::RejectPermission(message);
    }
    if !encrypted {
        return if kind == LanLoginKind::Desktop {
            LanLoginDecision::Continue
        } else {
            LanLoginDecision::RejectCompatDesktopOnly
        };
    }
    match kind {
        LanLoginKind::Desktop => match remembered {
            Some(saved) => LanLoginDecision::AllowRemembered(saved.permissions),
            None => LanLoginDecision::NeedsConfirmation,
        },
        LanLoginKind::FileTransfer => match remembered {
            Some(saved) if saved.permissions[FILE_PERMISSION] => {
                LanLoginDecision::AllowRemembered(saved.permissions)
            }
            _ => LanLoginDecision::NeedsConfirmation,
        },
        LanLoginKind::Terminal => match remembered {
            Some(saved) if saved.terminal => LanLoginDecision::AllowRemembered(saved.permissions),
            _ => LanLoginDecision::NeedsConfirmation,
        },
        LanLoginKind::Camera | LanLoginKind::PortForward => LanLoginDecision::NeedsConfirmation,
    }
}

fn service_denial(kind: LanLoginKind, gate: LanServiceGate) -> Option<&'static str> {
    match kind {
        LanLoginKind::FileTransfer if !gate.file => Some("No permission of file transfer"),
        LanLoginKind::Camera if !gate.camera => Some("No permission of viewing camera"),
        LanLoginKind::Terminal if !gate.terminal => Some("No permission of terminal"),
        LanLoginKind::PortForward if !gate.tunnel => Some("No permission of IP tunneling"),
        _ => None,
    }
}
#[derive(Serialize, Deserialize)]
struct Hello {
    key: Vec<u8>,
    ephemeral: Vec<u8>,
    nonce: Vec<u8>,
    signature: Vec<u8>,
}
fn identity() -> ResultType<(sign::PublicKey, sign::SecretKey)> {
    let (secret, public) = Config::get_key_pair();
    Ok((
        sign::PublicKey::from_slice(&public).context("LAN identity public key unavailable")?,
        sign::SecretKey::from_slice(&secret).context("LAN identity secret key unavailable")?,
    ))
}
pub fn is_addr(value: &str) -> bool {
    parse_addr(value).is_ok()
}
fn parse_addr(value: &str) -> ResultType<(SocketAddr, sign::PublicKey)> {
    let (addr, key) = value
        .strip_prefix("lan:")
        .context("not a LAN address")?
        .split_once('/')
        .context("LAN identity missing")?;
    let addr: SocketAddr = addr.parse()?;
    if addr.port() == 0 || addr.ip().is_unspecified() || addr.ip().is_multicast() {
        bail!("invalid LAN endpoint");
    }
    let bytes = hex::decode(key)?;
    Ok((
        addr,
        sign::PublicKey::from_slice(&bytes).context("invalid LAN identity")?,
    ))
}
fn body(hello: &Hello, role: u8, prior: &[u8]) -> Vec<u8> {
    let mut result = Vec::from(DOMAIN);
    result.push(role);
    result.extend_from_slice(prior);
    result.extend_from_slice(&hello.key);
    result.extend_from_slice(&hello.ephemeral);
    result.extend_from_slice(&hello.nonce);
    result
}
fn hello(role: u8, prior: &[u8]) -> ResultType<(Hello, box_::SecretKey)> {
    let (public, secret) = identity()?;
    let (ephemeral, private) = box_::gen_keypair();
    let mut h = Hello {
        key: public.0.to_vec(),
        ephemeral: ephemeral.0.to_vec(),
        nonce: randombytes::randombytes(32),
        signature: vec![],
    };
    h.signature = sign::sign_detached(&body(&h, role, prior), &secret)
        .to_bytes()
        .to_vec();
    Ok((h, private))
}
fn verify(h: &Hello, role: u8, prior: &[u8]) -> ResultType<sign::PublicKey> {
    if h.nonce.len() != 32 || h.ephemeral.len() != 32 {
        bail!("invalid LAN handshake");
    }
    let public = sign::PublicKey::from_slice(&h.key).context("invalid LAN public key")?;
    let signature =
        sign::Signature::from_bytes(&h.signature).map_err(|_| anyhow!("invalid LAN signature"))?;
    if !sign::verify_detached(&signature, &body(h, role, prior), &public) {
        bail!("LAN signature verification failed");
    }
    Ok(public)
}
async fn receive(stream: &mut Stream) -> ResultType<Vec<u8>> {
    Ok(stream
        .next()
        .await
        .context("LAN handshake closed")??
        .to_vec())
}
fn encrypt(
    stream: &mut Stream,
    local: &Hello,
    remote: &Hello,
    private: &box_::SecretKey,
    initiator: bool,
) -> ResultType<()> {
    let peer = box_::PublicKey::from_slice(&remote.ephemeral).context("invalid ephemeral key")?;
    let scalar =
        curve25519::Scalar::from_slice(&private.0).context("invalid ephemeral private key")?;
    let point = curve25519::GroupElement(peer.0);
    let shared = curve25519::scalarmult(&scalar, &point)
        .map_err(|_| anyhow!("invalid ephemeral public key"))?;
    let key = secretbox::Key(shared.0);
    let (client, server) = if initiator {
        (&local.ephemeral, &remote.ephemeral)
    } else {
        (&remote.ephemeral, &local.ephemeral)
    };
    stream.set_negotiated_key(
        key,
        initiator,
        &tcp::KxTranscript {
            initiator_pk: client,
            responder_pk: server,
            advertised: 1,
            picked: 1,
        },
    )
}
pub async fn accept_shared(stream: &mut Stream) -> ResultType<Option<String>> {
    stream.set_max_packet_length(FRAME_LIMIT);
    // Original RustDesk waits for Hash; SealDesk declares encryption before any login.
    // A delayed/malformed SealDesk preface can fail the connection, never downgrade its client.
    match tokio::time::timeout(preface_wait(), stream.next()).await {
        Err(_) => {
            if rustdesk_compat_enabled() {
                return Ok(None);
            }
            hbb_common::throttled_log!(
                Duration::from_secs(30),
                info,
                "rejected RustDesk compatible LAN connection"
            );
            bail!("RustDesk compatible connections are disabled");
        }
        Ok(Some(Ok(bytes))) if bytes.as_ref() == DOMAIN => {}
        _ => bail!("Invalid LAN protocol preface"),
    }
    accept(stream).await.map(Some)
}
pub async fn accept(stream: &mut Stream) -> ResultType<String> {
    stream.set_max_packet_length(FRAME_LIMIT);
    let (our, private) = hello(0, &[])?;
    let prior = serde_json::to_vec(&our)?;
    stream.0.send_raw(prior.clone()).await?;
    let remote: Hello = serde_json::from_slice(&receive(stream).await?)?;
    let key = verify(&remote, 1, &prior)?;
    encrypt(stream, &our, &remote, &private, false)?;
    stream.0.send_raw(DOMAIN.to_vec()).await?;
    if receive(stream).await? != DOMAIN {
        bail!("LAN key confirmation failed");
    }
    Ok(hex::encode(key.0))
}
pub async fn connect(address: &str) -> ResultType<Stream> {
    let (addr, expected) = parse_addr(address)?;
    let socket = tokio::net::TcpStream::connect(addr).await?;
    socket.set_nodelay(true)?;
    let local = socket.local_addr()?;
    let mut stream = Stream::from(socket, local);
    stream.set_max_packet_length(FRAME_LIMIT);
    stream.0.send_raw(DOMAIN.to_vec()).await?;
    let prior = receive(&mut stream).await?;
    let remote: Hello = serde_json::from_slice(&prior)?;
    if verify(&remote, 0, &[])? != expected {
        bail!("LAN host identity changed");
    }
    let (our, private) = hello(1, &prior)?;
    stream.0.send_raw(serde_json::to_vec(&our)?).await?;
    encrypt(&mut stream, &our, &remote, &private, true)?;
    if receive(&mut stream).await? != DOMAIN {
        bail!("LAN key confirmation failed");
    }
    stream.0.send_raw(DOMAIN.to_vec()).await?;
    stream.set_max_packet_length(usize::MAX);
    Ok(stream)
}
fn trust_path() -> std::path::PathBuf {
    Config::path("lan-trusted.json")
}
fn load_trusted() -> Vec<Trusted> {
    std::fs::read(trust_path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}
pub fn trusted_json() -> String {
    let _lock = TRUST_LOCK.lock().unwrap();
    serde_json::to_string(&load_trusted()).unwrap_or_else(|_| "[]".into())
}
pub fn trusted(key: &str) -> Option<Trusted> {
    let _lock = TRUST_LOCK.lock().unwrap();
    load_trusted().into_iter().find(|p| p.key == key)
}
fn save_trusted(peers: &[Trusted]) -> ResultType<()> {
    let path = trust_path();
    let parent = path.parent().context("missing LAN trust directory")?;
    std::fs::create_dir_all(parent)?;
    let temp = path.with_extension("tmp");
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp)?;
    file.write_all(&serde_json::to_vec(peers)?)?;
    file.sync_all()?;
    std::fs::rename(temp, path)?;
    Ok(())
}
pub fn remember(
    key: &str,
    name: &str,
    permissions: [bool; 8],
    expected_generation: u64,
    terminal: bool,
) -> ResultType<()> {
    let _lock = TRUST_LOCK.lock().unwrap();
    if generation(key) != expected_generation {
        bail!("LAN authorization was revoked");
    }
    if hex::decode(key)?.len() != 32 {
        bail!("invalid trusted LAN identity");
    }
    let mut peers = load_trusted();
    peers.retain(|p| p.key != key);
    if peers.len() >= 128 {
        bail!("too many trusted LAN controllers");
    }
    peers.push(Trusted {
        key: key.to_owned(),
        name: name.chars().take(128).collect(),
        permissions,
        terminal,
    });
    save_trusted(&peers)
}
pub fn forget(key: &str) -> ResultType<()> {
    let _lock = TRUST_LOCK.lock().unwrap();
    let mut peers = load_trusted();
    peers.retain(|p| p.key != key);
    save_trusted(&peers)?;
    *REVOKED.lock().unwrap().entry(key.to_owned()).or_default() += 1;
    crate::server::revoke_lan(key);
    Ok(())
}
pub fn compat_addresses_json() -> String {
    let addresses: Vec<String> =
        if ready() && crate::tailcat::host::enabled() && rustdesk_compat_enabled() {
            let mut addresses: Vec<String> = default_net::get_interfaces()
                .into_iter()
                .flat_map(|iface| iface.ipv4)
                .filter(|ip| ip.addr.is_private())
                .map(|ip| SocketAddr::new(ip.addr.into(), TCP_PORT).to_string())
                .collect();
            // Android denies netlink, so default_net is empty. The UI already
            // pushed interface addresses for Tailcat; reuse those for the share list.
            #[cfg(target_os = "android")]
            if addresses.is_empty() {
                addresses = crate::tailcat::android_private_ipv4()
                    .into_iter()
                    .map(|ip| SocketAddr::new(ip.into(), TCP_PORT).to_string())
                    .collect();
            }
            addresses
        } else {
            vec![]
        };
    serde_json::to_string(&addresses).unwrap_or_else(|_| "[]".into())
}
pub fn ready() -> bool {
    READY.load(Ordering::SeqCst)
}
pub async fn serve(server: crate::server::ServerPtr) {
    loop {
        let (tcp, udp) = serve_listeners(
            crate::tailcat::host::enabled(),
            direct_server_enabled(),
            lan_discovery_enabled(),
        );
        if !tcp && !udp {
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        }
        if let Err(error) = serve_round(server.clone()).await {
            log::warn!("LAN sharing stopped: {error}");
        }
        READY.store(false, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
async fn serve_round(server: crate::server::ServerPtr) -> ResultType<()> {
    let direct = direct_server_enabled();
    let discovery = lan_discovery_enabled();
    if !crate::tailcat::host::enabled() || (!direct && !discovery) {
        return Ok(());
    }
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let mut tasks = Vec::new();
    if direct {
        let addrs = current_tcp_bind_addrs();
        if addrs.is_empty() {
            bail!("LAN direct has no private address to bind");
        }
        let mut bound = Vec::new();
        for addr in addrs {
            if addr.ip().is_unspecified() {
                bail!("refusing to bind LAN TCP to {}", addr.ip());
            }
            match tcp::new_listener(addr, true).await {
                Ok(listener) => bound.push(listener),
                Err(error) => log::warn!("LAN bind {addr} skipped: {error}"),
            }
        }
        if bound.is_empty() {
            bail!("LAN direct bind failed");
        }
        log::info!(
            "LAN encrypted sharing listening on {}",
            bound
                .iter()
                .filter_map(|listener| listener.local_addr().ok())
                .map(|addr| addr.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
        for listener in bound {
            let tx = tx.clone();
            tasks.push(tokio::spawn(async move {
                loop {
                    match listener.accept().await {
                        Ok(pair) => {
                            if tx.send(pair).await.is_err() {
                                break;
                            }
                        }
                        Err(error) => {
                            log::warn!("LAN accept failed: {error}");
                            break;
                        }
                    }
                }
            }));
        }
        READY.store(true, Ordering::SeqCst);
    }
    let _abort = AbortOnDrop(tasks);
    let udp = if discovery {
        Some(tokio::net::UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED, DISCOVERY_PORT)).await?)
    } else {
        None
    };
    let mut buf = [0u8; 2048];
    let mut last = Instant::now();
    let mut replies = 0;
    loop {
        if !crate::tailcat::host::enabled()
            || direct_server_enabled() != direct
            || lan_discovery_enabled() != discovery
        {
            return Ok(());
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(100)) => {},
            incoming = rx.recv(), if direct => {
                let Some((socket, addr)) = incoming else { return Ok(()); };
                let local = socket.local_addr()?;
                let server = server.clone();
                tokio::spawn(async move {
                    if let Err(e) = crate::server::create_tcp_connection(server, Stream::from(socket, local), addr, false, crate::server::ConnectionMeta { via_lan: true, ..Default::default() }).await {
                        log::trace!("LAN connection ended: {e}");
                    }
                });
            },
            incoming = recv_discovery(udp.as_ref(), &mut buf), if discovery => {
                let Ok((n, addr)) = incoming else { continue; };
                if !private(addr.ip()) { continue; }
                if last.elapsed() >= Duration::from_secs(1) { replies = 0; last = Instant::now(); }
                if replies >= 20 { continue; }
                let Ok(msg) = RendezvousMessage::parse_from_bytes(&buf[..n]) else { continue; };
                let Some(rendezvous_message::Union::PeerDiscovery(query)) = msg.union else { continue; };
                if !answer_discovery_ping(&query) { continue; }
                let (key, _) = identity()?;
                let key = hex::encode(key.0);
                if query.id == key { continue; }
                let mut reply = RendezvousMessage::new();
                let sealdesk_ping = sealdesk_discovery(&query);
                let (id, misc) = if sealdesk_ping {
                    let tailcat = Config::get_option(crate::tailcat::OPTION_ADDR);
                    (key.clone(), serde_json::to_string(&sealdesk_hint(&key, direct, &tailcat))?)
                } else if !direct {
                    continue;
                } else {
                    let probe = UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED, 0))?;
                    probe.connect(addr)?;
                    let Some(id) = compat_pong_id(direct, probe.local_addr()?.ip()) else {
                        continue;
                    };
                    (id, String::new())
                };
                let platform = discovery_platform();
                reply.set_peer_discovery(PeerDiscovery { cmd: "pong".into(), id, hostname: crate::whoami_hostname(), platform, misc, ..Default::default() });
                if let Some(sock) = udp.as_ref() {
                    sock.send_to(&reply.write_to_bytes()?, addr).await?;
                    replies += 1;
                }
            }
        }
    }
}

async fn recv_discovery(
    udp: Option<&tokio::net::UdpSocket>,
    buf: &mut [u8],
) -> std::io::Result<(usize, SocketAddr)> {
    match udp {
        Some(sock) => sock.recv_from(buf).await,
        None => std::future::pending().await,
    }
}

fn discovery_platform() -> String {
    #[cfg(target_os = "android")]
    {
        return "Android".to_owned();
    }
    #[cfg(target_os = "ios")]
    {
        return "iOS".to_owned();
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        hbb_common::whoami::platform().to_string()
    }
}

struct AbortOnDrop(Vec<tokio::task::JoinHandle<()>>);
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}

/// Private IPv4 (10/8, 172.16/12, 192.168/16) and IPv6 ULA only.
/// Unspecified, loopback, link-local, and public addresses are dropped.
/// An empty enumeration uses `fallback_v4` (the Android interface list) and
/// still never adds 0.0.0.0 or ::.
pub(crate) fn tcp_bind_addrs(enumerated: &[IpAddr], fallback_v4: &[Ipv4Addr]) -> Vec<SocketAddr> {
    let mut selected = filter_bind_addrs(enumerated);
    if selected.is_empty() && !fallback_v4.is_empty() {
        let fallback: Vec<IpAddr> = fallback_v4.iter().copied().map(IpAddr::V4).collect();
        selected = filter_bind_addrs(&fallback);
    }
    selected.sort();
    selected.dedup();
    selected
}

fn filter_bind_addrs(addrs: &[IpAddr]) -> Vec<SocketAddr> {
    let mut out = Vec::new();
    for ip in addrs {
        if ip.is_unspecified() {
            continue;
        }
        let keep = match ip {
            IpAddr::V4(ip) => is_lan_bind_v4(*ip),
            IpAddr::V6(ip) => is_ula(*ip),
        };
        if keep {
            out.push(SocketAddr::new(*ip, TCP_PORT));
        }
    }
    out
}

pub(crate) fn is_lan_bind_v4(ip: Ipv4Addr) -> bool {
    ip.is_private()
}

fn is_ula(ip: Ipv6Addr) -> bool {
    ip.is_unique_local()
}

fn current_tcp_bind_addrs() -> Vec<SocketAddr> {
    let mut enumerated = Vec::new();
    for iface in default_net::get_interfaces() {
        for ip in iface.ipv4 {
            enumerated.push(IpAddr::V4(ip.addr));
        }
        for ip in iface.ipv6 {
            enumerated.push(IpAddr::V6(ip.addr));
        }
    }
    #[cfg(target_os = "android")]
    let fallback = crate::tailcat::android_private_ipv4();
    #[cfg(not(target_os = "android"))]
    let fallback = Vec::new();
    tcp_bind_addrs(&enumerated, &fallback)
}
fn private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v.is_private() || v.is_loopback(),
        IpAddr::V6(v) => v.is_unique_local() || v.is_loopback(),
    }
}
pub fn discover_json() -> String {
    match discover() {
        Ok(peers) => serde_json::to_string(&peers).unwrap_or_else(|_| "[]".into()),
        Err(e) => {
            log::trace!("LAN discovery: {e}");
            "[]".into()
        }
    }
}
fn discover() -> ResultType<Vec<Peer>> {
    let (key, _) = identity()?;
    let own = hex::encode(key.0);
    let mut query = RendezvousMessage::new();
    query.set_peer_discovery(PeerDiscovery {
        cmd: "ping".into(),
        id: own.clone(),
        misc: String::from_utf8_lossy(DOMAIN).into_owned(),
        ..Default::default()
    });
    let bytes = query.write_to_bytes()?;
    let mut sockets = vec![];
    for iface in default_net::get_interfaces() {
        for addr in iface.ipv4 {
            if !addr.addr.is_private() {
                continue;
            }
            let socket = UdpSocket::bind((addr.addr, 0));
            let Ok(socket) = socket else {
                continue;
            };
            if socket.set_broadcast(true).is_err() || socket.set_nonblocking(true).is_err() {
                continue;
            }
            let broadcast =
                std::net::Ipv4Addr::from(u32::from(addr.addr) | !u32::from(addr.netmask));
            if socket.send_to(&bytes, (broadcast, DISCOVERY_PORT)).is_ok() {
                sockets.push(socket);
            }
        }
    }
    // Like upstream discovery, Android can query even when interface enumeration is restricted.
    #[cfg(target_os = "android")]
    if let Ok(socket) = UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED, 0)) {
        if socket.set_broadcast(true).is_ok()
            && socket.set_nonblocking(true).is_ok()
            && socket
                .send_to(&bytes, (std::net::Ipv4Addr::BROADCAST, DISCOVERY_PORT))
                .is_ok()
        {
            sockets.push(socket);
        }
    }
    let mut peers = BTreeMap::new();
    let start = Instant::now();
    let mut buf = [0u8; 2048];
    while start.elapsed() < Duration::from_millis(250) {
        for socket in &sockets {
            if let Ok((n, addr)) = socket.recv_from(&mut buf) {
                let Ok(msg) = RendezvousMessage::parse_from_bytes(&buf[..n]) else {
                    continue;
                };
                let Some(rendezvous_message::Union::PeerDiscovery(reply)) = msg.union else {
                    continue;
                };
                if let Some((id, peer)) = discovered_peer(&reply, addr, &own) {
                    peers.insert(id, peer);
                }
            }
        }
        if peers.len() >= 64 {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(peers.into_values().collect())
}

fn discovered_peer(reply: &PeerDiscovery, addr: SocketAddr, own: &str) -> Option<(String, Peer)> {
    if reply.cmd != "pong" || !private(addr.ip()) {
        return None;
    }
    let (id, address, protocol, mac, nonce) = if reply.misc.is_empty() {
        if !crate::common::is_valid_untrusted_peer_id(&reply.id) {
            return None;
        }
        let endpoint = SocketAddr::new(addr.ip(), TCP_PORT);
        (
            format!("rustdesk:{endpoint}"),
            endpoint.to_string(),
            "rustdesk",
            String::new(),
            String::new(),
        )
    } else {
        // A malformed SealDesk identity must never fall back to plaintext RustDesk.
        // `tailcat` inside misc is ignored: the address is a credential and is not a binding.
        let hint = serde_json::from_str::<Hint>(&reply.misc).ok()?;
        if hint.version != 1
            || hint.key == own
            || hint.port == 0
            || hex::decode(&hint.key)
                .map(|b| b.len() != 32)
                .unwrap_or(true)
        {
            return None;
        }
        let endpoint = SocketAddr::new(addr.ip(), hint.port);
        (
            hint.key.clone(),
            format!("lan:{endpoint}/{}", hint.key),
            "sealdesk",
            hint.mac,
            hint.nonce,
        )
    };
    Some((
        id,
        Peer {
            name: reply
                .hostname
                .chars()
                .filter(|c| !c.is_control())
                .take(128)
                .collect(),
            ip: addr.ip().to_string(),
            address,
            protocol: protocol.to_owned(),
            platform: reply
                .platform
                .chars()
                .filter(|c| !c.is_control())
                .take(64)
                .collect(),
            tailcat: String::new(),
            mac,
            nonce,
        },
    ))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalNet {
    pub addr: IpAddr,
    pub prefix: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanUpgrade {
    pub address: String,
    pub remote_key: String,
    pub delay: Duration,
}

pub(crate) fn local_identity_hex() -> String {
    identity()
        .map(|(public, _)| hex::encode(public.0))
        .unwrap_or_default()
}

pub(crate) fn local_segments() -> Vec<LocalNet> {
    let mut nets = Vec::new();
    for iface in default_net::get_interfaces() {
        for ip in iface.ipv4 {
            if is_lan_bind_v4(ip.addr) && ip.prefix_len > 0 && ip.prefix_len <= 32 {
                nets.push(LocalNet {
                    addr: IpAddr::V4(ip.addr),
                    prefix: ip.prefix_len,
                });
            }
        }
        for ip in iface.ipv6 {
            if is_ula(ip.addr) && ip.prefix_len > 0 && ip.prefix_len <= 128 {
                nets.push(LocalNet {
                    addr: IpAddr::V6(ip.addr),
                    prefix: ip.prefix_len,
                });
            }
        }
    }
    #[cfg(target_os = "android")]
    if !nets.iter().any(|net| net.addr.is_ipv4()) {
        for (ip, prefix) in crate::tailcat::android_private_nets() {
            if is_lan_bind_v4(ip) && prefix > 0 && prefix <= 32 {
                nets.push(LocalNet {
                    addr: IpAddr::V4(ip),
                    prefix,
                });
            }
        }
    }
    nets
}

pub(crate) fn lan_dial_delay(local_key: &str, remote_key: &str) -> Duration {
    if local_key < remote_key {
        Duration::ZERO
    } else {
        LAN_RACE_WAIT
    }
}

/// Any LAN failure, including a signature or identity mismatch, uses tailcat.
pub(crate) fn transport_after_lan_error(message: &str) -> &'static str {
    let _ = message;
    "tailcat"
}

pub(crate) fn is_encrypted_lan_channel(address: &str) -> bool {
    parse_addr(address).is_ok()
}

/// Known `tc` peers on the same private subnet upgrade to `lan:` only when the
/// discovery MAC binds that `tc` to the advertised LAN key, port, and nonce.
/// A forged or unsigned association is ignored so the caller stays on tailcat.
/// A plaintext RustDesk address is never treated as that encrypted channel.
pub(crate) fn plan_lan_upgrade(
    peer: &str,
    peers: &[Peer],
    locals: &[LocalNet],
    local_key: &str,
) -> Option<LanUpgrade> {
    if !crate::tailcat::is_addr(peer) {
        return None;
    }
    let found = peers.iter().find(|candidate| {
        candidate.protocol == "sealdesk"
            && is_encrypted_lan_channel(&candidate.address)
            && candidate
                .ip
                .parse::<IpAddr>()
                .ok()
                .is_some_and(|ip| same_segment(ip, locals))
            && lan_upgrade_authenticated(peer, candidate)
    })?;
    let remote_key = found.address.rsplit_once('/')?.1.to_owned();
    if remote_key.len() != 64 {
        return None;
    }
    Some(LanUpgrade {
        address: found.address.clone(),
        delay: lan_dial_delay(local_key, &remote_key),
        remote_key,
    })
}

fn lan_upgrade_authenticated(tailcat: &str, candidate: &Peer) -> bool {
    let Ok((addr, key)) = parse_addr(&candidate.address) else {
        return false;
    };
    tailcat_binding_ok(
        tailcat,
        &hex::encode(key.0),
        addr.port(),
        &candidate.nonce,
        &candidate.mac,
    )
}

fn same_segment(peer: IpAddr, locals: &[LocalNet]) -> bool {
    locals.iter().any(|net| match (net.addr, peer) {
        (IpAddr::V4(local), IpAddr::V4(remote))
            if is_lan_bind_v4(local) && is_lan_bind_v4(remote) =>
        {
            ipv4_same(local, remote, net.prefix)
        }
        (IpAddr::V6(local), IpAddr::V6(remote)) if is_ula(local) && is_ula(remote) => {
            ipv6_same(local, remote, net.prefix)
        }
        _ => false,
    })
}

fn ipv4_same(left: Ipv4Addr, right: Ipv4Addr, prefix: u8) -> bool {
    if prefix == 0 || prefix > 32 {
        return false;
    }
    let shift = 32 - prefix;
    let mask = u32::MAX << shift;
    (u32::from(left) & mask) == (u32::from(right) & mask)
}

fn ipv6_same(left: Ipv6Addr, right: Ipv6Addr, prefix: u8) -> bool {
    if prefix == 0 || prefix > 128 {
        return false;
    }
    let shift = 128 - prefix;
    let mask = u128::MAX << shift;
    (u128::from(left) & mask) == (u128::from(right) & mask)
}

pub(crate) async fn prepare_tailcat_lan_upgrade(peer: &str) -> Option<LanUpgrade> {
    if !lan_discovery_enabled() || !crate::tailcat::is_addr(peer) {
        return None;
    }
    let peer = peer.to_owned();
    let peers = tokio::task::spawn_blocking(|| discover().unwrap_or_default())
        .await
        .unwrap_or_default();
    plan_lan_upgrade(&peer, &peers, &local_segments(), &local_identity_hex())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn direct_address_support_does_not_expand_broadcast_discovery() {
        let reply = PeerDiscovery {
            cmd: "pong".into(),
            id: "123456789".into(),
            ..Default::default()
        };
        for address in ["100.64.1.7:21119", "169.254.1.7:21119", "8.8.8.8:21119"] {
            assert!(discovered_peer(&reply, address.parse().unwrap(), "own").is_none());
        }
    }
    #[test]
    fn original_rustdesk_discovery_uses_source_ip_and_default_port() {
        let mut reply = PeerDiscovery {
            cmd: "pong".into(),
            id: "123456789".into(),
            hostname: "RustDesk host".into(),
            ..Default::default()
        };
        let source = "192.168.1.7:45678".parse().unwrap();
        let (_, peer) = discovered_peer(&reply, source, "own").unwrap();
        assert_eq!(peer.address, "192.168.1.7:21118");
        assert_eq!(peer.protocol, "rustdesk");
        reply.id = "bad/id".into();
        assert!(discovered_peer(&reply, source, "own").is_none());
        reply.id = "123456789".into();
        assert!(discovered_peer(&reply, "8.8.8.8:1234".parse().unwrap(), "own").is_none());
    }
    #[test]
    fn sealdesk_discovery_preserves_identity_and_never_downgrades() {
        let key = hex::encode([7u8; 32]);
        let mut reply = PeerDiscovery {
            cmd: "pong".into(),
            id: key.clone(),
            misc: serde_json::to_string(&Hint {
                version: 1,
                key: key.clone(),
                port: TCP_PORT,
                nonce: String::new(),
                mac: String::new(),
            })
            .unwrap(),
            ..Default::default()
        };
        let source = "192.168.1.8:1234".parse().unwrap();
        let (_, peer) = discovered_peer(&reply, source, "own").unwrap();
        assert_eq!(peer.address, format!("lan:192.168.1.8:{TCP_PORT}/{key}"));
        assert_eq!(peer.protocol, "sealdesk");
        assert!(discovered_peer(&reply, source, &key).is_none());
        for misc in ["{", "{}", r#"{"version":2,"key":"bad","port":21118}"#] {
            reply.misc = misc.into();
            assert!(discovered_peer(&reply, source, "own").is_none());
        }
    }
    #[test]
    fn queued_remember_cannot_restore_a_revoked_authorization() {
        let key = hex::encode([0x91; 32]);
        REVOKED.lock().unwrap().insert(key.clone(), 1);
        let error = remember(&key, "old approval", [true; 8], 0, false).unwrap_err();
        assert!(error.to_string().contains("revoked"));
        REVOKED.lock().unwrap().remove(&key);
    }
    #[test]
    fn signatures_bind_roles_challenges_and_ephemeral_keys() {
        let (server, _) = hello(0, &[]).unwrap();
        let prior = serde_json::to_vec(&server).unwrap();
        let (mut client, _) = hello(1, &prior).unwrap();
        assert!(verify(&client, 1, &prior).is_ok());
        assert!(verify(&client, 0, &prior).is_err());
        let (other, _) = hello(0, &[]).unwrap();
        assert!(verify(&client, 1, &serde_json::to_vec(&other).unwrap()).is_err());
        client.ephemeral[0] ^= 1;
        assert!(verify(&client, 1, &prior).is_err());
    }
    #[tokio::test]
    async fn native_transport_is_encrypted_and_rejects_changed_host_identity() {
        let (_lock, _reset) = lan_test_env();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (public, _) = identity().unwrap();
        let expected = hex::encode(public.0);
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let local = socket.local_addr().unwrap();
            let mut stream = Stream::from(socket, local);
            assert_eq!(accept_shared(&mut stream).await.unwrap(), Some(expected));
            assert!(stream.is_secured());
            assert_eq!(receive(&mut stream).await.unwrap(), b"desktop payload");
            stream.send_raw(b"encrypted reply".to_vec()).await.unwrap();
        });
        let mut client = connect(&format!("lan:{addr}/{}", hex::encode(public.0)))
            .await
            .unwrap();
        assert!(client.is_secured());
        client.send_raw(b"desktop payload".to_vec()).await.unwrap();
        assert_eq!(receive(&mut client).await.unwrap(), b"encrypted reply");
        server.await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let local = socket.local_addr().unwrap();
            assert!(accept_shared(&mut Stream::from(socket, local))
                .await
                .is_err());
        });
        let (wrong, _) = sign::gen_keypair();
        assert!(connect(&format!("lan:{addr}/{}", hex::encode(wrong.0)))
            .await
            .is_err());
        server.await.unwrap();
    }
    #[tokio::test]
    async fn shared_port_accepts_legacy_waiters_but_rejects_bad_encryption_prefaces() {
        let (_lock, _reset) = lan_test_env();
        Config::set_option(OPTION_RUSTDESK_COMPAT.to_owned(), "Y".into());
        set_preface_wait_for_test(Some(Duration::from_millis(30)));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let socket = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (accepted, _) = listener.accept().await.unwrap();
        let local = accepted.local_addr().unwrap();
        let mut server = Stream::from(accepted, local);
        assert!(accept_shared(&mut server).await.unwrap().is_none());
        assert!(!server.is_secured());
        let local = socket.local_addr().unwrap();
        let mut client = Stream::from(socket, local);
        client
            .send_raw(b"bad encryption preface".to_vec())
            .await
            .unwrap();
        assert!(accept_shared(&mut server).await.is_err());
    }
    #[tokio::test]
    async fn low_order_ephemeral_key_is_rejected() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let socket = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let local = socket.local_addr().unwrap();
        let mut stream = Stream::from(socket, local);
        let (ours, private) = hello(0, &[]).unwrap();
        let (mut peer, _) = hello(1, &[]).unwrap();
        peer.ephemeral = vec![0; 32];
        assert!(encrypt(&mut stream, &ours, &peer, &private, false).is_err());
    }

    fn lan_test_env() -> (std::sync::MutexGuard<'static, ()>, impl Drop) {
        static LAN_TESTS: Mutex<()> = Mutex::new(());
        let guard = LAN_TESTS.lock().unwrap();
        set_preface_wait_for_test(None);
        Config::set_option(OPTION_RUSTDESK_COMPAT.to_owned(), String::new());
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                set_preface_wait_for_test(None);
                Config::set_option(OPTION_RUSTDESK_COMPAT.to_owned(), String::new());
            }
        }
        (guard, Reset)
    }

    #[test]
    fn preface_wait_defaults_to_1500ms_and_can_be_shortened() {
        let (_lock, _reset) = lan_test_env();
        assert_eq!(PREFACE_WAIT, Duration::from_millis(1500));
        assert_eq!(preface_wait(), Duration::from_millis(1500));
        set_preface_wait_for_test(Some(Duration::from_millis(5)));
        assert_eq!(preface_wait(), Duration::from_millis(5));
    }

    #[test]
    fn rustdesk_compat_defaults_off_until_explicitly_enabled() {
        let (_lock, _reset) = lan_test_env();
        assert!(!rustdesk_compat_enabled());
        Config::set_option(OPTION_RUSTDESK_COMPAT.to_owned(), "N".into());
        assert!(!rustdesk_compat_enabled());
        Config::set_option(OPTION_RUSTDESK_COMPAT.to_owned(), "Y".into());
        assert!(rustdesk_compat_enabled());
    }

    #[test]
    fn rustdesk_pings_are_not_answered_when_compat_is_off() {
        let (_lock, _reset) = lan_test_env();
        let sealdesk = PeerDiscovery {
            cmd: "ping".into(),
            id: hex::encode([9u8; 32]),
            misc: String::from_utf8_lossy(DOMAIN).into_owned(),
            ..Default::default()
        };
        let rustdesk = PeerDiscovery {
            cmd: "ping".into(),
            id: "123456789".into(),
            ..Default::default()
        };
        assert!(answer_discovery_ping(&sealdesk));
        assert!(!answer_discovery_ping(&rustdesk));
        Config::set_option(OPTION_RUSTDESK_COMPAT.to_owned(), "Y".into());
        assert!(answer_discovery_ping(&sealdesk));
        assert!(answer_discovery_ping(&rustdesk));
        Config::set_option(OPTION_RUSTDESK_COMPAT.to_owned(), "N".into());
        assert!(answer_discovery_ping(&sealdesk));
        assert!(!answer_discovery_ping(&rustdesk));
        assert!(!answer_discovery_ping(&PeerDiscovery {
            cmd: "pong".into(),
            ..sealdesk
        }));
    }

    #[test]
    fn disabling_compat_notifies_waiters_immediately() {
        let (_lock, _reset) = lan_test_env();
        let waiting = compat_disabled_notify().notified();
        let mut waiting = std::pin::pin!(waiting);
        let waker = noop_waker();
        let mut cx = std::task::Context::from_waker(&waker);
        assert!(
            std::future::Future::poll(waiting.as_mut(), &mut cx).is_pending(),
            "waiter must be registered before the switch flips"
        );
        Config::set_option(OPTION_RUSTDESK_COMPAT.to_owned(), "N".into());
        note_rustdesk_compat(true);
        assert!(std::future::Future::poll(waiting.as_mut(), &mut cx).is_ready());
    }

    fn noop_waker() -> std::task::Waker {
        use std::task::{RawWaker, RawWakerVTable, Waker};
        unsafe fn clone(p: *const ()) -> RawWaker {
            RawWaker::new(p, &VTABLE)
        }
        unsafe fn wake(_: *const ()) {}
        unsafe fn wake_by_ref(_: *const ()) {}
        unsafe fn drop(_: *const ()) {}
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);
        unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
    }

    async fn connect_preface_after(addr: std::net::SocketAddr, delay: Duration) -> Stream {
        let socket = tokio::net::TcpStream::connect(addr).await.unwrap();
        socket.set_nodelay(true).unwrap();
        let local = socket.local_addr().unwrap();
        let mut stream = Stream::from(socket, local);
        stream.set_max_packet_length(FRAME_LIMIT);
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        stream.0.send_raw(DOMAIN.to_vec()).await.unwrap();
        let prior = receive(&mut stream).await.unwrap();
        let remote: Hello = serde_json::from_slice(&prior).unwrap();
        let (public, _) = identity().unwrap();
        assert_eq!(verify(&remote, 0, &[]).unwrap(), public);
        let (our, private) = hello(1, &prior).unwrap();
        stream
            .0
            .send_raw(serde_json::to_vec(&our).unwrap())
            .await
            .unwrap();
        encrypt(&mut stream, &our, &remote, &private, true).unwrap();
        assert_eq!(receive(&mut stream).await.unwrap(), DOMAIN);
        stream.0.send_raw(DOMAIN.to_vec()).await.unwrap();
        stream
    }

    #[tokio::test]
    async fn delayed_sealdesk_preface_handshakes_and_timeout_follows_the_switch() {
        let (_lock, _reset) = lan_test_env();
        set_preface_wait_for_test(Some(Duration::from_millis(80)));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (public, _) = identity().unwrap();
        let expected = hex::encode(public.0);
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let local = socket.local_addr().unwrap();
            let mut stream = Stream::from(socket, local);
            assert_eq!(
                accept_shared(&mut stream).await.unwrap().as_deref(),
                Some(expected.as_str())
            );
        });
        let mut client = connect_preface_after(addr, Duration::from_millis(40)).await;
        assert!(client.is_secured());
        server.await.unwrap();

        Config::set_option(OPTION_RUSTDESK_COMPAT.to_owned(), "Y".into());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let socket = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (accepted, _) = listener.accept().await.unwrap();
        let local = accepted.local_addr().unwrap();
        let mut server = Stream::from(accepted, local);
        let started = Instant::now();
        assert!(accept_shared(&mut server).await.unwrap().is_none());
        assert!(started.elapsed() < Duration::from_millis(400));
        drop(socket);

        Config::set_option(OPTION_RUSTDESK_COMPAT.to_owned(), "N".into());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let socket = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (accepted, _) = listener.accept().await.unwrap();
        let local = accepted.local_addr().unwrap();
        let mut server = Stream::from(accepted, local);
        let started = Instant::now();
        let error = accept_shared(&mut server).await.unwrap_err();
        assert!(error.to_string().contains("disabled"));
        assert!(started.elapsed() < Duration::from_millis(400));
        drop(socket);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (public, _) = identity().unwrap();
        let expected = hex::encode(public.0);
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let local = socket.local_addr().unwrap();
            let mut stream = Stream::from(socket, local);
            assert_eq!(
                accept_shared(&mut stream).await.unwrap().as_deref(),
                Some(expected.as_str())
            );
        });
        let client = connect_preface_after(addr, Duration::from_millis(40)).await;
        assert!(client.is_secured());
        server.await.unwrap();
    }

    fn gate_open() -> LanServiceGate {
        LanServiceGate {
            file: true,
            camera: true,
            terminal: true,
            tunnel: true,
        }
    }

    fn remembered_desktop() -> LanRemembered {
        LanRemembered {
            permissions: [true; 8],
            terminal: false,
        }
    }

    #[test]
    fn compat_unions_stay_desktop_only() {
        for kind in [
            LanLoginKind::FileTransfer,
            LanLoginKind::Camera,
            LanLoginKind::Terminal,
            LanLoginKind::PortForward,
        ] {
            assert_eq!(
                decide_lan_login(false, true, kind, gate_open(), Some(remembered_desktop())),
                LanLoginDecision::RejectCompatDesktopOnly,
                "{kind:?}"
            );
        }
        assert_eq!(
            decide_lan_login(false, true, LanLoginKind::Desktop, gate_open(), None),
            LanLoginDecision::Continue
        );
    }

    #[test]
    fn lan_terminal_still_rejected_when_enable_terminal_is_off() {
        let mut gate = gate_open();
        gate.terminal = false;
        let mut saved = remembered_desktop();
        saved.terminal = true;
        assert_eq!(
            decide_lan_login(true, false, LanLoginKind::Terminal, gate, Some(saved)),
            LanLoginDecision::RejectPermission("No permission of terminal")
        );
    }

    #[test]
    fn lan_first_terminal_needs_confirmation_and_remember_defaults_off() {
        assert_eq!(
            decide_lan_login(true, false, LanLoginKind::Terminal, gate_open(), None),
            LanLoginDecision::NeedsConfirmation
        );
        assert_eq!(
            decide_lan_login(
                true,
                false,
                LanLoginKind::Terminal,
                gate_open(),
                Some(remembered_desktop())
            ),
            LanLoginDecision::NeedsConfirmation
        );
        let mut saved = remembered_desktop();
        saved.terminal = true;
        assert!(matches!(
            decide_lan_login(
                true,
                false,
                LanLoginKind::Terminal,
                gate_open(),
                Some(saved)
            ),
            LanLoginDecision::AllowRemembered(_)
        ));
        let parsed: Trusted = serde_json::from_str(
            r#"{"key":"ab","name":"n","permissions":[false,false,false,false,false,false,false,false]}"#,
        )
        .unwrap();
        assert!(!parsed.terminal);
    }

    #[test]
    fn lan_unions_follow_their_own_gates() {
        let mut gate = gate_open();
        gate.file = false;
        assert_eq!(
            decide_lan_login(
                true,
                false,
                LanLoginKind::FileTransfer,
                gate,
                Some(remembered_desktop())
            ),
            LanLoginDecision::RejectPermission("No permission of file transfer")
        );
        gate = gate_open();
        let mut saved = remembered_desktop();
        saved.permissions[FILE_PERMISSION] = false;
        assert_eq!(
            decide_lan_login(true, false, LanLoginKind::FileTransfer, gate, Some(saved)),
            LanLoginDecision::NeedsConfirmation
        );
        assert!(matches!(
            decide_lan_login(
                true,
                false,
                LanLoginKind::FileTransfer,
                gate,
                Some(remembered_desktop())
            ),
            LanLoginDecision::AllowRemembered(_)
        ));
        gate.camera = false;
        assert_eq!(
            decide_lan_login(
                true,
                false,
                LanLoginKind::Camera,
                gate,
                Some(remembered_desktop())
            ),
            LanLoginDecision::RejectPermission("No permission of viewing camera")
        );
        gate = gate_open();
        assert_eq!(
            decide_lan_login(
                true,
                false,
                LanLoginKind::Camera,
                gate,
                Some(remembered_desktop())
            ),
            LanLoginDecision::NeedsConfirmation
        );
        gate.tunnel = false;
        assert_eq!(
            decide_lan_login(
                true,
                false,
                LanLoginKind::PortForward,
                gate,
                Some(remembered_desktop())
            ),
            LanLoginDecision::RejectPermission("No permission of IP tunneling")
        );
        gate = gate_open();
        assert_eq!(
            decide_lan_login(
                true,
                false,
                LanLoginKind::PortForward,
                gate,
                Some(remembered_desktop())
            ),
            LanLoginDecision::NeedsConfirmation
        );
        assert!(matches!(
            decide_lan_login(
                true,
                false,
                LanLoginKind::Desktop,
                gate,
                Some(remembered_desktop())
            ),
            LanLoginDecision::AllowRemembered(_)
        ));
    }

    fn sample_peer(protocol: &str, address: &str, ip: &str, tailcat: &str) -> Peer {
        Peer {
            name: "Desk".into(),
            address: address.into(),
            ip: ip.into(),
            protocol: protocol.into(),
            platform: "Linux".into(),
            tailcat: tailcat.into(),
            mac: String::new(),
            nonce: String::new(),
        }
    }

    fn bound_peer(tailcat: &str, key_byte: u8, ip: &str) -> Peer {
        let key = hex::encode([key_byte; 32]);
        let nonce_bytes = [key_byte.wrapping_add(1); 32];
        let nonce = hex::encode(nonce_bytes);
        let address = format!("lan:{ip}:{TCP_PORT}/{key}");
        let mac = tailcat_binding_mac(tailcat, &key, TCP_PORT, &nonce_bytes);
        Peer {
            name: "Desk".into(),
            address,
            ip: ip.into(),
            protocol: "sealdesk".into(),
            platform: "Linux".into(),
            tailcat: String::new(),
            mac,
            nonce,
        }
    }

    fn lan_locals() -> Vec<LocalNet> {
        vec![LocalNet {
            addr: "192.168.1.4".parse().unwrap(),
            prefix: 24,
        }]
    }

    #[test]
    fn known_tailcat_peer_on_the_same_segment_upgrades_then_falls_back() {
        let tc = "tcabcdefghijklmnopqrstuvwxyz0123456789ABCD";
        let peer = bound_peer(tc, 9, "192.168.1.9");
        let address = peer.address.clone();
        let peers = vec![peer];
        let upgrade = plan_lan_upgrade(tc, &peers, &lan_locals(), "00").unwrap();
        assert_eq!(upgrade.address, address);
        assert!(is_encrypted_lan_channel(&upgrade.address));
        assert_eq!(transport_after_lan_error("connection refused"), "tailcat");
        assert_eq!(
            transport_after_lan_error("LAN host identity changed"),
            "tailcat"
        );
        assert_eq!(
            transport_after_lan_error("LAN signature verification failed"),
            "tailcat"
        );
        assert_eq!(transport_after_lan_error("timed out"), "tailcat");
        let other = LocalNet {
            addr: "10.0.0.4".parse().unwrap(),
            prefix: 24,
        };
        assert!(plan_lan_upgrade(tc, &peers, &[other], "00").is_none());
    }

    #[test]
    fn plaintext_compat_is_never_an_encrypted_lan_channel() {
        let tc = "tcabcdefghijklmnopqrstuvwxyz0123456789ABCD";
        let plain = "192.168.1.9:21118";
        assert!(!is_encrypted_lan_channel(plain));
        assert!(!is_encrypted_lan_channel("tcp:192.168.1.9:21118"));
        let peers = vec![
            sample_peer("rustdesk", plain, "192.168.1.9", tc),
            sample_peer("sealdesk", plain, "192.168.1.9", tc),
        ];
        assert!(plan_lan_upgrade(tc, &peers, &lan_locals(), "00").is_none());
        assert!(plan_lan_upgrade(plain, &peers, &lan_locals(), "00").is_none());
    }

    #[test]
    fn forged_discovery_is_rejected_and_falls_back_to_tailcat() {
        let tc = "tcabcdefghijklmnopqrstuvwxyz0123456789ABCD";
        let other = "tcabcdefghijklmnopqrstuvwxyz0123456789ZZZZ";
        let mut claimed = bound_peer(other, 3, "192.168.1.9");
        claimed.tailcat = tc.into();
        assert!(plan_lan_upgrade(tc, &[claimed], &lan_locals(), "00").is_none());
        let mut swapped = bound_peer(tc, 9, "192.168.1.8");
        let attacker = hex::encode([4u8; 32]);
        swapped.address = format!("lan:192.168.1.9:{TCP_PORT}/{attacker}");
        swapped.ip = "192.168.1.9".into();
        swapped.tailcat = tc.into();
        assert!(
            plan_lan_upgrade(tc, std::slice::from_ref(&swapped), &lan_locals(), "00").is_none()
        );
        let mut invented = swapped.clone();
        invented.mac = hex::encode([0u8; 32]);
        assert!(plan_lan_upgrade(tc, &[invented], &lan_locals(), "00").is_none());
        assert_eq!(
            transport_after_lan_error("LAN signature verification failed"),
            "tailcat"
        );
    }

    #[test]
    fn discovery_reply_omits_the_tailcat_credential() {
        let tc = "tcabcdefghijklmnopqrstuvwxyz0123456789ABCD";
        let key = hex::encode([7u8; 32]);
        let hint = sealdesk_hint(&key, true, tc);
        let misc = serde_json::to_string(&hint).unwrap();
        assert!(!misc.contains(tc));
        assert!(!misc.contains("tailcat"));
        let reply = PeerDiscovery {
            cmd: "pong".into(),
            id: key.clone(),
            misc: misc.clone(),
            ..Default::default()
        };
        let (_, peer) = discovered_peer(&reply, "192.168.1.8:99".parse().unwrap(), "own").unwrap();
        let listed = serde_json::to_string(&peer).unwrap();
        assert!(!listed.contains(tc));
        assert!(peer.tailcat.is_empty());
        let upgrade = plan_lan_upgrade(tc, &[peer], &lan_locals(), "00").unwrap();
        assert!(upgrade.address.contains(&key));
        let mut forged = serde_json::from_str::<serde_json::Value>(&misc).unwrap();
        forged["tailcat"] = serde_json::Value::String(tc.into());
        forged["mac"] = serde_json::Value::String(hex::encode([1u8; 32]));
        let forged_reply = PeerDiscovery {
            cmd: "pong".into(),
            id: key,
            misc: forged.to_string(),
            ..Default::default()
        };
        let (_, forged_peer) =
            discovered_peer(&forged_reply, "192.168.1.9:99".parse().unwrap(), "own").unwrap();
        assert!(!serde_json::to_string(&forged_peer).unwrap().contains(tc));
        assert!(plan_lan_upgrade(tc, &[forged_peer], &lan_locals(), "00").is_none());
        assert_eq!(transport_after_lan_error("forged discovery"), "tailcat");
    }

    #[test]
    fn direct_off_does_not_advertise_tcp_or_offer_an_upgrade() {
        let tc = "tcabcdefghijklmnopqrstuvwxyz0123456789ABCD";
        let key = hex::encode([8u8; 32]);
        let hint = sealdesk_hint(&key, false, tc);
        assert_eq!(hint.port, advertised_tcp_port(false));
        assert!(hint.mac.is_empty());
        let misc = serde_json::to_string(&hint).unwrap();
        assert!(!misc.contains(tc));
        let reply = PeerDiscovery {
            cmd: "pong".into(),
            id: key,
            misc,
            ..Default::default()
        };
        assert!(discovered_peer(&reply, "192.168.1.8:99".parse().unwrap(), "own").is_none());
        assert!(plan_lan_upgrade(tc, &[], &lan_locals(), "00").is_none());
        assert!(compat_pong_id(false, "192.168.1.8".parse().unwrap()).is_none());
        assert_eq!(
            compat_pong_id(true, "192.168.1.8".parse().unwrap()).as_deref(),
            Some(format!("192.168.1.8:{TCP_PORT}").as_str())
        );
    }

    #[test]
    fn simultaneous_lan_dial_has_a_single_immediate_side() {
        let low = "11";
        let high = "ff";
        let first = lan_dial_delay(low, high);
        let second = lan_dial_delay(high, low);
        assert_eq!(first, Duration::ZERO);
        assert_eq!(second, LAN_RACE_WAIT);
        assert_ne!(first, second);
        assert!(first.min(second) == Duration::ZERO);
        assert!(first.max(second) > Duration::ZERO);
    }

    #[test]
    fn tcp_bind_addrs_exclude_unspecified_and_use_android_fallback() {
        let unspecified = [
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            IpAddr::V6(Ipv6Addr::UNSPECIFIED),
            IpAddr::V4("127.0.0.1".parse().unwrap()),
            IpAddr::V4("169.254.1.1".parse().unwrap()),
            IpAddr::V4("100.64.0.1".parse().unwrap()),
            IpAddr::V4("8.8.8.8".parse().unwrap()),
            "fe80::1".parse().unwrap(),
        ];
        assert!(tcp_bind_addrs(&unspecified, &[]).is_empty());
        let mixed = [
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            IpAddr::V6(Ipv6Addr::UNSPECIFIED),
            "10.1.2.3".parse().unwrap(),
            "172.16.0.1".parse().unwrap(),
            "192.168.1.1".parse().unwrap(),
            "fc00::1".parse().unwrap(),
            "fd12::1".parse().unwrap(),
            "8.8.8.8".parse().unwrap(),
        ];
        let bound = tcp_bind_addrs(&mixed, &["0.0.0.0".parse().unwrap()]);
        let ips: Vec<_> = bound.iter().map(|addr| addr.ip()).collect();
        assert!(!ips.iter().any(|ip| ip.is_unspecified()));
        assert!(!ips.contains(&IpAddr::V4(Ipv4Addr::UNSPECIFIED)));
        assert!(!ips.contains(&IpAddr::V6(Ipv6Addr::UNSPECIFIED)));
        assert_eq!(ips.len(), 5);
        let fallback = tcp_bind_addrs(
            &[],
            &[
                "192.168.4.2".parse().unwrap(),
                Ipv4Addr::UNSPECIFIED,
                "8.8.8.8".parse().unwrap(),
            ],
        );
        assert_eq!(
            fallback,
            vec![SocketAddr::new("192.168.4.2".parse().unwrap(), TCP_PORT)]
        );
        assert!(serve_listeners(false, true, true) == (false, false));
        assert_eq!(serve_listeners(true, true, false), (true, false));
        assert_eq!(serve_listeners(true, false, true), (false, true));
        assert_eq!(serve_listeners(true, false, false), (false, false));
    }
}
