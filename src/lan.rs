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
        crypto::{box_, scalarmult::curve25519, secretbox, sign},
        randombytes,
    },
    tcp, tokio, ResultType, Stream,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr, UdpSocket},
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
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Trusted {
    pub key: String,
    pub name: String,
    pub permissions: [bool; 8],
}
#[derive(Serialize, Deserialize)]
struct Hint {
    version: u32,
    key: String,
    port: u16,
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
    match tokio::time::timeout(Duration::from_millis(500), stream.next()).await {
        Err(_) => return Ok(None),
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
    let addresses: Vec<String> = if ready() && crate::tailcat::host::enabled() {
        default_net::get_interfaces().into_iter().flat_map(|iface| iface.ipv4)
            .filter(|ip| ip.addr.is_private())
            .map(|ip| SocketAddr::new(ip.addr.into(), TCP_PORT).to_string()).collect()
    } else { vec![] };
    serde_json::to_string(&addresses).unwrap_or_else(|_| "[]".into())
}
pub fn ready() -> bool {
    READY.load(Ordering::SeqCst)
}
pub async fn serve(server: crate::server::ServerPtr) {
    loop {
        while !crate::tailcat::host::enabled() {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        if let Err(error) = serve_round(server.clone()).await {
            log::warn!("LAN sharing stopped: {error}");
        }
        READY.store(false, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
async fn serve_round(server: crate::server::ServerPtr) -> ResultType<()> {
    let tcp = tcp::listen_any(TCP_PORT).await?;
    let udp =
        tokio::net::UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED, DISCOVERY_PORT)).await?;
    READY.store(true, Ordering::SeqCst);
    log::info!("LAN encrypted sharing ready");
    let mut buf = [0u8; 2048];
    let mut last = Instant::now();
    let mut replies = 0;
    loop {
        if !crate::tailcat::host::enabled() {
            return Ok(());
        }
        tokio::select! {
            _=tokio::time::sleep(Duration::from_millis(100))=>{},
            incoming=tcp.accept()=>{
                let (socket,addr)=incoming?;
                let local=socket.local_addr()?; let server=server.clone();
                tokio::spawn(async move {
                    if let Err(e)=crate::server::create_tcp_connection(server,Stream::from(socket,local),addr,false,crate::server::ConnectionMeta{via_lan:true,..Default::default()}).await { log::trace!("LAN connection ended: {e}"); }
                });
            },
            incoming=udp.recv_from(&mut buf)=>{
                let (n,addr)=incoming?; if !private(addr.ip()) { continue; }
                if last.elapsed()>=Duration::from_secs(1) { replies=0; last=Instant::now(); }
                if replies>=20 { continue; }
                let Ok(msg)=RendezvousMessage::parse_from_bytes(&buf[..n]) else { continue; };
                let Some(rendezvous_message::Union::PeerDiscovery(query))=msg.union else { continue; };
                if query.cmd!="ping" { continue; }
                let (key,_)=identity()?; let key=hex::encode(key.0); if query.id==key { continue; }
                let mut reply=RendezvousMessage::new();
                let (id,misc)=if query.misc==String::from_utf8_lossy(DOMAIN) || hex::decode(&query.id).map(|b|b.len()==32).unwrap_or(false) {
                    (key.clone(),serde_json::to_string(&Hint{version:1,key,port:TCP_PORT})?)
                } else {
                    // Original clients dial the discovered ID; advertise a direct endpoint, not a rendezvous ID.
                    let probe=UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED,0))?;
                    probe.connect(addr)?;
                    (SocketAddr::new(probe.local_addr()?.ip(),TCP_PORT).to_string(),String::new())
                };
                #[cfg(not(target_os = "ios"))]
                let platform = hbb_common::whoami::platform().to_string();
                #[cfg(target_os = "ios")]
                let platform = "iOS".to_owned();
                reply.set_peer_discovery(PeerDiscovery{cmd:"pong".into(),id,hostname:crate::whoami_hostname(),platform,misc,..Default::default()});
                udp.send_to(&reply.write_to_bytes()?,addr).await?; replies+=1;
            }
        }
    }
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
        if socket.set_broadcast(true).is_ok() && socket.set_nonblocking(true).is_ok()
            && socket.send_to(&bytes, (std::net::Ipv4Addr::BROADCAST, DISCOVERY_PORT)).is_ok()
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
    let (id, address, protocol) = if reply.misc.is_empty() {
        if !crate::common::is_valid_untrusted_peer_id(&reply.id) {
            return None;
        }
        let endpoint = SocketAddr::new(addr.ip(), TCP_PORT);
        (format!("rustdesk:{endpoint}"), endpoint.to_string(), "rustdesk")
    } else {
        // A malformed SealDesk identity must never fall back to plaintext RustDesk.
        let hint = serde_json::from_str::<Hint>(&reply.misc).ok()?;
        if hint.version != 1 || hint.key == own || hint.port == 0
            || hex::decode(&hint.key).map(|b| b.len() != 32).unwrap_or(true)
        {
            return None;
        }
        let endpoint = SocketAddr::new(addr.ip(), hint.port);
        (hint.key.clone(), format!("lan:{endpoint}/{}", hint.key), "sealdesk")
    };
    Some((id, Peer {
        name: reply.hostname.chars().filter(|c| !c.is_control()).take(128).collect(),
        ip: addr.ip().to_string(),
        address,
        protocol: protocol.to_owned(),
    }))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn direct_address_support_does_not_expand_broadcast_discovery() {
        let reply = PeerDiscovery { cmd: "pong".into(), id: "123456789".into(), ..Default::default() };
        for address in ["100.64.0.16:21119", "169.254.1.7:21119", "8.8.8.8:21119"] {
            assert!(discovered_peer(&reply, address.parse().unwrap(), "own").is_none());
        }
    }
    #[test]
    fn original_rustdesk_discovery_uses_source_ip_and_default_port() {
        let mut reply = PeerDiscovery { cmd: "pong".into(), id: "123456789".into(), hostname: "RustDesk host".into(), ..Default::default() };
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
        let mut reply = PeerDiscovery { cmd: "pong".into(), id: key.clone(), misc: serde_json::to_string(&Hint { version: 1, key: key.clone(), port: TCP_PORT }).unwrap(), ..Default::default() };
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
        let error = remember(&key, "old approval", [true; 8], 0).unwrap_err();
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
            assert!(accept_shared(&mut Stream::from(socket, local)).await.is_err());
        });
        let (wrong, _) = sign::gen_keypair();
        assert!(connect(&format!("lan:{addr}/{}", hex::encode(wrong.0)))
            .await
            .is_err());
        server.await.unwrap();
    }
    #[tokio::test]
    async fn shared_port_accepts_legacy_waiters_but_rejects_bad_encryption_prefaces() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let socket = tokio::net::TcpStream::connect(listener.local_addr().unwrap()).await.unwrap();
        let (accepted, _) = listener.accept().await.unwrap();
        let local = accepted.local_addr().unwrap();
        let mut server = Stream::from(accepted, local);
        assert!(accept_shared(&mut server).await.unwrap().is_none());
        assert!(!server.is_secured());
        let local = socket.local_addr().unwrap();
        let mut client = Stream::from(socket, local);
        client.send_raw(b"bad encryption preface".to_vec()).await.unwrap();
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
}
