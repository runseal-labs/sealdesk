//! SealDesk LAN discovery uses RustDesk's PeerDiscovery query/reply protocol.
//! Native sessions use signed ephemeral key exchange and the existing split-key stream cipher.
use hbb_common::{
    anyhow::{anyhow, Context},
    bail,
    config::{Config, LocalConfig},
    log,
    protobuf::Message as _,
    rendezvous_proto::{rendezvous_message, PeerDiscovery, RendezvousMessage},
    sodiumoxide::{
        crypto::{
            aead::xchacha20poly1305_ietf, box_, scalarmult::curve25519, secretbox, sign,
        },
        randombytes,
    },
    tcp, tokio, ResultType, Stream,
};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};
use std::{
    collections::BTreeMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant, SystemTime},
};
#[path = "lan_udp.rs"]
mod udp;
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
    /// Hex possession proof from the discovery hint. Empty for a plaintext RustDesk reply.
    #[serde(default)]
    pub proof: String,
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
    /// Discovery nonce covered by `proof`. Not a tailcat credential.
    #[serde(default)]
    nonce: String,
    /// Possession proof sealed to this request. The tailcat address itself is never sent.
    #[serde(default)]
    proof: String,
    /// Hex node public key. The controller opens `proof` with it; the address stays off the wire.
    #[serde(default)]
    node: String,
}

const HINT_VERSION: u32 = 2;
const PROTO_SEALDESK: &str = "sealdesk";
const PROTO_COMPAT: &str = "compat";
const POSSESSION_LABEL: &[u8] = b"sealdesk-lan-possession-v1";

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

/// One discovery request's ephemeral X25519 secret and nonce. Dropped, and the
/// secret zeroized, when the request is finished. Only replies to this nonce
/// are accepted.
struct PossessionChallenge {
    eph_public: [u8; 32],
    nonce: [u8; 32],
    secret: Zeroizing<[u8; 32]>,
}

#[derive(Serialize, Deserialize)]
struct PingBody {
    domain: String,
    eph: String,
    nonce: String,
}

struct DiscoveryRound {
    peers: Vec<Peer>,
    challenge: PossessionChallenge,
}

fn ensure_sodium() -> bool {
    hbb_common::sodiumoxide::init().is_ok()
}

fn decode32(hex_key: &str) -> Option<[u8; 32]> {
    let bytes = hex::decode(hex_key).ok()?;
    if bytes.len() != 32 {
        return None;
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Some(out)
}

impl PossessionChallenge {
    fn generate() -> Option<Self> {
        if !ensure_sodium() {
            return None;
        }
        let (public, mut secret) = box_::gen_keypair();
        let kept = Zeroizing::new(secret.0);
        secret.0.zeroize();
        let mut nonce = [0u8; 32];
        randombytes::randombytes_into(&mut nonce);
        Some(Self {
            eph_public: public.0,
            nonce,
            secret: kept,
        })
    }

    fn ping_misc(&self) -> String {
        serde_json::to_string(&PingBody {
            domain: String::from_utf8_lossy(DOMAIN).into_owned(),
            eph: hex::encode(self.eph_public),
            nonce: hex::encode(self.nonce),
        })
        .unwrap_or_default()
    }
}

fn parse_ping_challenge(query: &PeerDiscovery) -> Option<([u8; 32], [u8; 32])> {
    let body: PingBody = serde_json::from_str(&query.misc).ok()?;
    if body.domain.as_bytes() != DOMAIN {
        return None;
    }
    Some((decode32(&body.eph)?, decode32(&body.nonce)?))
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        let digest = Sha256::digest(key);
        k[..32].copy_from_slice(&digest);
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; BLOCK];
    let mut opad = [0x5cu8; BLOCK];
    for i in 0..BLOCK {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(data);
    let inner = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner);
    let digest = outer.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    k.zeroize();
    ipad.zeroize();
    opad.zeroize();
    out
}

fn hkdf_sha256(salt: &[u8], ikm: &[u8], info: &[u8]) -> [u8; 32] {
    let mut prk = hmac_sha256(salt, ikm);
    let mut labeled = Vec::with_capacity(info.len() + 1);
    labeled.extend_from_slice(info);
    labeled.push(1);
    let okm = hmac_sha256(&prk, &labeled);
    prk.zeroize();
    labeled.zeroize();
    okm
}

fn x25519_public(secret: &[u8; 32]) -> Option<[u8; 32]> {
    let scalar = curve25519::Scalar::from_slice(secret)?;
    Some(curve25519::scalarmult_base(&scalar).0)
}

fn x25519_shared(secret: &[u8; 32], public: &[u8; 32]) -> Option<Zeroizing<[u8; 32]>> {
    let scalar = curve25519::Scalar::from_slice(secret)?;
    let point = curve25519::GroupElement(*public);
    let shared = curve25519::scalarmult(&scalar, &point).ok()?;
    Some(Zeroizing::new(shared.0))
}

fn possession_key(
    our_secret: &[u8; 32],
    their_public: &[u8; 32],
    eph_public: &[u8; 32],
    node_public: &[u8; 32],
) -> Option<Zeroizing<[u8; 32]>> {
    let shared = x25519_shared(our_secret, their_public)?;
    let mut info = Vec::with_capacity(POSSESSION_LABEL.len() + 64);
    info.extend_from_slice(POSSESSION_LABEL);
    info.extend_from_slice(eph_public);
    info.extend_from_slice(node_public);
    let okm = hkdf_sha256(POSSESSION_LABEL, shared.as_ref(), &info);
    info.zeroize();
    Some(Zeroizing::new(okm))
}

/// Plaintext and AAD. Both cover the ephemeral public key, nonce, LAN public
/// key, port, and protocol id (`sealdesk` or `compat`).
fn possession_binding(
    eph_public: &[u8; 32],
    nonce: &[u8; 32],
    lan_public: &[u8; 32],
    port: u16,
    protocol: &str,
) -> Option<Vec<u8>> {
    if port == 0 || (protocol != PROTO_SEALDESK && protocol != PROTO_COMPAT) {
        return None;
    }
    let mut body = Vec::with_capacity(POSSESSION_LABEL.len() + 98 + protocol.len());
    body.extend_from_slice(POSSESSION_LABEL);
    body.extend_from_slice(eph_public);
    body.extend_from_slice(nonce);
    body.extend_from_slice(lan_public);
    body.extend_from_slice(&port.to_be_bytes());
    body.push(u8::try_from(protocol.len()).ok()?);
    body.extend_from_slice(protocol.as_bytes());
    Some(body)
}

/// Seal a possession proof to the controller's ephemeral key with the tailcat
/// node secret. X25519, then HKDF-SHA256, then XChaCha20-Poly1305. The binding
/// is both the plaintext and the AAD.
fn seal_possession(
    node_secret: &[u8; 32],
    eph_public: &[u8; 32],
    nonce: &[u8; 32],
    lan_public: &[u8; 32],
    port: u16,
    protocol: &str,
) -> Option<String> {
    if !ensure_sodium() {
        return None;
    }
    let node_public = x25519_public(node_secret)?;
    let key = possession_key(node_secret, eph_public, eph_public, &node_public)?;
    let body = possession_binding(eph_public, nonce, lan_public, port, protocol)?;
    let aead_nonce = xchacha20poly1305_ietf::gen_nonce();
    let mut aead_key = xchacha20poly1305_ietf::Key(*key);
    let ciphertext = xchacha20poly1305_ietf::seal(&body, Some(&body), &aead_nonce, &aead_key);
    aead_key.0.zeroize();
    let mut packed = Vec::with_capacity(aead_nonce.0.len() + ciphertext.len());
    packed.extend_from_slice(&aead_nonce.0);
    packed.extend_from_slice(&ciphertext);
    Some(hex::encode(packed))
}

fn open_possession(
    node_public: &[u8; 32],
    eph_secret: &[u8; 32],
    eph_public: &[u8; 32],
    nonce: &[u8; 32],
    lan_public: &[u8; 32],
    port: u16,
    protocol: &str,
    proof_hex: &str,
) -> bool {
    if !ensure_sodium() || proof_hex.len() > 1024 {
        return false;
    }
    let Ok(packed) = hex::decode(proof_hex) else {
        return false;
    };
    if packed.len() < xchacha20poly1305_ietf::NONCEBYTES + xchacha20poly1305_ietf::TAGBYTES {
        return false;
    }
    let (nonce_bytes, ciphertext) = packed.split_at(xchacha20poly1305_ietf::NONCEBYTES);
    let Some(aead_nonce) = xchacha20poly1305_ietf::Nonce::from_slice(nonce_bytes) else {
        return false;
    };
    let Some(body) = possession_binding(eph_public, nonce, lan_public, port, protocol) else {
        return false;
    };
    let Some(key) = possession_key(eph_secret, node_public, eph_public, node_public) else {
        return false;
    };
    let mut aead_key = xchacha20poly1305_ietf::Key(*key);
    let opened = xchacha20poly1305_ietf::open(ciphertext, Some(&body), &aead_nonce, &aead_key);
    aead_key.0.zeroize();
    let Ok(clear) = opened else {
        return false;
    };
    clear.len() == body.len() && hbb_common::sodiumoxide::utils::memcmp(&clear, &body)
}

fn sealdesk_hint(
    lan_key_hex: &str,
    direct: bool,
    request: Option<&([u8; 32], [u8; 32])>,
    node_secret: Option<&[u8; 32]>,
) -> Hint {
    let port = advertised_tcp_port(direct);
    let (nonce, proof, node) = match (request, node_secret, decode32(lan_key_hex)) {
        (Some((eph, nonce)), Some(secret), Some(lan_public)) if port != 0 => {
            let proof = seal_possession(secret, eph, nonce, &lan_public, port, PROTO_SEALDESK)
                .unwrap_or_default();
            let node = x25519_public(secret).map(hex::encode).unwrap_or_default();
            if proof.is_empty() || node.is_empty() {
                (String::new(), String::new(), String::new())
            } else {
                (hex::encode(nonce), proof, node)
            }
        }
        _ => (String::new(), String::new(), String::new()),
    };
    Hint {
        version: HINT_VERSION,
        key: lan_key_hex.to_owned(),
        port,
        nonce,
        proof,
        node,
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
    Ok(connect_socket(addr, Some(expected)).await?.0)
}

pub(crate) const UNVERIFIED_DIRECT: &str = "Direct";

pub(crate) const DIRECT_ADDRESS_DENIED: &str =
    "Direct connection allows only a private, 100.64.0.0/10, link-local, or loopback address";

pub(crate) fn shows_verified_identity(stream_type: &str, transport_secured: bool) -> bool {
    transport_secured && stream_type != UNVERIFIED_DIRECT
}

pub(crate) fn is_typed_direct(value: &str) -> bool {
    hbb_common::is_ip_str(value) || is_unspecified_v6_literal(value)
}

fn is_unspecified_v6_literal(value: &str) -> bool {
    value == "::" || value == "[::]" || value.starts_with("[::]:")
}

pub(crate) fn is_pinned_or_typed(value: &str) -> bool {
    is_addr(value) || is_typed_direct(value)
}

/// `lan:` stays on the pinned dial. A typed address never replaces that dial.
/// Its host key is remembered per normalized `host:port` only after the user
/// confirms. A later match is reported as `LAN` (verified). A new or changed
/// key stays `Direct` until then. A changed `lan:` pin still fails closed.
///
/// The third value is the host key this dial negotiated when that key still
/// needs confirmation. It is `None` for a pinned dial or a key that already matches.
pub(crate) async fn connect_pinned_or_typed(
    peer: &str,
) -> ResultType<(Stream, &'static str, Option<String>)> {
    if is_addr(peer) {
        return Ok((connect(peer).await?, "LAN", None));
    }
    if is_typed_direct(peer) {
        let addr = parse_direct_socket(peer)?;
        let (stream, presented) = connect_socket(addr, None).await?;
        let address = addr.to_string();
        let presented = hex::encode(presented.0);
        let stored = {
            let state = direct_host_state().lock().unwrap();
            canonical_stored_host(&load_direct_hosts(&state), &address)
        };
        if stored.as_deref() == Some(presented.as_str()) {
            return Ok((stream, "LAN", None));
        }
        return Ok((stream, UNVERIFIED_DIRECT, Some(presented)));
    }
    bail!("not a direct address")
}

/// Typed `ip`, `ip:port`, or IPv6. The host key is learned from the handshake
/// (there is no `lan:` pin). The preface is still SealDesk's, not legacy TCP.
pub async fn connect_direct(value: &str) -> ResultType<Stream> {
    Ok(connect_socket(parse_direct_socket(value)?, None).await?.0)
}

fn parse_direct_socket(value: &str) -> ResultType<SocketAddr> {
    if is_unspecified_v6_literal(value) {
        bail!(DIRECT_ADDRESS_DENIED);
    }
    if !hbb_common::is_ip_str(value) {
        bail!("not an IP address");
    }
    let with_port = hbb_common::socket_client::check_port(value, i32::from(TCP_PORT));
    let addr: SocketAddr = with_port.parse().context("invalid IP address")?;
    if addr.port() == 0 {
        bail!("invalid IP port");
    }
    if !direct_ip_allowed(addr.ip()) {
        bail!(DIRECT_ADDRESS_DENIED);
    }
    Ok(addr)
}

fn direct_ip_allowed(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_loopback() || ip.is_private() || ip.is_link_local() || is_shared_v4(ip)
        }
        IpAddr::V6(ip) => ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local(),
    }
}

/// RFC 6598. `Ipv4Addr::is_private` does not include this range.
fn is_shared_v4(ip: Ipv4Addr) -> bool {
    let [first, second, _, _] = ip.octets();
    first == 100 && (64..128).contains(&second)
}

async fn connect_socket(
    addr: SocketAddr,
    expected: Option<sign::PublicKey>,
) -> ResultType<(Stream, sign::PublicKey)> {
    let socket = tokio::net::TcpStream::connect(addr).await?;
    socket.set_nodelay(true)?;
    let local = socket.local_addr()?;
    let mut stream = Stream::from(socket, local);
    stream.set_max_packet_length(FRAME_LIMIT);
    stream.0.send_raw(DOMAIN.to_vec()).await?;
    let prior = receive(&mut stream).await?;
    let remote: Hello = serde_json::from_slice(&prior)?;
    let presented = verify(&remote, 0, &[])?;
    if expected.is_some_and(|key| key != presented) {
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
    Ok((stream, presented))
}

const DIRECT_HOST_KEYS: &str = "direct-host-keys";

pub(crate) struct DirectPrompt {
    pub msg_type: &'static str,
    pub title: &'static str,
    pub text: String,
}

struct DirectHostState {
    #[cfg(test)]
    memory: Option<BTreeMap<String, String>>,
}

fn direct_host_state() -> &'static Mutex<DirectHostState> {
    static STATE: Mutex<DirectHostState> = Mutex::new(DirectHostState {
        #[cfg(test)]
        memory: None,
    });
    &STATE
}

fn normalize_direct_address(value: &str) -> ResultType<String> {
    Ok(parse_direct_socket(value)?.to_string())
}

fn canonical_host_key(key: &str) -> Option<String> {
    let bytes = hex::decode(key.trim()).ok()?;
    if bytes.len() != 32 {
        return None;
    }
    Some(hex::encode(bytes))
}

fn canonical_stored_host(hosts: &BTreeMap<String, String>, address: &str) -> Option<String> {
    hosts.get(address).and_then(|key| canonical_host_key(key))
}

/// First 16 hex digits of the host key, in groups of four.
fn fingerprint_prefix(hex_key: &str) -> String {
    let shown: String = hex_key
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .take(16)
        .collect();
    let mut out = String::new();
    for (i, ch) in shown.chars().enumerate() {
        if i > 0 && i % 4 == 0 {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}

fn load_direct_hosts(state: &DirectHostState) -> BTreeMap<String, String> {
    #[cfg(test)]
    if let Some(memory) = &state.memory {
        return memory.clone();
    }
    #[cfg(not(test))]
    let _ = state;
    let raw = LocalConfig::get_option(DIRECT_HOST_KEYS);
    if raw.is_empty() {
        return BTreeMap::new();
    }
    match serde_json::from_str(&raw) {
        Ok(hosts) => hosts,
        Err(err) => {
            log::warn!("ignoring unreadable direct host keys: {err}");
            BTreeMap::new()
        }
    }
}

fn store_direct_hosts(state: &mut DirectHostState, hosts: &BTreeMap<String, String>) {
    #[cfg(test)]
    if state.memory.is_some() {
        state.memory = Some(hosts.clone());
        return;
    }
    #[cfg(not(test))]
    let _ = state;
    LocalConfig::set_option(
        DIRECT_HOST_KEYS.to_owned(),
        serde_json::to_string(hosts).unwrap_or_else(|_| "{}".to_owned()),
    );
}

fn insert_direct_host(state: &mut DirectHostState, address: String, key: String) {
    let mut hosts = load_direct_hosts(state);
    hosts.insert(address, key);
    store_direct_hosts(state, &hosts);
}

#[cfg(test)]
fn stored_direct_host(address: &str) -> Option<String> {
    let state = direct_host_state().lock().unwrap();
    canonical_stored_host(&load_direct_hosts(&state), address)
}

#[cfg(test)]
fn remember_direct_host(address: &str, key: &str) -> ResultType<()> {
    let key = canonical_host_key(key).context("invalid direct host key")?;
    let mut state = direct_host_state().lock().unwrap();
    insert_direct_host(&mut state, address.to_owned(), key);
    Ok(())
}

/// Prompt for a typed dial whose negotiated host key is not the saved one.
/// `presented` belongs to this session. `None` when that key is already saved,
/// or the peer is not a typed dial.
pub(crate) fn direct_confirm_prompt(peer: &str, presented: &str) -> Option<DirectPrompt> {
    let address = normalize_direct_address(peer).ok()?;
    let presented = canonical_host_key(presented)?;
    let state = direct_host_state().lock().unwrap();
    let stored = canonical_stored_host(&load_direct_hosts(&state), &address);
    drop(state);
    if stored.as_deref() == Some(presented.as_str()) {
        return None;
    }
    Some(match stored {
        Some(old) => DirectPrompt {
            msg_type: "insecure-connection-changed-error",
            title: "Host identity changed",
            text: format!(
                "{}\n→\n{}",
                fingerprint_prefix(&old),
                fingerprint_prefix(&presented)
            ),
        },
        None => DirectPrompt {
            msg_type: "insecure-connection-nocancel-hasclose",
            title: "Unverified identity",
            text: fingerprint_prefix(&presented),
        },
    })
}

/// Record this session's host key. A dismiss leaves the saved key unchanged.
pub(crate) fn accept_direct_host(peer: &str, presented: &str) {
    let Ok(address) = normalize_direct_address(peer) else {
        return;
    };
    let Some(key) = canonical_host_key(presented) else {
        return;
    };
    let mut state = direct_host_state().lock().unwrap();
    insert_direct_host(&mut state, address, key);
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
            // Same private interfaces as discovery, plus CGNAT 100.64.0.0/10
            // even when that address is on a virtual or point-to-point adapter.
            // On Windows the private set drops down adapters and
            // IfType/TAP/Wintun/WireGuard tunnels.
            // Android denies netlink, so default_net is empty and Wi-Fi is only
            // in the Tailcat interface list. Emptiness is decided before CGNAT
            // is appended, or a Tailscale address would hide that Wi-Fi entry.
            let discovered: Vec<Ipv4Addr> = udp::interfaces_from_default_net()
                .into_iter()
                .flat_map(|iface| iface.v4)
                .map(|(ip, _)| ip)
                .collect();
            #[cfg(target_os = "android")]
            let fallback = crate::tailcat::android_private_ipv4();
            #[cfg(not(target_os = "android"))]
            let fallback = Vec::new();
            compat_share_addrs(&discovered, &direct_cgnat_v4(), &fallback)
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
/// Node secret used to seal a possession proof. Stat and read run on a
/// blocking thread. The async loop only receives `Option<Zeroizing<[u8; 32]>>`.
/// `tailcat-server.json` is reread only when its mtime or length changes, so one
/// peer cannot stall discovery by forcing a parse on every ping. The key and
/// the file contents are never logged.
struct NodeSecretCache {
    path: String,
    seen: bool,
    stamp: Option<(Option<SystemTime>, u64)>,
    key: Option<Zeroizing<[u8; 32]>>,
}

impl NodeSecretCache {
    fn load() -> Self {
        let mut cache = Self {
            path: crate::tailcat::server_key_path(),
            seen: false,
            stamp: None,
            key: None,
        };
        cache.refresh_if_changed();
        cache
    }

    fn refresh_if_changed(&mut self) {
        let meta = std::fs::metadata(&self.path).ok();
        let stamp = meta
            .as_ref()
            .map(|meta| (meta.modified().ok(), meta.len()));
        if self.seen && stamp == self.stamp {
            return;
        }
        self.stamp = stamp;
        self.seen = true;
        self.key = if meta.is_some() {
            crate::tailcat::node_private_key()
        } else {
            None
        };
    }
}

/// Loads and refreshes the node secret on `spawn_blocking`. Only the key is
/// sent back; an unchanged mtime and length does not read the file.
fn watch_node_secret() -> (
    tokio::sync::mpsc::Receiver<Option<Zeroizing<[u8; 32]>>>,
    tokio::task::JoinHandle<()>,
) {
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    let task = tokio::spawn(async move {
        let mut cache = match tokio::task::spawn_blocking(NodeSecretCache::load).await {
            Ok(cache) => cache,
            Err(_) => return,
        };
        if tx.send(cache.key.clone()).await.is_err() {
            return;
        }
        loop {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let before = cache.stamp;
            cache = match tokio::task::spawn_blocking(move || {
                cache.refresh_if_changed();
                cache
            })
            .await
            {
                Ok(cache) => cache,
                Err(_) => return,
            };
            if cache.stamp != before && tx.send(cache.key.clone()).await.is_err() {
                return;
            }
        }
    });
    (rx, task)
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
    let (mut node_key_rx, mut watch_node_key) = if discovery {
        let (rx, task) = watch_node_secret();
        tasks.push(task);
        (rx, true)
    } else {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        (rx, false)
    };
    let mut node_key: Option<Zeroizing<[u8; 32]>> = None;
    let _abort = AbortOnDrop(tasks);
    let (udp_tx, mut udp_rx) = tokio::sync::mpsc::channel(32);
    let mut listeners = udp::ListenerSet::empty();
    let mut scanned: Option<Instant> = None;
    let mut last = Instant::now();
    let mut replies = 0;
    loop {
        if !crate::tailcat::host::enabled()
            || direct_server_enabled() != direct
            || lan_discovery_enabled() != discovery
        {
            return Ok(());
        }
        if discovery
            && scanned
                .map(|at| at.elapsed() >= udp::IFACE_REFRESH)
                .unwrap_or(true)
        {
            scanned = Some(Instant::now());
            let desired = tokio::task::spawn_blocking(udp::current_bind_plan)
                .await
                .unwrap_or_default();
            let before = listeners.summary(DISCOVERY_PORT);
            listeners.reconcile(desired, DISCOVERY_PORT, udp_tx.clone());
            let after = listeners.summary(DISCOVERY_PORT);
            if before != after {
                if after.is_empty() {
                    log::info!("LAN discovery stopped listening");
                } else {
                    log::info!("LAN discovery listening on {after}");
                }
            } else if after.is_empty() {
                hbb_common::throttled_log!(
                    Duration::from_secs(60),
                    info,
                    "LAN discovery has no private address to bind"
                );
            }
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(100)) => {}
            loaded = node_key_rx.recv(), if watch_node_key => {
                match loaded {
                    Some(key) => node_key = key,
                    None => watch_node_key = false,
                }
            },
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
            incoming = udp_rx.recv(), if discovery && !listeners.sockets.is_empty() => {
                let Some(hit) = incoming else { continue; };
                let Some(sock) = listeners.sockets.get(&hit.key).cloned() else { continue; };
                let addr = hit.from;
                if !discovery_source_ok(addr.ip()) { continue; }
                if last.elapsed() >= Duration::from_secs(1) { replies = 0; last = Instant::now(); }
                if replies >= 20 { continue; }
                let Ok(msg) = RendezvousMessage::parse_from_bytes(&hit.bytes) else { continue; };
                let Some(rendezvous_message::Union::PeerDiscovery(query)) = msg.union else { continue; };
                if !answer_discovery_ping(&query) { continue; }
                let (key, _) = identity()?;
                let key = hex::encode(key.0);
                if query.id == key { continue; }
                let mut reply = RendezvousMessage::new();
                let sealdesk_ping = sealdesk_discovery(&query);
                let (id, misc) = if sealdesk_ping {
                    let request = parse_ping_challenge(&query);
                    let hint = sealdesk_hint(
                        &key,
                        direct,
                        request.as_ref(),
                        node_key.as_deref(),
                    );
                    if hint.proof.is_empty() {
                        continue;
                    }
                    (key.clone(), serde_json::to_string(&hint)?)
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
                if let Err(error) = sock.send_to(&reply.write_to_bytes()?, addr).await {
                    log::trace!("LAN discovery reply failed: {error}");
                    continue;
                }
                replies += 1;
            }
        }
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

/// Private IPv4 (10/8, 172.16/12, 192.168/16), CGNAT 100.64.0.0/10, and IPv6 ULA.
/// Unspecified, loopback, link-local, and public addresses are dropped.
/// An empty enumeration uses `fallback_v4` (the Android interface list) and
/// still never adds 0.0.0.0 or ::.
/// 100.64.0.0/10 is not RFC1918 (`Ipv4Addr::is_private` is false). Tailscale and
/// Headscale use it, often on a point-to-point interface that discovery skips.
/// Direct typed connections still need TCP 21118 on that address. UDP 21119 does not.
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
            IpAddr::V4(ip) => is_lan_bind_v4(*ip) || is_cgnat_v4(*ip),
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

/// RFC 6598 shared address space. Not private, not global; used by CGNAT and Tailscale.
pub(crate) fn is_cgnat_v4(ip: Ipv4Addr) -> bool {
    let [a, b, _, _] = ip.octets();
    a == 100 && (64..128).contains(&b)
}

/// CGNAT addresses from an Android-style interface list, including VPN and
/// point-to-point interfaces that LAN discovery refuses. Other VPN addresses stay out.
pub(crate) fn cgnat_v4_from_interface_json(payload: &str) -> Vec<Ipv4Addr> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
        return Vec::new();
    };
    let Some(list) = value.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for iface in list {
        let flags = iface.get("flags").and_then(|v| v.as_u64()).unwrap_or(0);
        // Go net.Flags: 1 up, 4 loopback. Point-to-point (8) and VPN names are kept.
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
            if is_cgnat_v4(ip) && !out.contains(&ip) {
                out.push(ip);
            }
        }
    }
    out
}

/// `tcp_bind_addrs` uses `fallback_v4` only when the filtered enumeration is
/// empty. Callers must pass the pre-CGNAT enumeration so an Android Wi-Fi
/// fallback is not skipped just because Tailscale added 100.64.0.0/10.
fn tcp_bind_with_direct_cgnat(
    enumerated: &[IpAddr],
    fallback_v4: &[Ipv4Addr],
    cgnat: &[Ipv4Addr],
) -> Vec<SocketAddr> {
    let mut addrs = tcp_bind_addrs(enumerated, fallback_v4);
    for ip in cgnat {
        if !is_cgnat_v4(*ip) {
            continue;
        }
        let addr = SocketAddr::new(IpAddr::V4(*ip), TCP_PORT);
        if !addrs.contains(&addr) {
            addrs.push(addr);
        }
    }
    addrs.sort();
    addrs.dedup();
    addrs
}

/// Discovery private/CGNAT addresses, then direct CGNAT. `android_fallback` is
/// appended only when the discovery set is empty, and that check happens
/// before CGNAT is added.
fn compat_share_addrs(
    discovered_v4: &[Ipv4Addr],
    cgnat: &[Ipv4Addr],
    android_fallback: &[Ipv4Addr],
) -> Vec<String> {
    let mut addresses: Vec<String> = discovered_v4
        .iter()
        .copied()
        .filter(|ip| ip.is_private() || is_cgnat_v4(*ip))
        .map(|ip| SocketAddr::new(IpAddr::V4(ip), TCP_PORT).to_string())
        .collect();
    let supplement_private = addresses.is_empty();
    for ip in cgnat {
        if !is_cgnat_v4(*ip) {
            continue;
        }
        let text = SocketAddr::new(IpAddr::V4(*ip), TCP_PORT).to_string();
        if !addresses.contains(&text) {
            addresses.push(text);
        }
    }
    if supplement_private {
        for ip in android_fallback {
            let text = SocketAddr::new(IpAddr::V4(*ip), TCP_PORT).to_string();
            if !addresses.contains(&text) {
                addresses.push(text);
            }
        }
    }
    addresses
}

fn direct_cgnat_v4() -> Vec<Ipv4Addr> {
    let mut out = Vec::new();
    for iface in default_net::get_interfaces() {
        for net in iface.ipv4 {
            if is_cgnat_v4(net.addr) && !out.contains(&net.addr) {
                out.push(net.addr);
            }
        }
    }
    #[cfg(target_os = "android")]
    for ip in crate::tailcat::android_cgnat_ipv4() {
        if !out.contains(&ip) {
            out.push(ip);
        }
    }
    out
}

fn is_ula(ip: Ipv6Addr) -> bool {
    ip.is_unique_local()
}

fn current_tcp_bind_addrs() -> Vec<SocketAddr> {
    let mut enumerated = Vec::new();
    for iface in udp::interfaces_from_default_net() {
        for (ip, _) in iface.v4 {
            enumerated.push(IpAddr::V4(ip));
        }
        for (ip, _) in iface.v6 {
            enumerated.push(IpAddr::V6(ip));
        }
    }
    // Discovery drops point-to-point and virtual adapters, which is where
    // 100.64.0.0/10 usually sits. Bind the Android Wi-Fi fallback from the
    // pre-CGNAT list, then append those CGNAT addresses for TCP only.
    #[cfg(target_os = "android")]
    let fallback = crate::tailcat::android_private_ipv4();
    #[cfg(not(target_os = "android"))]
    let fallback = Vec::new();
    tcp_bind_with_direct_cgnat(&enumerated, &fallback, &direct_cgnat_v4())
}
fn private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v.is_private() || v.is_loopback(),
        IpAddr::V6(v) => v.is_unique_local() || v.is_loopback(),
    }
}

fn discovery_source_ok(ip: IpAddr) -> bool {
    udp::udp_ip_allowed(ip)
        || match ip {
            IpAddr::V4(ip) => ip.is_loopback(),
            IpAddr::V6(ip) => ip.is_loopback(),
        }
}
pub fn discover_json() -> String {
    match discover() {
        Ok(round) => serde_json::to_string(&round.peers).unwrap_or_else(|_| "[]".into()),
        Err(e) => {
            log::trace!("LAN discovery: {e}");
            "[]".into()
        }
    }
}
#[cfg(unix)]
fn discovery_interface_flags(flags: u32) -> bool {
    use hbb_common::libc::{IFF_BROADCAST, IFF_LOOPBACK, IFF_POINTOPOINT, IFF_UP};

    flags & (IFF_UP | IFF_BROADCAST) as u32 == (IFF_UP | IFF_BROADCAST) as u32
        && flags & (IFF_LOOPBACK | IFF_POINTOPOINT) as u32 == 0
}

/// Unix TCP 21118 and UDP 21119 share this filter. A TUN subnet broadcast uses
/// the local routing table even outside a VPN's app allowlist. Windows flags
/// are not Unix IFF bits; that path uses `windows_discovery_adapter_allowed`
/// from the same `interfaces_from_default_net` enumeration.
#[cfg(unix)]
fn keep_discovery_iface(flags: u32) -> bool {
    discovery_interface_flags(flags)
}

#[cfg(not(unix))]
fn keep_discovery_iface(_flags: u32) -> bool {
    true
}

/// `IF_OPER_STATUS.IfOperStatusUp`.
#[cfg(any(windows, test))]
pub(crate) const IF_OPER_STATUS_UP: u32 = 1;
#[cfg(any(windows, test))]
pub(crate) const IF_TYPE_ETHERNET_CSMACD: u32 = 6;
#[cfg(any(windows, test))]
pub(crate) const IF_TYPE_PROP_POINT2POINT_SERIAL: u32 = 22;
#[cfg(any(windows, test))]
pub(crate) const IF_TYPE_PPP: u32 = 23;
#[cfg(any(windows, test))]
pub(crate) const IF_TYPE_SLIP: u32 = 28;
#[cfg(any(windows, test))]
pub(crate) const IF_TYPE_PROP_VIRTUAL: u32 = 53;
#[cfg(any(windows, test))]
pub(crate) const IF_TYPE_FASTETHER: u32 = 62;
#[cfg(any(windows, test))]
pub(crate) const IF_TYPE_IEEE80211: u32 = 71;
#[cfg(any(windows, test))]
pub(crate) const IF_TYPE_PPPMULTILINKBUNDLE: u32 = 108;
#[cfg(any(windows, test))]
pub(crate) const IF_TYPE_GIGABITETHERNET: u32 = 117;
#[cfg(any(windows, test))]
pub(crate) const IF_TYPE_TUNNEL: u32 = 131;
#[cfg(any(windows, test))]
pub(crate) const IF_TYPE_MPLS_TUNNEL: u32 = 150;
#[cfg(any(windows, test))]
pub(crate) const IF_TYPE_IEEE8023AD_LAG: u32 = 161;

#[cfg(any(windows, test))]
pub(crate) struct WindowsDiscoveryAdapter<'a> {
    pub if_type: u32,
    pub oper_status: u32,
    pub name: &'a str,
    pub friendly_name: &'a str,
    pub description: &'a str,
}

/// Shared Windows gate for UDP 21119 (query and listen) and TCP 21118 bind.
/// `OperStatus` must be Up. PPP, TUNNEL, and the other tunnel / virtual
/// `IfType`s are out, as are Wintun, TAP, and WireGuard names or descriptions.
#[cfg(any(windows, test))]
pub(crate) fn windows_discovery_adapter_allowed(adapter: &WindowsDiscoveryAdapter<'_>) -> bool {
    adapter.oper_status == IF_OPER_STATUS_UP
        && !windows_tunnel_if_type(adapter.if_type)
        && !windows_virtual_adapter_text(adapter.name)
        && !windows_virtual_adapter_text(adapter.friendly_name)
        && !windows_virtual_adapter_text(adapter.description)
}

#[cfg(any(windows, test))]
fn windows_tunnel_if_type(if_type: u32) -> bool {
    matches!(
        if_type,
        IF_TYPE_PROP_POINT2POINT_SERIAL
            | IF_TYPE_PPP
            | IF_TYPE_SLIP
            | IF_TYPE_PROP_VIRTUAL
            | IF_TYPE_PPPMULTILINKBUNDLE
            | IF_TYPE_TUNNEL
            | IF_TYPE_MPLS_TUNNEL
    )
}

#[cfg(any(windows, test))]
fn windows_virtual_adapter_text(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    ["wintun", "wireguard", "tap-windows", "tap-win32", "tapwin"]
        .iter()
        .any(|marker| text.contains(marker))
}

fn discover() -> ResultType<DiscoveryRound> {
    let (key, _) = identity()?;
    let own = hex::encode(key.0);
    let challenge =
        PossessionChallenge::generate().context("LAN discovery challenge unavailable")?;
    let mut query = RendezvousMessage::new();
    query.set_peer_discovery(PeerDiscovery {
        cmd: "ping".into(),
        id: own.clone(),
        misc: challenge.ping_misc(),
        ..Default::default()
    });
    let bytes = query.write_to_bytes()?;
    let enumerated = udp::interfaces_from_default_net();
    #[cfg(target_os = "android")]
    let android = udp::ifaces_from_android_json(&crate::tailcat::android_interface_json());
    #[cfg(not(target_os = "android"))]
    let android = Vec::new();
    let (ifaces, global_broadcast) = udp::query_targets(&enumerated, &android);
    let mut sockets = udp::collect_query_sockets(ifaces, DISCOVERY_PORT, &bytes);
    // Android 11+ can deny netlink, so default_net is empty or reports flags 0.
    // Query 255.255.255.255 from an ephemeral socket instead of binding 0.0.0.0:21119.
    #[cfg(not(target_os = "android"))]
    let _ = global_broadcast;
    #[cfg(target_os = "android")]
    if global_broadcast || sockets.is_empty() {
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
                if let Some((id, peer)) = discovered_peer(&reply, addr, &own, &challenge) {
                    peers.insert(id, peer);
                }
            }
        }
        if peers.len() >= 64 {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(DiscoveryRound {
        peers: peers.into_values().collect(),
        challenge,
    })
}

fn discovered_peer(
    reply: &PeerDiscovery,
    addr: SocketAddr,
    own: &str,
    challenge: &PossessionChallenge,
) -> Option<(String, Peer)> {
    if reply.cmd != "pong" || !private(addr.ip()) {
        return None;
    }
    let (id, address, protocol, proof, nonce) = if reply.misc.is_empty() {
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
        // A tailcat address inside misc is ignored: the address is a credential,
        // not a possession proof. Version-1 replies, other nonces, and proofs that
        // do not open are discarded.
        let hint = serde_json::from_str::<Hint>(&reply.misc).ok()?;
        if hint.version != HINT_VERSION
            || hint.key == own
            || hint.port == 0
            || hex::decode(&hint.key)
                .map(|b| b.len() != 32)
                .unwrap_or(true)
        {
            return None;
        }
        let nonce = decode32(&hint.nonce)?;
        if nonce != challenge.nonce {
            return None;
        }
        let lan_public = decode32(&hint.key)?;
        let node_public = decode32(&hint.node)?;
        if !open_possession(
            &node_public,
            &challenge.secret,
            &challenge.eph_public,
            &challenge.nonce,
            &lan_public,
            hint.port,
            PROTO_SEALDESK,
            &hint.proof,
        ) {
            return None;
        }
        let endpoint = SocketAddr::new(addr.ip(), hint.port);
        (
            hint.key.clone(),
            format!("lan:{endpoint}/{}", hint.key),
            PROTO_SEALDESK,
            hint.proof,
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
            proof,
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
    for iface in udp::interfaces_from_default_net() {
        for (ip, prefix) in iface.v4 {
            if is_lan_bind_v4(ip) && prefix > 0 && prefix <= 32 {
                nets.push(LocalNet {
                    addr: IpAddr::V4(ip),
                    prefix,
                });
            }
        }
        for (ip, prefix) in iface.v6 {
            if is_ula(ip) && prefix > 0 && prefix <= 128 {
                nets.push(LocalNet {
                    addr: IpAddr::V6(ip),
                    prefix,
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

/// Any LAN failure, including an identity-check mismatch, uses tailcat.
pub(crate) fn transport_after_lan_error(message: &str) -> &'static str {
    let _ = message;
    "tailcat"
}

pub(crate) fn is_encrypted_lan_channel(address: &str) -> bool {
    parse_addr(address).is_ok()
}

/// Known `tc` peers on the same private subnet upgrade to `lan:` only when the
/// reply's possession proof opens with that address's node public key and this
/// request's ephemeral key. A forged reply, or one from another request, is
/// ignored so the caller stays on tailcat. A plaintext RustDesk address is
/// never treated as that encrypted channel.
pub(crate) fn plan_lan_upgrade(
    peer: &str,
    node_public: &[u8; 32],
    peers: &[Peer],
    locals: &[LocalNet],
    local_key: &str,
    challenge: &PossessionChallenge,
) -> Option<LanUpgrade> {
    if !crate::tailcat::is_addr(peer) {
        return None;
    }
    let found = peers.iter().find(|candidate| {
        candidate.protocol == PROTO_SEALDESK
            && is_encrypted_lan_channel(&candidate.address)
            && candidate
                .ip
                .parse::<IpAddr>()
                .ok()
                .is_some_and(|ip| same_segment(ip, locals))
            && lan_upgrade_authenticated(node_public, candidate, challenge)
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

fn lan_upgrade_authenticated(
    node_public: &[u8; 32],
    candidate: &Peer,
    challenge: &PossessionChallenge,
) -> bool {
    if candidate.protocol != PROTO_SEALDESK {
        return false;
    }
    let Ok((addr, key)) = parse_addr(&candidate.address) else {
        return false;
    };
    let Some(nonce) = decode32(&candidate.nonce) else {
        return false;
    };
    if nonce != challenge.nonce {
        return false;
    }
    open_possession(
        node_public,
        &challenge.secret,
        &challenge.eph_public,
        &challenge.nonce,
        &key.0,
        addr.port(),
        PROTO_SEALDESK,
        &candidate.proof,
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
    let Ok(Ok(round)) = tokio::task::spawn_blocking(discover).await else {
        return None;
    };
    let Some(node_public) = crate::tailcat::node_public(&peer) else {
        return None;
    };
    plan_lan_upgrade(
        &peer,
        &node_public,
        &round.peers,
        &local_segments(),
        &local_identity_hex(),
        &round.challenge,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn discovery_skips_vpn_interfaces() {
        use hbb_common::libc::{IFF_BROADCAST, IFF_POINTOPOINT, IFF_UP};

        assert!(!discovery_interface_flags(
            (IFF_UP | IFF_POINTOPOINT) as u32
        ));
        assert!(!discovery_interface_flags(
            (IFF_UP | IFF_BROADCAST | IFF_POINTOPOINT) as u32
        ));
    }

    #[cfg(unix)]
    #[test]
    fn discovery_keeps_active_lan_broadcast_interfaces() {
        use hbb_common::libc::{IFF_BROADCAST, IFF_UP};

        assert!(discovery_interface_flags((IFF_UP | IFF_BROADCAST) as u32));
        assert!(!discovery_interface_flags(IFF_BROADCAST as u32));
    }

    fn windows_adapter<'a>(
        if_type: u32,
        oper_status: u32,
        name: &'a str,
        friendly_name: &'a str,
        description: &'a str,
    ) -> WindowsDiscoveryAdapter<'a> {
        WindowsDiscoveryAdapter {
            if_type,
            oper_status,
            name,
            friendly_name,
            description,
        }
    }

    #[test]
    fn windows_discovery_keeps_ethernet_and_wifi_and_drops_vpn_adapters() {
        let keep = [
            windows_adapter(
                IF_TYPE_ETHERNET_CSMACD,
                IF_OPER_STATUS_UP,
                "{ETH}",
                "Ethernet",
                "Intel(R) Ethernet Connection (7) I219-V",
            ),
            windows_adapter(
                IF_TYPE_IEEE80211,
                IF_OPER_STATUS_UP,
                "{WIFI}",
                "Wi-Fi",
                "Intel(R) Wi-Fi 6 AX201",
            ),
            windows_adapter(
                IF_TYPE_FASTETHER,
                IF_OPER_STATUS_UP,
                "{FE}",
                "Ethernet 2",
                "Realtek PCIe FE Family Controller",
            ),
            windows_adapter(
                IF_TYPE_GIGABITETHERNET,
                IF_OPER_STATUS_UP,
                "{GBE}",
                "Ethernet 3",
                "Realtek Gaming GbE Family Controller",
            ),
            windows_adapter(
                IF_TYPE_IEEE8023AD_LAG,
                IF_OPER_STATUS_UP,
                "{TEAM}",
                "Ethernet Team",
                "Microsoft Network Adapter Multiplexor Driver",
            ),
            windows_adapter(
                IF_TYPE_ETHERNET_CSMACD,
                IF_OPER_STATUS_UP,
                "{VETH}",
                "vEthernet (Default Switch)",
                "Hyper-V Virtual Ethernet Adapter",
            ),
            windows_adapter(
                IF_TYPE_IEEE80211,
                IF_OPER_STATUS_UP,
                "{DIRECT}",
                "Local Area Connection* 1",
                "Microsoft Wi-Fi Direct Virtual Adapter",
            ),
        ];
        for adapter in &keep {
            assert!(
                windows_discovery_adapter_allowed(adapter),
                "dropped {} / {}",
                adapter.friendly_name,
                adapter.description
            );
        }

        let drop = [
            windows_adapter(
                IF_TYPE_PPP,
                IF_OPER_STATUS_UP,
                "{PPP}",
                "VPN",
                "WAN Miniport (PPTP)",
            ),
            windows_adapter(
                IF_TYPE_TUNNEL,
                IF_OPER_STATUS_UP,
                "{TUN}",
                "Teredo",
                "Microsoft Teredo Tunneling Adapter",
            ),
            windows_adapter(
                IF_TYPE_PROP_VIRTUAL,
                IF_OPER_STATUS_UP,
                "{VIRT}",
                "Tailscale",
                "Wintun Userspace Tunnel",
            ),
            windows_adapter(IF_TYPE_SLIP, IF_OPER_STATUS_UP, "{SLIP}", "SLIP", "SLIP"),
            windows_adapter(
                IF_TYPE_PROP_POINT2POINT_SERIAL,
                IF_OPER_STATUS_UP,
                "{P2P}",
                "Serial",
                "Point-to-point serial",
            ),
            windows_adapter(
                IF_TYPE_PPPMULTILINKBUNDLE,
                IF_OPER_STATUS_UP,
                "{ML}",
                "PPP",
                "PPP Multilink",
            ),
            windows_adapter(
                IF_TYPE_MPLS_TUNNEL,
                IF_OPER_STATUS_UP,
                "{MPLS}",
                "MPLS",
                "MPLS tunnel",
            ),
            windows_adapter(
                IF_TYPE_ETHERNET_CSMACD,
                IF_OPER_STATUS_UP,
                "{TAP}",
                "Ethernet 4",
                "TAP-Windows Adapter V9",
            ),
            windows_adapter(
                IF_TYPE_ETHERNET_CSMACD,
                IF_OPER_STATUS_UP,
                "{TAP32}",
                "Local Area Connection 2",
                "TAP-Win32 Adapter OAS",
            ),
            windows_adapter(
                IF_TYPE_ETHERNET_CSMACD,
                IF_OPER_STATUS_UP,
                "tapwin0",
                "Ethernet 5",
                "Realtek",
            ),
            windows_adapter(
                IF_TYPE_IEEE80211,
                IF_OPER_STATUS_UP,
                "{WG}",
                "WireGuard",
                "WireGuard Tunnel",
            ),
            windows_adapter(
                IF_TYPE_GIGABITETHERNET,
                IF_OPER_STATUS_UP,
                "WiNtUn",
                "Office",
                "Intel(R) Ethernet Connection",
            ),
            windows_adapter(
                IF_TYPE_ETHERNET_CSMACD,
                2,
                "{DOWN}",
                "Ethernet",
                "Intel(R) Ethernet Connection",
            ),
            windows_adapter(
                IF_TYPE_IEEE80211,
                5,
                "{DORMANT}",
                "Wi-Fi",
                "Intel(R) Wi-Fi 6 AX201",
            ),
            windows_adapter(
                IF_TYPE_ETHERNET_CSMACD,
                7,
                "{LL}",
                "Ethernet",
                "Intel(R) Ethernet Connection",
            ),
            windows_adapter(
                IF_TYPE_FASTETHER,
                4,
                "{UNK}",
                "Ethernet",
                "Realtek PCIe FE Family Controller",
            ),
            windows_adapter(
                IF_TYPE_ETHERNET_CSMACD,
                3,
                "{TEST}",
                "Ethernet",
                "Intel(R) Ethernet Connection",
            ),
            windows_adapter(
                IF_TYPE_GIGABITETHERNET,
                6,
                "{NP}",
                "Ethernet 3",
                "Realtek Gaming GbE Family Controller",
            ),
            windows_adapter(
                IF_TYPE_ETHERNET_CSMACD,
                0,
                "{NONE}",
                "Ethernet",
                "Intel(R) Ethernet Connection",
            ),
        ];
        for adapter in &drop {
            assert!(
                !windows_discovery_adapter_allowed(adapter),
                "kept {} / {} type {}",
                adapter.friendly_name,
                adapter.description,
                adapter.if_type
            );
        }

        let lan = "192.168.1.20".parse::<Ipv4Addr>().unwrap();
        assert!(windows_discovery_adapter_allowed(&keep[0]));
        let iface = udp::LanIface {
            name: "Ethernet".into(),
            index: 4,
            v4: vec![(lan, 24)],
            v6: vec![],
        };
        let plan = udp::discovery_bind_plan(std::slice::from_ref(&iface));
        assert!(plan_has(&plan, &udp::BindKey::Unicast(IpAddr::V4(lan))));
        assert_eq!(
            tcp_bind_addrs(&[IpAddr::V4(lan)], &[]),
            vec![SocketAddr::new(IpAddr::V4(lan), TCP_PORT)]
        );
        assert!(!windows_discovery_adapter_allowed(&drop[2]));
    }

    #[test]
    fn direct_address_support_does_not_expand_broadcast_discovery() {
        let reply = PeerDiscovery {
            cmd: "pong".into(),
            id: "123456789".into(),
            ..Default::default()
        };
        let challenge = fresh_challenge();
        for address in ["100.64.1.7:21119", "169.254.1.7:21119", "8.8.8.8:21119"] {
            assert!(discovered_peer(&reply, address.parse().unwrap(), "own", &challenge).is_none());
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
        let challenge = fresh_challenge();
        let (_, peer) = discovered_peer(&reply, source, "own", &challenge).unwrap();
        assert_eq!(peer.address, "192.168.1.7:21118");
        assert_eq!(peer.protocol, "rustdesk");
        reply.id = "bad/id".into();
        assert!(discovered_peer(&reply, source, "own", &challenge).is_none());
        reply.id = "123456789".into();
        assert!(discovered_peer(&reply, "8.8.8.8:1234".parse().unwrap(), "own", &challenge).is_none());
    }
    #[test]
    fn sealdesk_discovery_preserves_identity_and_never_downgrades() {
        let key = hex::encode([7u8; 32]);
        let (_public, secret) = node_keypair();
        let challenge = fresh_challenge();
        let request = (challenge.eph_public, challenge.nonce);
        let hint = sealdesk_hint(&key, true, Some(&request), Some(&secret));
        let mut reply = PeerDiscovery {
            cmd: "pong".into(),
            id: key.clone(),
            misc: serde_json::to_string(&hint).unwrap(),
            ..Default::default()
        };
        let source = "192.168.1.8:1234".parse().unwrap();
        let (_, peer) = discovered_peer(&reply, source, "own", &challenge).unwrap();
        assert_eq!(peer.address, format!("lan:192.168.1.8:{TCP_PORT}/{key}"));
        assert_eq!(peer.protocol, "sealdesk");
        assert!(discovered_peer(&reply, source, &key, &challenge).is_none());
        assert!(discovered_peer(&reply, source, "own", &fresh_challenge()).is_none());
        for misc in [
            "{",
            "{}",
            r#"{"version":1,"key":"bad","port":21118}"#,
            r#"{"version":3,"key":"bad","port":21118}"#,
        ] {
            reply.misc = misc.into();
            assert!(discovered_peer(&reply, source, "own", &challenge).is_none());
        }
    }

    fn legacy_version_1_reply(key: &str) -> PeerDiscovery {
        let mac = "ab".repeat(32);
        PeerDiscovery {
            cmd: "pong".into(),
            id: key.into(),
            hostname: "Old host".into(),
            misc: format!(
                r#"{{"version":1,"key":"{key}","port":{TCP_PORT},"nonce":"{}","mac":"{mac}","proof":"deadbeef"}}"#,
                hex::encode([1u8; 32])
            ),
            ..Default::default()
        }
    }

    #[test]
    fn version_1_and_unproved_replies_are_discarded() {
        let key = hex::encode([7u8; 32]);
        let (public, secret) = node_keypair();
        let challenge = fresh_challenge();
        let source = "192.168.1.8:1234".parse().unwrap();
        assert!(discovered_peer(&legacy_version_1_reply(&key), source, "own", &challenge).is_none());
        let request = (challenge.eph_public, challenge.nonce);
        let mut hint = sealdesk_hint(&key, true, Some(&request), Some(&secret));
        assert_eq!(hint.version, HINT_VERSION);
        assert_eq!(hint.node, hex::encode(public));
        let proved = PeerDiscovery {
            cmd: "pong".into(),
            hostname: "Desk".into(),
            misc: serde_json::to_string(&hint).unwrap(),
            ..Default::default()
        };
        let (_, peer) = discovered_peer(&proved, source, "own", &challenge).unwrap();
        assert_eq!(peer.protocol, PROTO_SEALDESK);
        assert!(!peer.proof.is_empty());
        assert!(plan_lan_upgrade(
            tc_addr(),
            &public,
            std::slice::from_ref(&peer),
            &lan_locals(),
            "00",
            &challenge
        )
        .is_some());
        hint.proof.clear();
        let unproved = PeerDiscovery {
            cmd: "pong".into(),
            misc: serde_json::to_string(&hint).unwrap(),
            ..Default::default()
        };
        assert!(discovered_peer(&unproved, source, "own", &challenge).is_none());
        hint.proof = proved_peer(&secret, &challenge, 7, "192.168.1.8", TCP_PORT, PROTO_SEALDESK).proof;
        hint.node = hex::encode([9u8; 32]);
        let wrong_node = PeerDiscovery {
            cmd: "pong".into(),
            misc: serde_json::to_string(&hint).unwrap(),
            ..Default::default()
        };
        assert!(discovered_peer(&wrong_node, source, "own", &challenge).is_none());
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

    #[test]
    fn typed_direct_addresses_parse_onto_the_lan_port() {
        let cases = [
            ("192.168.1.20", TCP_PORT),
            ("192.168.77.12:21118", 21118),
            ("192.168.0.1", TCP_PORT),
            ("192.168.255.254:21118", 21118),
            ("10.1.2.3", TCP_PORT),
            ("10.0.0.1", TCP_PORT),
            ("10.255.0.4:9", 9),
            ("10.255.255.254:9", 9),
            ("172.16.0.1", TCP_PORT),
            ("172.31.255.9", TCP_PORT),
            ("172.20.5.5:21119", 21119),
            ("172.31.255.254:21119", 21119),
            ("100.64.0.8", TCP_PORT),
            ("100.64.0.1", TCP_PORT),
            ("100.127.255.1:21118", 21118),
            ("100.127.255.254:21118", 21118),
            ("127.0.0.1", TCP_PORT),
            ("127.0.0.1:21118", 21118),
            ("169.254.1.1", TCP_PORT),
            ("169.254.255.1:21119", 21119),
            ("fd00::1", TCP_PORT),
            ("fc00::1", TCP_PORT),
            ("[fd00::1]:21118", 21118),
            ("[fd12::abcd]:9", 9),
            ("fe80::1", TCP_PORT),
            ("[fe80::1]:9", 9),
        ];
        for (sample, port) in cases {
            let addr = parse_direct_socket(sample).unwrap_or_else(|error| {
                panic!("{sample} should be a direct LAN target: {error}")
            });
            assert_eq!(addr.port(), port, "{sample}");
        }
        let denied = [
            "0.0.0.0",
            "0.0.0.0:21118",
            "::",
            "[::]",
            "[::]:21118",
            "224.0.0.1",
            "224.0.0.1:21118",
            "239.255.255.255",
            "ff02::1",
            "[ff02::1]:21118",
            "8.8.8.8",
            "8.8.8.8:53",
            "1.1.1.1",
            "172.15.255.1",
            "172.32.0.1",
            "100.63.255.255",
            "100.128.0.1",
            "255.255.255.255",
            "2001:db8::1",
            "[2001:db8::1]:443",
        ];
        for sample in denied {
            let error = parse_direct_socket(sample).unwrap_err();
            assert!(
                error.to_string().contains(DIRECT_ADDRESS_DENIED),
                "{sample}: {error}"
            );
        }
        assert!(parse_direct_socket("not-an-ip").is_err());
        assert!(parse_direct_socket("lan:192.168.1.1:21118/abcd").is_err());
        assert!(parse_direct_socket("192.168.1.1:0").is_err());
        assert!(parse_direct_socket("256.1.1.1").is_err());
    }

    #[tokio::test]
    async fn typed_ip_handshake_is_encrypted_without_a_pinned_identity() {
        let (_lock, _reset) = lan_test_env();
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
            assert!(stream.is_secured());
            assert_eq!(receive(&mut stream).await.unwrap(), b"desktop payload");
            stream
                .send_raw(b"encrypted reply".to_vec())
                .await
                .unwrap();
        });
        let target = format!("127.0.0.1:{}", addr.port());
        let mut client = connect_direct(&target).await.unwrap();
        assert!(client.is_secured());
        client
            .send_raw(b"desktop payload".to_vec())
            .await
            .unwrap();
        assert_eq!(receive(&mut client).await.unwrap(), b"encrypted reply");
        server.await.unwrap();

        // The old typed-IP dial opened TCP and sent nothing. That is still rejected.
        set_preface_wait_for_test(Some(Duration::from_millis(40)));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let socket = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (accepted, _) = listener.accept().await.unwrap();
        let local = accepted.local_addr().unwrap();
        let mut server = Stream::from(accepted, local);
        let error = accept_shared(&mut server).await.unwrap_err();
        assert!(error.to_string().contains("disabled"));
        drop(socket);
    }

    #[tokio::test]
    async fn pinned_lan_key_change_is_rejected_and_not_an_unverified_direct_session() {
        let (_lock, _reset) = lan_test_env();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (public, _) = identity().unwrap();
        let real = hex::encode(public.0);
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let local = socket.local_addr().unwrap();
            let mut stream = Stream::from(socket, local);
            accept_shared(&mut stream).await
        });
        let (wrong, _) = sign::gen_keypair();
        let pinned = format!("lan:{addr}/{}", hex::encode(wrong.0));
        match connect_pinned_or_typed(&pinned).await {
            Ok((_, kind, _)) => panic!("pinned key change returned {kind}"),
            Err(error) => assert!(
                error.to_string().contains("LAN host identity changed"),
                "{error}"
            ),
        }
        let accepted = server.await.unwrap();
        assert!(
            accepted.is_err(),
            "a changed lan: key completed a session: {accepted:?}"
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let local = socket.local_addr().unwrap();
            let mut stream = Stream::from(socket, local);
            assert!(accept_shared(&mut stream).await.unwrap().is_some());
        });
        let (stream, kind, _) = connect_pinned_or_typed(&addr.to_string()).await.unwrap();
        assert_eq!(kind, UNVERIFIED_DIRECT);
        assert!(stream.is_secured());
        assert!(!shows_verified_identity(kind, stream.is_secured()));
        drop(stream);
        server.await.unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let local = socket.local_addr().unwrap();
            let mut stream = Stream::from(socket, local);
            assert!(accept_shared(&mut stream).await.unwrap().is_some());
        });
        let (stream, kind, _) =
            connect_pinned_or_typed(&format!("lan:{addr}/{real}")).await.unwrap();
        assert_eq!(kind, "LAN");
        assert!(shows_verified_identity(kind, stream.is_secured()));
        drop(stream);
        server.await.unwrap();
    }

    #[test]
    fn direct_host_address_and_fingerprint_are_stable() {
        assert_eq!(
            normalize_direct_address("192.168.1.20").unwrap(),
            "192.168.1.20:21118"
        );
        assert_eq!(
            normalize_direct_address("192.168.1.20:21118").unwrap(),
            "192.168.1.20:21118"
        );
        assert_eq!(
            normalize_direct_address("192.168.1.20:9").unwrap(),
            "192.168.1.20:9"
        );
        assert_eq!(
            normalize_direct_address("fd00::1").unwrap(),
            "[fd00::1]:21118"
        );
        assert_eq!(
            normalize_direct_address("[fd00::1]:9").unwrap(),
            "[fd00::1]:9"
        );
        assert_eq!(
            fingerprint_prefix(&"0123456789abcdef".repeat(4)),
            "0123 4567 89ab cdef"
        );
    }

    fn memory_direct_hosts() -> impl Drop {
        {
            let mut state = direct_host_state().lock().unwrap();
            state.memory = Some(BTreeMap::new());
        }
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                let mut state = direct_host_state().lock().unwrap();
                state.memory = None;
            }
        }
        Reset
    }

    #[tokio::test]
    async fn typed_direct_records_a_confirmed_host_and_warns_when_it_changes() {
        let (_lock, _reset) = lan_test_env();
        let _memory = memory_direct_hosts();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target = listener.local_addr().unwrap().to_string();
        let address = normalize_direct_address(&target).unwrap();
        assert_eq!(address, target);
        let (public, _) = identity().unwrap();
        let real = hex::encode(public.0);
        let server = tokio::spawn(async move {
            for _ in 0..4 {
                let (socket, _) = listener.accept().await.unwrap();
                let local = socket.local_addr().unwrap();
                let mut stream = Stream::from(socket, local);
                accept_shared(&mut stream).await.unwrap();
                let _ = receive(&mut stream).await;
            }
        });

        let (stream, kind, presented) = connect_pinned_or_typed(&target).await.unwrap();
        assert_eq!(kind, UNVERIFIED_DIRECT);
        let presented = presented.unwrap();
        assert_eq!(presented, real);
        assert!(stream.is_secured());
        assert!(!shows_verified_identity(kind, stream.is_secured()));
        let prompt = direct_confirm_prompt(&target, &presented).unwrap();
        assert_eq!(prompt.title, "Unverified identity");
        assert_eq!(prompt.msg_type, "insecure-connection-nocancel-hasclose");
        assert_eq!(prompt.text, fingerprint_prefix(&real));
        assert!(!prompt.text.contains(&real));
        assert!(stored_direct_host(&address).is_none());
        drop(stream);
        accept_direct_host(&target, &presented);
        assert_eq!(stored_direct_host(&address).as_deref(), Some(real.as_str()));

        let (stream, kind, presented) = connect_pinned_or_typed(&target).await.unwrap();
        assert_eq!(kind, "LAN");
        assert!(presented.is_none());
        assert!(stream.is_secured());
        assert!(shows_verified_identity(kind, stream.is_secured()));
        assert!(direct_confirm_prompt(&target, &real).is_none());
        drop(stream);

        let wrong = hex::encode([0xab; 32]);
        assert_ne!(wrong, real);
        remember_direct_host(&address, &wrong).unwrap();
        let (stream, kind, presented) = connect_pinned_or_typed(&target).await.unwrap();
        assert_eq!(kind, UNVERIFIED_DIRECT);
        let presented = presented.unwrap();
        assert_eq!(presented, real);
        assert!(!shows_verified_identity(kind, stream.is_secured()));
        let prompt = direct_confirm_prompt(&target, &presented).unwrap();
        assert_eq!(prompt.title, "Host identity changed");
        assert_eq!(prompt.msg_type, "insecure-connection-changed-error");
        assert_eq!(
            prompt.text,
            format!(
                "{}\n→\n{}",
                fingerprint_prefix(&wrong),
                fingerprint_prefix(&real)
            )
        );
        drop(stream);
        assert_eq!(stored_direct_host(&address).as_deref(), Some(wrong.as_str()));
        accept_direct_host(&target, &presented);
        assert_eq!(stored_direct_host(&address).as_deref(), Some(real.as_str()));

        let (stream, kind, _) = connect_pinned_or_typed(&target).await.unwrap();
        assert_eq!(kind, "LAN");
        assert!(shows_verified_identity(kind, stream.is_secured()));
        assert!(direct_confirm_prompt(&target, &real).is_none());
        drop(stream);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn same_address_confirmations_record_only_their_own_host_key() {
        let (_lock, _reset) = lan_test_env();
        let _memory = memory_direct_hosts();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target = listener.local_addr().unwrap().to_string();
        let address = normalize_direct_address(&target).unwrap();
        let (public, _) = identity().unwrap();
        let key_a = hex::encode(public.0);
        let key_b = hex::encode([0x22; 32]);
        assert_ne!(key_a, key_b);
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (socket, _) = listener.accept().await.unwrap();
                let local = socket.local_addr().unwrap();
                let mut stream = Stream::from(socket, local);
                accept_shared(&mut stream).await.unwrap();
                let _ = receive(&mut stream).await;
            }
        });

        let (stream_a, kind_a, presented_a) = connect_pinned_or_typed(&target).await.unwrap();
        assert_eq!(kind_a, UNVERIFIED_DIRECT);
        let key_a = presented_a.unwrap();
        let prompt_a = direct_confirm_prompt(&target, &key_a).unwrap();
        let prompt_b = direct_confirm_prompt(&target, &key_b).unwrap();
        assert_eq!(prompt_a.title, "Unverified identity");
        assert_eq!(prompt_b.title, "Unverified identity");
        assert_eq!(prompt_a.text, fingerprint_prefix(&key_a));
        assert_eq!(prompt_b.text, fingerprint_prefix(&key_b));
        assert_ne!(prompt_a.text, prompt_b.text);
        accept_direct_host(&target, &key_a);
        assert_eq!(stored_direct_host(&address).as_deref(), Some(key_a.as_str()));
        assert!(direct_confirm_prompt(&target, &key_b).is_some());
        drop(stream_a);

        let (stream_again, kind_again, _) = connect_pinned_or_typed(&target).await.unwrap();
        assert_eq!(kind_again, "LAN");
        assert!(shows_verified_identity(kind_again, stream_again.is_secured()));
        drop(stream_again);

        accept_direct_host(&target, &key_b);
        assert_eq!(stored_direct_host(&address).as_deref(), Some(key_b.as_str()));
        assert!(direct_confirm_prompt(&target, &key_b).is_none());
        let prompt_a = direct_confirm_prompt(&target, &key_a).unwrap();
        assert_eq!(prompt_a.title, "Host identity changed");
        assert_eq!(
            prompt_a.text,
            format!(
                "{}\n→\n{}",
                fingerprint_prefix(&key_b),
                fingerprint_prefix(&key_a)
            )
        );
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
            proof: String::new(),
            nonce: String::new(),
        }
    }

    fn node_keypair() -> ([u8; 32], [u8; 32]) {
        hbb_common::sodiumoxide::init().expect("sodium");
        let (public, secret) = box_::gen_keypair();
        (public.0, secret.0)
    }

    fn fresh_challenge() -> PossessionChallenge {
        PossessionChallenge::generate().expect("challenge")
    }

    fn tc_addr() -> &'static str {
        "tcabcdefghijklmnopqrstuvwxyz0123456789ABCD"
    }

    fn proved_peer(
        node_secret: &[u8; 32],
        challenge: &PossessionChallenge,
        lan_byte: u8,
        ip: &str,
        port: u16,
        protocol: &str,
    ) -> Peer {
        let lan_public = [lan_byte; 32];
        let key = hex::encode(lan_public);
        let proof = seal_possession(
            node_secret,
            &challenge.eph_public,
            &challenge.nonce,
            &lan_public,
            port,
            protocol,
        )
        .expect("seal");
        Peer {
            name: "Desk".into(),
            address: format!("lan:{ip}:{port}/{key}"),
            ip: ip.into(),
            protocol: protocol.into(),
            platform: "Linux".into(),
            tailcat: String::new(),
            proof,
            nonce: hex::encode(challenge.nonce),
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
        let tc = tc_addr();
        let (public, secret) = node_keypair();
        let challenge = fresh_challenge();
        let peer = proved_peer(&secret, &challenge, 9, "192.168.1.9", TCP_PORT, PROTO_SEALDESK);
        let address = peer.address.clone();
        let peers = vec![peer];
        let upgrade =
            plan_lan_upgrade(tc, &public, &peers, &lan_locals(), "00", &challenge).unwrap();
        assert_eq!(upgrade.address, address);
        assert!(is_encrypted_lan_channel(&upgrade.address));
        assert_eq!(transport_after_lan_error("connection refused"), "tailcat");
        assert_eq!(
            transport_after_lan_error("LAN host identity changed"),
            "tailcat"
        );
        assert_eq!(transport_after_lan_error("identity check failed"), "tailcat");
        assert_eq!(transport_after_lan_error("timed out"), "tailcat");
        let other = LocalNet {
            addr: "10.0.0.4".parse().unwrap(),
            prefix: 24,
        };
        assert!(plan_lan_upgrade(tc, &public, &peers, &[other], "00", &challenge).is_none());
    }

    #[test]
    fn plaintext_compat_is_never_an_encrypted_lan_channel() {
        let tc = tc_addr();
        let (public, _) = node_keypair();
        let challenge = fresh_challenge();
        let plain = "192.168.1.9:21118";
        assert!(!is_encrypted_lan_channel(plain));
        assert!(!is_encrypted_lan_channel("tcp:192.168.1.9:21118"));
        let peers = vec![
            sample_peer("rustdesk", plain, "192.168.1.9", tc),
            sample_peer("sealdesk", plain, "192.168.1.9", tc),
        ];
        assert!(plan_lan_upgrade(tc, &public, &peers, &lan_locals(), "00", &challenge).is_none());
        assert!(plan_lan_upgrade(plain, &public, &peers, &lan_locals(), "00", &challenge).is_none());
    }

    #[test]
    fn possession_proof_rejects_a_peer_without_the_node_private_key() {
        let tc = tc_addr();
        let (public, secret) = node_keypair();
        let (_other_public, other_secret) = node_keypair();
        let challenge = fresh_challenge();
        let forged = proved_peer(
            &other_secret,
            &challenge,
            3,
            "192.168.1.9",
            TCP_PORT,
            PROTO_SEALDESK,
        );
        assert!(plan_lan_upgrade(tc, &public, &[forged], &lan_locals(), "00", &challenge).is_none());
        let real = proved_peer(&secret, &challenge, 9, "192.168.1.8", TCP_PORT, PROTO_SEALDESK);
        assert!(
            plan_lan_upgrade(tc, &public, std::slice::from_ref(&real), &lan_locals(), "00", &challenge)
                .is_some()
        );
        let mut swapped = real.clone();
        let attacker = hex::encode([4u8; 32]);
        swapped.address = format!("lan:192.168.1.9:{TCP_PORT}/{attacker}");
        swapped.ip = "192.168.1.9".into();
        assert!(
            plan_lan_upgrade(
                tc,
                &public,
                std::slice::from_ref(&swapped),
                &lan_locals(),
                "00",
                &challenge
            )
            .is_none()
        );
        let mut invented = swapped;
        invented.proof = hex::encode([0u8; 48]);
        assert!(plan_lan_upgrade(tc, &public, &[invented], &lan_locals(), "00", &challenge).is_none());
        assert_eq!(transport_after_lan_error("identity check failed"), "tailcat");
    }

    #[test]
    fn possession_proof_rejects_a_replayed_response() {
        let tc = tc_addr();
        let (public, secret) = node_keypair();
        let first = fresh_challenge();
        let second = fresh_challenge();
        let peer = proved_peer(&secret, &first, 9, "192.168.1.9", TCP_PORT, PROTO_SEALDESK);
        assert!(plan_lan_upgrade(tc, &public, std::slice::from_ref(&peer), &lan_locals(), "00", &second).is_none());
        let mut rewritten = peer.clone();
        rewritten.nonce = hex::encode(second.nonce);
        assert!(
            plan_lan_upgrade(
                tc,
                &public,
                std::slice::from_ref(&rewritten),
                &lan_locals(),
                "00",
                &second
            )
            .is_none()
        );
        assert!(plan_lan_upgrade(tc, &public, std::slice::from_ref(&peer), &lan_locals(), "00", &first).is_some());
    }

    #[test]
    fn possession_proof_rejects_a_tampered_port() {
        let tc = tc_addr();
        let (public, secret) = node_keypair();
        let challenge = fresh_challenge();
        let peer = proved_peer(&secret, &challenge, 9, "192.168.1.9", TCP_PORT, PROTO_SEALDESK);
        let mut tampered = peer.clone();
        tampered.address = format!("lan:192.168.1.9:21117/{}", hex::encode([9u8; 32]));
        assert!(
            plan_lan_upgrade(
                tc,
                &public,
                std::slice::from_ref(&tampered),
                &lan_locals(),
                "00",
                &challenge
            )
            .is_none()
        );
        let lan = [9u8; 32];
        assert!(!open_possession(
            &public,
            &challenge.secret,
            &challenge.eph_public,
            &challenge.nonce,
            &lan,
            21117,
            PROTO_SEALDESK,
            &peer.proof,
        ));
        assert!(open_possession(
            &public,
            &challenge.secret,
            &challenge.eph_public,
            &challenge.nonce,
            &lan,
            TCP_PORT,
            PROTO_SEALDESK,
            &peer.proof,
        ));
    }

    #[test]
    fn possession_proof_rejects_a_tampered_protocol() {
        let tc = tc_addr();
        let (public, secret) = node_keypair();
        let challenge = fresh_challenge();
        let lan = [6u8; 32];
        let proof = seal_possession(
            &secret,
            &challenge.eph_public,
            &challenge.nonce,
            &lan,
            TCP_PORT,
            PROTO_SEALDESK,
        )
        .expect("seal");
        assert!(open_possession(
            &public,
            &challenge.secret,
            &challenge.eph_public,
            &challenge.nonce,
            &lan,
            TCP_PORT,
            PROTO_SEALDESK,
            &proof,
        ));
        assert!(!open_possession(
            &public,
            &challenge.secret,
            &challenge.eph_public,
            &challenge.nonce,
            &lan,
            TCP_PORT,
            PROTO_COMPAT,
            &proof,
        ));
        let compat = seal_possession(
            &secret,
            &challenge.eph_public,
            &challenge.nonce,
            &lan,
            TCP_PORT,
            PROTO_COMPAT,
        )
        .expect("seal");
        assert!(!open_possession(
            &public,
            &challenge.secret,
            &challenge.eph_public,
            &challenge.nonce,
            &lan,
            TCP_PORT,
            PROTO_SEALDESK,
            &compat,
        ));
        let mut peer = proved_peer(&secret, &challenge, 6, "192.168.1.9", TCP_PORT, PROTO_SEALDESK);
        peer.protocol = PROTO_COMPAT.into();
        assert!(plan_lan_upgrade(tc, &public, &[peer], &lan_locals(), "00", &challenge).is_none());
    }

    #[test]
    fn discovery_request_hides_the_ephemeral_secret() {
        let challenge = fresh_challenge();
        let misc = challenge.ping_misc();
        assert!(!misc.contains(&hex::encode(*challenge.secret)));
        assert!(misc.contains(&hex::encode(challenge.eph_public)));
        assert!(misc.contains(&hex::encode(challenge.nonce)));
        let parsed: PingBody = serde_json::from_str(&misc).unwrap();
        assert_eq!(parsed.domain.as_bytes(), DOMAIN);
    }

    #[test]
    fn discovery_reply_omits_the_tailcat_credential() {
        let tc = tc_addr();
        let (public, secret) = node_keypair();
        let challenge = fresh_challenge();
        let key = hex::encode([7u8; 32]);
        let request = (challenge.eph_public, challenge.nonce);
        let hint = sealdesk_hint(&key, true, Some(&request), Some(&secret));
        let misc = serde_json::to_string(&hint).unwrap();
        assert!(!misc.contains(tc));
        assert!(!misc.contains("tailcat"));
        assert!(!misc.contains("privkey"));
        let reply = PeerDiscovery {
            cmd: "pong".into(),
            id: key.clone(),
            misc: misc.clone(),
            ..Default::default()
        };
        let (_, peer) =
            discovered_peer(&reply, "192.168.1.8:99".parse().unwrap(), "own", &challenge)
                .unwrap();
        let listed = serde_json::to_string(&peer).unwrap();
        assert!(!listed.contains(tc));
        assert!(peer.tailcat.is_empty());
        let upgrade =
            plan_lan_upgrade(tc, &public, &[peer], &lan_locals(), "00", &challenge).unwrap();
        assert!(upgrade.address.contains(&key));
        let mut forged = serde_json::from_str::<serde_json::Value>(&misc).unwrap();
        forged["tailcat"] = serde_json::Value::String(tc.into());
        forged["proof"] = serde_json::Value::String(hex::encode([1u8; 32]));
        let forged_reply = PeerDiscovery {
            cmd: "pong".into(),
            id: key,
            misc: forged.to_string(),
            ..Default::default()
        };
        assert!(discovered_peer(
            &forged_reply,
            "192.168.1.9:99".parse().unwrap(),
            "own",
            &challenge,
        )
        .is_none());
        assert!(plan_lan_upgrade(tc, &public, &[], &lan_locals(), "00", &challenge).is_none());
        assert_eq!(transport_after_lan_error("forged discovery"), "tailcat");
    }

    #[test]
    fn direct_off_does_not_advertise_tcp_or_offer_an_upgrade() {
        let tc = tc_addr();
        let (public, secret) = node_keypair();
        let challenge = fresh_challenge();
        let key = hex::encode([8u8; 32]);
        let request = (challenge.eph_public, challenge.nonce);
        let hint = sealdesk_hint(&key, false, Some(&request), Some(&secret));
        assert_eq!(hint.port, advertised_tcp_port(false));
        assert!(hint.proof.is_empty());
        let misc = serde_json::to_string(&hint).unwrap();
        assert!(!misc.contains(tc));
        let reply = PeerDiscovery {
            cmd: "pong".into(),
            id: key,
            misc,
            ..Default::default()
        };
        assert!(discovered_peer(
            &reply,
            "192.168.1.8:99".parse().unwrap(),
            "own",
            &challenge
        )
        .is_none());
        assert!(plan_lan_upgrade(tc, &public, &[], &lan_locals(), "00", &challenge).is_none());
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
    fn direct_cgnat_is_bound_for_typed_ip_but_not_discovered() {
        assert!(is_cgnat_v4("100.64.0.1".parse().unwrap()));
        assert!(is_cgnat_v4("100.127.255.254".parse().unwrap()));
        assert!(!is_cgnat_v4("100.63.255.255".parse().unwrap()));
        assert!(!is_cgnat_v4("100.128.0.1".parse().unwrap()));
        assert!(!is_cgnat_v4("192.168.1.1".parse().unwrap()));
        assert!(!is_lan_bind_v4("100.64.0.8".parse().unwrap()));
        assert!(is_lan_bind_v4("10.1.2.3".parse().unwrap()));
        assert!(is_lan_bind_v4("192.168.1.20".parse().unwrap()));

        let enumerated = vec![
            "192.168.1.20".parse().unwrap(),
            "10.9.8.7".parse().unwrap(),
            "8.8.8.8".parse().unwrap(),
        ];
        // A Tailscale address that discovery enumeration never handed us.
        let bound = tcp_bind_with_direct_cgnat(
            &enumerated,
            &[],
            &[
                "100.64.8.9".parse().unwrap(),
                "8.8.8.8".parse().unwrap(),
                "100.128.0.1".parse().unwrap(),
            ],
        );
        let ips: Vec<_> = bound.iter().map(|addr| addr.to_string()).collect();
        assert_eq!(
            ips,
            vec![
                format!("10.9.8.7:{TCP_PORT}"),
                format!("100.64.8.9:{TCP_PORT}"),
                format!("192.168.1.20:{TCP_PORT}"),
            ]
        );
        assert!(!udp::udp_ip_allowed("100.64.8.9".parse().unwrap()));
        assert!(udp::udp_ip_allowed("192.168.1.20".parse().unwrap()));
        assert!(udp::udp_ip_allowed("10.9.8.7".parse().unwrap()));

        let payload = r#"[
            {"name":"tun0","flags":9,"addrs":["100.64.5.6/32","10.8.0.2/24"]},
            {"name":"wlan0","flags":1,"addrs":["192.168.1.5/24","8.8.8.8/32","100.64.1.2:21118"]},
            {"name":"down0","flags":0,"addrs":["100.64.9.9/32"]},
            {"name":"lo","flags":5,"addrs":["100.64.0.1/32"]}
        ]"#;
        assert_eq!(
            cgnat_v4_from_interface_json(payload),
            vec!["100.64.5.6".parse::<Ipv4Addr>().unwrap()]
        );
        let with_port = tcp_bind_addrs(
            &[
                "100.64.12.9".parse().unwrap(),
                "192.168.0.4".parse().unwrap(),
                "10.0.0.5".parse().unwrap(),
            ],
            &[],
        );
        assert!(with_port.iter().all(|addr| addr.port() == TCP_PORT));
    }

    #[test]
    fn android_empty_enum_binds_wifi_fallback_and_cgnat() {
        assert!(is_cgnat_v4("100.64.0.0".parse().unwrap()));
        assert!(is_cgnat_v4("100.127.255.255".parse().unwrap()));
        assert!(!is_cgnat_v4("100.63.255.255".parse().unwrap()));
        assert!(!is_cgnat_v4("100.128.0.0".parse().unwrap()));

        let wifi: Ipv4Addr = "192.168.1.20".parse().unwrap();
        let cgnat: Ipv4Addr = "100.64.8.9".parse().unwrap();
        let outside = [
            "100.63.255.255".parse().unwrap(),
            "100.128.0.0".parse().unwrap(),
        ];
        let bound = tcp_bind_with_direct_cgnat(&[], &[wifi], &[cgnat, outside[0], outside[1]]);
        let ips: Vec<_> = bound.iter().map(|addr| addr.to_string()).collect();
        assert_eq!(
            ips,
            vec![
                format!("100.64.8.9:{TCP_PORT}"),
                format!("192.168.1.20:{TCP_PORT}"),
            ]
        );
        let listed = compat_share_addrs(&[], &[cgnat, outside[0], outside[1]], &[wifi]);
        assert_eq!(
            listed,
            vec![
                format!("100.64.8.9:{TCP_PORT}"),
                format!("192.168.1.20:{TCP_PORT}"),
            ]
        );

        let nonempty = tcp_bind_with_direct_cgnat(
            &["10.1.2.3".parse().unwrap()],
            &[wifi],
            &[cgnat],
        );
        let nonempty_ips: Vec<_> = nonempty.iter().map(|addr| addr.ip()).collect();
        assert!(nonempty_ips.contains(&"10.1.2.3".parse().unwrap()));
        assert!(nonempty_ips.contains(&IpAddr::V4(cgnat)));
        assert!(!nonempty_ips.contains(&IpAddr::V4(wifi)));
        let listed_lan = compat_share_addrs(&["10.1.2.3".parse().unwrap()], &[cgnat], &[wifi]);
        assert!(listed_lan.iter().any(|text| text.starts_with("10.1.2.3:")));
        assert!(listed_lan.iter().any(|text| text.starts_with("100.64.8.9:")));
        assert!(!listed_lan.iter().any(|text| text.starts_with("192.168.1.20:")));
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
        assert_eq!(
            tcp_bind_addrs(&unspecified, &[]),
            vec![SocketAddr::new("100.64.0.1".parse().unwrap(), TCP_PORT)]
        );
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

    fn lan_iface(name: &str, index: u32, v4: &[(&str, u8)], v6: &[(&str, u8)]) -> udp::LanIface {
        udp::LanIface {
            name: name.to_owned(),
            index,
            v4: v4
                .iter()
                .map(|(ip, prefix)| (ip.parse().unwrap(), *prefix))
                .collect(),
            v6: v6
                .iter()
                .map(|(ip, prefix)| (ip.parse().unwrap(), *prefix))
                .collect(),
        }
    }

    fn plan_has(plan: &[udp::BindTarget], key: &udp::BindKey) -> bool {
        plan.iter().any(|target| &target.key == key)
    }

    fn v6_multicast_owns_reply() -> bool {
        cfg!(any(windows, target_os = "macos", target_os = "ios"))
    }

    #[test]
    fn udp_bind_plan_skips_public_and_keeps_private_and_link_local() {
        let plan = udp::discovery_bind_plan(&[
            lan_iface(
                "eth0",
                2,
                &[
                    ("10.1.2.3", 8),
                    ("172.16.0.1", 12),
                    ("192.168.1.10", 24),
                    ("169.254.1.5", 16),
                    ("100.64.1.1", 10),
                    ("8.8.8.8", 24),
                    ("127.0.0.1", 8),
                    ("0.0.0.0", 0),
                ],
                &[
                    ("fc00::1", 7),
                    ("fd12::1", 64),
                    ("fe80::1", 64),
                    ("2001:db8::1", 64),
                    ("::1", 128),
                    ("::", 0),
                ],
            ),
            lan_iface("lo", 1, &[("127.0.0.1", 8)], &[("::1", 128)]),
        ]);
        for ip in ["10.1.2.3", "172.16.0.1", "192.168.1.10", "169.254.1.5"] {
            assert!(
                plan_has(&plan, &udp::BindKey::Unicast(ip.parse().unwrap())),
                "missing {ip}"
            );
        }
        for ip in ["fc00::1", "fd12::1", "fe80::1"] {
            let present = plan_has(&plan, &udp::BindKey::Unicast(ip.parse().unwrap()));
            // Windows and Darwin bind the V6Multicast socket to the link-local
            // reply, so that address is not also a unicast listener.
            if v6_multicast_owns_reply() && ip == "fe80::1" {
                assert!(!present, "multicast socket owns {ip}");
            } else {
                assert!(present, "missing {ip}");
            }
        }
        for ip in [
            "8.8.8.8",
            "100.64.1.1",
            "127.0.0.1",
            "0.0.0.0",
            "2001:db8::1",
            "::1",
            "::",
        ] {
            assert!(
                !plan_has(&plan, &udp::BindKey::Unicast(ip.parse().unwrap())),
                "kept {ip}"
            );
        }
        assert!(plan.iter().all(|target| match target.key {
            udp::BindKey::Unicast(ip) => !ip.is_unspecified() && udp::udp_ip_allowed(ip),
            udp::BindKey::Directed(ip) => ip != Ipv4Addr::BROADCAST && !ip.is_unspecified(),
            _ => true,
        }));
        assert!(plan_has(
            &plan,
            &udp::BindKey::Directed("192.168.1.255".parse().unwrap())
        ));
        assert!(plan_has(
            &plan,
            &udp::BindKey::Directed("169.254.255.255".parse().unwrap())
        ));
        assert!(!plan.iter().any(|target| matches!(
            target.key,
            udp::BindKey::Directed(ip) if ip == "8.8.8.255".parse::<Ipv4Addr>().unwrap()
        )));
        assert!(plan_has(
            &plan,
            &udp::BindKey::Limited {
                index: 2,
                name: "eth0".into(),
            }
        ));
        assert!(plan_has(&plan, &udp::BindKey::V4Multicast { index: 2 }));
        assert!(plan_has(&plan, &udp::BindKey::V6Multicast { index: 2 }));

        let fb = udp::fallback_listen(&plan, udp::ScopeError::DeviceDenied);
        let limited: Vec<_> = fb
            .plan
            .iter()
            .filter(|target| matches!(target.key, udp::BindKey::Limited { .. }))
            .collect();
        assert_eq!(limited.len(), 1);
        assert_eq!(
            limited[0].key,
            udp::BindKey::Limited {
                index: 0,
                name: String::new(),
            }
        );
        assert_eq!(fb.join_v4, vec!["10.1.2.3".parse::<Ipv4Addr>().unwrap()]);
        let two = udp::discovery_bind_plan(&[
            lan_iface("wlan0", 3, &[("192.168.4.9", 24)], &[]),
            lan_iface("eth0", 2, &[("10.0.0.5", 8)], &[]),
        ]);
        let two = udp::fallback_listen(&two, udp::ScopeError::DeviceDenied);
        assert_eq!(
            two.plan
                .iter()
                .filter(|target| matches!(target.key, udp::BindKey::Limited { .. }))
                .count(),
            1
        );
        assert_eq!(
            two.join_v4,
            vec![
                "10.0.0.5".parse::<Ipv4Addr>().unwrap(),
                "192.168.4.9".parse().unwrap(),
            ]
        );
        assert!(!fb
            .plan
            .iter()
            .any(|target| matches!(target.key, udp::BindKey::V4Multicast { .. })));
        assert!(fb.plan.iter().all(|target| match &target.key {
            udp::BindKey::Unicast(ip) => !ip.is_unspecified(),
            udp::BindKey::Directed(ip) => !ip.is_unspecified() && *ip != Ipv4Addr::BROADCAST,
            udp::BindKey::Limited { name, index } => name.is_empty() && *index == 0,
            udp::BindKey::V4Multicast { .. } => false,
            udp::BindKey::V6Multicast { .. } => true,
        }));
        assert!(fb.plan.iter().all(|target| !target.reply.is_unspecified()));
        let unicast_only = udp::fallback_listen(&plan, udp::ScopeError::FreebindFailed);
        assert!(unicast_only.join_v4.is_empty());
        assert!(unicast_only.plan.iter().all(|target| {
            matches!(target.key, udp::BindKey::Unicast(ip) if !ip.is_unspecified())
        }));
        assert!(plan_has(
            &unicast_only.plan,
            &udp::BindKey::Unicast("192.168.1.10".parse().unwrap())
        ));
        assert!(!plan_has(
            &unicast_only.plan,
            &udp::BindKey::Directed("192.168.1.255".parse().unwrap())
        ));
        assert!(udp::device_bind_denied(&std::io::Error::from_raw_os_error(
            hbb_common::libc::EPERM
        )));
        assert!(udp::device_bind_denied(&std::io::Error::from_raw_os_error(
            hbb_common::libc::EACCES
        )));
        assert!(!udp::device_bind_denied(
            &std::io::Error::from_raw_os_error(hbb_common::libc::ENODEV)
        ));

        let android = udp::ifaces_from_android_json(
            r#"[{"name":"wlan0","index":3,"flags":17,"addrs":["192.168.4.2/24","8.8.8.8/24","169.254.9.9/16","fe80::5/64","2001:db8::5/64"]},{"name":"lo","index":1,"flags":5,"addrs":["127.0.0.1/8"]}]"#,
        );
        let android = udp::discovery_bind_plan(&android);
        assert!(plan_has(
            &android,
            &udp::BindKey::Unicast("192.168.4.2".parse().unwrap())
        ));
        assert!(plan_has(
            &android,
            &udp::BindKey::Unicast("169.254.9.9".parse().unwrap())
        ));
        let link_local = plan_has(
            &android,
            &udp::BindKey::Unicast("fe80::5".parse().unwrap()),
        );
        if v6_multicast_owns_reply() {
            assert!(!link_local);
            assert!(plan_has(
                &android,
                &udp::BindKey::V6Multicast { index: 3 }
            ));
        } else {
            assert!(link_local);
        }
        assert!(!plan_has(
            &android,
            &udp::BindKey::Unicast("8.8.8.8".parse().unwrap())
        ));
        assert!(!plan_has(
            &android,
            &udp::BindKey::Unicast("127.0.0.1".parse().unwrap())
        ));
        assert!(!plan_has(
            &android,
            &udp::BindKey::Unicast("2001:db8::5".parse().unwrap())
        ));
    }

    #[test]
    fn udp_rebind_opens_added_addresses_and_closes_removed_ones() {
        let before =
            udp::discovery_bind_plan(&[lan_iface("eth0", 2, &[("192.168.1.10", 24)], &[])]);
        let after = udp::discovery_bind_plan(&[
            lan_iface("eth0", 2, &[("192.168.1.20", 24)], &[]),
            lan_iface("wlan0", 3, &[("10.0.0.2", 8)], &[]),
        ]);
        let before_keys: Vec<_> = before.iter().map(|target| target.key.clone()).collect();
        let after_keys: Vec<_> = after.iter().map(|target| target.key.clone()).collect();
        let (close, open) = udp::rebind_keys(&before_keys, &after_keys);
        assert!(close.contains(&udp::BindKey::Unicast("192.168.1.10".parse().unwrap())));
        assert!(!close.contains(&udp::BindKey::Unicast("192.168.1.20".parse().unwrap())));
        assert!(open.contains(&udp::BindKey::Unicast("192.168.1.20".parse().unwrap())));
        assert!(open.contains(&udp::BindKey::Unicast("10.0.0.2".parse().unwrap())));
        assert!(open.contains(&udp::BindKey::Directed("10.255.255.255".parse().unwrap())));
        assert!(!close.contains(&udp::BindKey::Directed("192.168.1.255".parse().unwrap())));
        assert!(!open.contains(&udp::BindKey::Directed("192.168.1.255".parse().unwrap())));
        let eth0_limited = udp::BindKey::Limited {
            index: 2,
            name: "eth0".into(),
        };
        assert!(!close.contains(&eth0_limited));
        assert!(!open.contains(&eth0_limited));
        assert!(open.contains(&udp::BindKey::Limited {
            index: 3,
            name: "wlan0".into(),
        }));
        let (close_all, open_none) = udp::rebind_keys(&after_keys, &[]);
        assert_eq!(close_all.len(), after_keys.len());
        assert!(open_none.is_empty());
    }

    fn ephemeral_port() -> u16 {
        UdpSocket::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    fn loopback_target(key: udp::BindKey) -> udp::BindTarget {
        udp::BindTarget {
            key,
            reply: IpAddr::V4(Ipv4Addr::LOCALHOST),
            index: 1,
            name: "lo".into(),
        }
    }

    #[tokio::test]
    async fn discovery_rebind_follows_the_loopback_address() {
        let port = ephemeral_port();
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let mut set = udp::ListenerSet::empty();
        let target = loopback_target(udp::BindKey::Unicast(IpAddr::V4(Ipv4Addr::LOCALHOST)));
        set.reconcile(vec![target.clone()], port, tx.clone());
        assert_eq!(set.sockets.len(), 1);
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        client
            .send_to(b"ping", (Ipv4Addr::LOCALHOST, port))
            .unwrap();
        let hit = tokio::time::timeout(Duration::from_millis(500), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(hit.bytes, b"ping");
        set.reconcile(Vec::new(), port, tx.clone());
        assert!(set.sockets.is_empty());
        tokio::time::sleep(Duration::from_millis(50)).await;
        client
            .send_to(b"gone", (Ipv4Addr::LOCALHOST, port))
            .unwrap();
        assert!(tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .is_err());
        set.reconcile(vec![target], port, tx);
        assert_eq!(set.sockets.len(), 1);
        client
            .send_to(b"back", (Ipv4Addr::LOCALHOST, port))
            .unwrap();
        let hit = tokio::time::timeout(Duration::from_millis(500), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(hit.bytes, b"back");
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn discovery_broadcast_and_multicast_roundtrip_on_loopback() {
        let port = ephemeral_port();
        let targets = vec![
            loopback_target(udp::BindKey::Unicast(IpAddr::V4(Ipv4Addr::LOCALHOST))),
            loopback_target(udp::BindKey::Directed("127.255.255.255".parse().unwrap())),
            loopback_target(udp::BindKey::Limited {
                index: 1,
                name: "lo".into(),
            }),
            loopback_target(udp::BindKey::V4Multicast { index: 1 }),
        ];
        let mut opened = Vec::new();
        for target in &targets {
            let socket = udp::open_target(target, port)
                .unwrap_or_else(|error| panic!("bind {}: {error}", target.key.label(port)));
            socket.set_nonblocking(true).unwrap();
            let ip = socket.local_addr().unwrap().ip();
            assert!(!ip.is_unspecified(), "{}", target.key.label(port));
            opened.push((target.key.clone(), socket));
        }
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        client.set_broadcast(true).unwrap();
        client.set_nonblocking(true).unwrap();
        let cases = [
            (
                udp::BindKey::Unicast(IpAddr::V4(Ipv4Addr::LOCALHOST)),
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
                b"uni".as_slice(),
            ),
            (
                udp::BindKey::Directed("127.255.255.255".parse().unwrap()),
                SocketAddr::new("127.255.255.255".parse().unwrap(), port),
                b"dir".as_slice(),
            ),
            (
                udp::BindKey::Limited {
                    index: 1,
                    name: "lo".into(),
                },
                SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), port),
                b"lim".as_slice(),
            ),
            (
                udp::BindKey::V4Multicast { index: 1 },
                SocketAddr::new(IpAddr::V4(udp::IPV4_ALL_HOSTS), port),
                b"mc".as_slice(),
            ),
        ];
        for (key, dest, payload) in cases {
            client.send_to(payload, dest).unwrap();
            let start = Instant::now();
            let mut found = None;
            while start.elapsed() < Duration::from_millis(400) && found.is_none() {
                for (bound, socket) in &opened {
                    let mut buf = [0u8; 64];
                    match socket.recv_from(&mut buf) {
                        Ok((n, from)) => {
                            found = Some((bound.clone(), buf[..n].to_vec(), from));
                            break;
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                        Err(error) => panic!("{error}"),
                    }
                }
                if found.is_none() {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
            let (bound, bytes, from) = found.unwrap_or_else(|| panic!("no reply for {payload:?}"));
            assert_eq!(bound, key, "{payload:?} arrived on the wrong socket");
            assert_eq!(bytes, payload);
            opened
                .iter()
                .find(|(candidate, _)| candidate == &key)
                .unwrap()
                .1
                .send_to(b"pong", from)
                .unwrap();
            let start = Instant::now();
            let mut pong = None;
            while start.elapsed() < Duration::from_millis(400) && pong.is_none() {
                let mut buf = [0u8; 64];
                match client.recv_from(&mut buf) {
                    Ok((n, src)) => pong = Some((buf[..n].to_vec(), src)),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("{error}"),
                }
            }
            let (bytes, src) = pong.expect("pong");
            assert_eq!(bytes, b"pong");
            assert_eq!(src.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
        }
    }

    #[test]
    fn discovery_ipv6_multicast_roundtrip_on_link_local_when_present() {
        let Some(iface) = udp::interfaces_from_default_net()
            .into_iter()
            .find(|iface| {
                iface.v6.iter().any(|(ip, _)| ip.is_unicast_link_local()) && iface.index != 0
            })
        else {
            eprintln!("skipping discovery_ipv6_multicast_roundtrip_on_link_local_when_present: no link-local IPv6 interface");
            return;
        };
        let ip = iface
            .v6
            .iter()
            .find(|(ip, _)| ip.is_unicast_link_local())
            .unwrap()
            .0;
        let port = ephemeral_port();
        let target = udp::BindTarget {
            key: udp::BindKey::V6Multicast { index: iface.index },
            reply: IpAddr::V6(ip),
            index: iface.index,
            name: iface.name,
        };
        let server = udp::open_target(&target, port).unwrap();
        server.set_nonblocking(true).unwrap();
        assert!(!server.local_addr().unwrap().ip().is_unspecified());
        let client = UdpSocket::bind(std::net::SocketAddrV6::new(ip, 0, 0, iface.index)).unwrap();
        client.set_nonblocking(true).unwrap();
        let dest = std::net::SocketAddrV6::new(udp::IPV6_ALL_NODES, port, 0, iface.index);
        client.send_to(b"v6", dest).unwrap();
        let start = Instant::now();
        let mut got = None;
        while start.elapsed() < Duration::from_millis(400) && got.is_none() {
            let mut buf = [0u8; 64];
            match server.recv_from(&mut buf) {
                Ok((n, from)) => got = Some((buf[..n].to_vec(), from)),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("{error}"),
            }
        }
        let (bytes, from) = got.expect("ipv6 multicast");
        assert_eq!(bytes, b"v6");
        server.send_to(b"pong6", from).unwrap();
        let start = Instant::now();
        let mut pong = None;
        while start.elapsed() < Duration::from_millis(400) && pong.is_none() {
            let mut buf = [0u8; 64];
            match client.recv_from(&mut buf) {
                Ok((n, src)) => pong = Some((buf[..n].to_vec(), src)),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("{error}"),
            }
        }
        let (bytes, src) = pong.expect("ipv6 pong");
        assert_eq!(bytes, b"pong6");
        let src_ip = src.ip();
        assert!(
            matches!(src_ip, IpAddr::V6(ip) if ip.is_unicast_link_local())
                || src_ip == IpAddr::V6(ip)
        );
    }

    #[cfg(unix)]
    #[test]
    fn udp_and_tcp_share_the_discovery_interface_filter() {
        use hbb_common::libc::{IFF_BROADCAST, IFF_POINTOPOINT, IFF_UP};

        let vpn = (IFF_UP | IFF_POINTOPOINT) as u32;
        let lan = (IFF_UP | IFF_BROADCAST) as u32;
        assert!(udp::lan_iface_if_kept(
            "tun0",
            8,
            vpn,
            vec![("10.8.0.2".parse().unwrap(), 24)],
            vec![],
        )
        .is_none());
        assert!(udp::lan_iface_if_kept(
            "wlan0",
            3,
            0,
            vec![("192.168.1.5".parse().unwrap(), 24)],
            vec![],
        )
        .is_none());
        let kept = udp::lan_iface_if_kept(
            "eth0",
            2,
            lan,
            vec![("192.168.1.10".parse().unwrap(), 24)],
            vec![],
        )
        .unwrap();
        let plan = udp::discovery_bind_plan(std::slice::from_ref(&kept));
        assert!(plan_has(
            &plan,
            &udp::BindKey::Unicast("192.168.1.10".parse().unwrap())
        ));
        assert!(plan.iter().all(|target| match target.key {
            udp::BindKey::Unicast(ip) => !ip.is_unspecified(),
            _ => true,
        }));
        let bound = tcp_bind_addrs(&[IpAddr::V4(kept.v4[0].0)], &[]);
        assert_eq!(
            bound,
            vec![SocketAddr::new("192.168.1.10".parse().unwrap(), TCP_PORT)]
        );
    }

    #[test]
    fn android_discovery_falls_back_when_netlink_is_empty_or_flags_are_zero() {
        // Empty, and the Unix flag filter's flags-0 result, are the same input:
        // no kept interface. Discovery still queries 255.255.255.255.
        let (ifaces, global) = udp::query_targets(&[], &[]);
        assert!(ifaces.is_empty());
        assert!(global);

        let json = udp::ifaces_from_android_json(
            r#"[{"name":"tun0","index":9,"flags":9,"addrs":["10.8.0.2/24"]},{"name":"wlan0","index":3,"flags":1,"addrs":["192.168.4.9/24","fe80::5/64"]}]"#,
        );
        assert_eq!(json.len(), 1);
        assert_eq!(json[0].name, "wlan0");
        let (ifaces, global) = udp::query_targets(&[], &json);
        assert!(!global);
        assert_eq!(ifaces.len(), 1);
        let plan = udp::discovery_bind_plan(ifaces);
        assert!(plan_has(
            &plan,
            &udp::BindKey::Unicast("192.168.4.9".parse().unwrap())
        ));
        assert!(!plan.iter().any(|target| matches!(
            target.key,
            udp::BindKey::Unicast(ip) if ip == "10.8.0.2".parse::<IpAddr>().unwrap()
        )));

        let tun_only = udp::ifaces_from_android_json(
            r#"[{"name":"tun0","flags":1,"addrs":["10.8.0.2/24"]},{"name":"ppp0","flags":1,"addrs":["10.9.0.2/24"]},{"name":"wg0","flags":1,"addrs":["10.7.0.2/24"]},{"name":"lo","flags":5,"addrs":["127.0.0.1/8"]}]"#,
        );
        assert!(tun_only.is_empty());
        let (ifaces, global) = udp::query_targets(&[], &tun_only);
        assert!(ifaces.is_empty());
        assert!(global);
    }
}
