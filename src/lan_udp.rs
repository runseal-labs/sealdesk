//! UDP 21119 listeners for LAN discovery.
//!
//! Sockets bind this host's private and link-local addresses only. Nothing
//! binds `0.0.0.0` or `::` as a global listen. On Linux a unicast bind does not
//! receive `255.255.255.255`, so each interface also binds its directed
//! broadcast, a device-scoped limited broadcast (`SO_BINDTODEVICE`), and the
//! all-hosts / all-nodes multicast groups. If `SO_BINDTODEVICE` is denied, one
//! `IP_FREEBIND` socket listens on `255.255.255.255` and `224.0.0.1` is joined
//! on the private unicast socket. That path still does not bind `0.0.0.0` or
//! `::`. macOS uses `IP_BOUND_IF` around an unspecified bind because that is
//! what receives broadcasts there. Windows delivers broadcasts to the interface
//! unicast socket, and cannot bind the IPv6 all-nodes group (`ff02::1` returns
//! `WSAEADDRNOTAVAIL`). That listener binds the interface address and joins the group.

use hbb_common::tokio;
use socket2::{Domain, SockAddr, Socket, Type};
use std::{
    collections::{BTreeMap, HashSet},
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6, UdpSocket},
    num::NonZeroU32,
    sync::Arc,
    time::Duration,
};

pub(crate) const IFACE_REFRESH: Duration = Duration::from_secs(2);
pub(crate) const IPV4_ALL_HOSTS: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 1);
pub(crate) const IPV6_ALL_NODES: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LanIface {
    pub name: String,
    pub index: u32,
    pub v4: Vec<(Ipv4Addr, u8)>,
    pub v6: Vec<(Ipv6Addr, u8)>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum BindKey {
    Unicast(IpAddr),
    Directed(Ipv4Addr),
    /// Limited broadcast. An empty `name` is one global `255.255.255.255`
    /// socket, used only when the interface has no name to scope it with.
    Limited {
        index: u32,
        name: String,
    },
    V4Multicast {
        index: u32,
    },
    V6Multicast {
        index: u32,
    },
}

impl BindKey {
    pub(crate) fn label(&self, port: u16) -> String {
        match self {
            BindKey::Unicast(ip) => SocketAddr::new(*ip, port).to_string(),
            BindKey::Directed(ip) => format!("{ip}:{port}"),
            BindKey::Limited { name, .. } => {
                if name.is_empty() {
                    format!("255.255.255.255:{port}")
                } else {
                    format!("255.255.255.255%{name}:{port}")
                }
            }
            BindKey::V4Multicast { index } => format!("224.0.0.1%{index}:{port}"),
            BindKey::V6Multicast { index } => format!("[ff02::1%{index}]:{port}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BindTarget {
    pub key: BindKey,
    pub reply: IpAddr,
    pub index: u32,
    pub name: String,
}

/// `SO_BINDTODEVICE` was denied (EPERM/EACCES), or the freebind-only listen failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScopeError {
    DeviceDenied,
    FreebindFailed,
}

/// Listeners to open after [`ScopeError`]. `join_v4` is joined on the matching
/// unicast socket; there is no second, device-scoped multicast bind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FallbackListen {
    pub plan: Vec<BindTarget>,
    pub join_v4: Vec<Ipv4Addr>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScopeMode {
    TryDevice,
    Freebind,
    UnicastOnly,
}

/// EPERM or EACCES from `SO_BINDTODEVICE`. Other errors must not widen the listen.
pub(crate) fn device_bind_denied(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(code) if code == hbb_common::libc::EPERM || code == hbb_common::libc::EACCES
    )
}

fn uses_bind_device(target: &BindTarget) -> bool {
    match &target.key {
        BindKey::Limited { name, .. } => !name.is_empty(),
        BindKey::V4Multicast { .. } => !target.name.is_empty(),
        _ => false,
    }
}

fn unspecified_key(key: &BindKey) -> bool {
    match key {
        BindKey::Unicast(ip) => ip.is_unspecified(),
        BindKey::Directed(ip) => ip.is_unspecified() || *ip == Ipv4Addr::BROADCAST,
        _ => false,
    }
}

/// Collapse device-scoped listens.
///
/// `DeviceDenied` keeps a single `255.255.255.255` socket (empty interface name,
/// not `0.0.0.0`) and records private addresses for `join_multicast_v4`.
/// `FreebindFailed` keeps unicast sockets only.
pub(crate) fn fallback_listen(plan: &[BindTarget], failure: ScopeError) -> FallbackListen {
    if failure == ScopeError::FreebindFailed {
        return FallbackListen {
            plan: plan
                .iter()
                .filter(|target| matches!(target.key, BindKey::Unicast(ip) if !ip.is_unspecified()))
                .cloned()
                .collect(),
            join_v4: Vec::new(),
        };
    }
    let mut kept = Vec::new();
    let mut join_v4 = Vec::new();
    let mut limited: Option<BindTarget> = None;
    for target in plan {
        if unspecified_key(&target.key) || target.reply.is_unspecified() {
            continue;
        }
        match &target.key {
            BindKey::Limited { .. } => {
                limited.get_or_insert(BindTarget {
                    key: BindKey::Limited {
                        index: 0,
                        name: String::new(),
                    },
                    reply: target.reply,
                    index: 0,
                    name: String::new(),
                });
            }
            BindKey::V4Multicast { .. } => {
                if let IpAddr::V4(ip) = target.reply {
                    if udp_ip_allowed(IpAddr::V4(ip)) {
                        join_v4.push(ip);
                    }
                }
            }
            _ => kept.push(target.clone()),
        }
    }
    if let Some(target) = limited {
        kept.push(target);
    }
    kept.sort_by(|left, right| left.key.cmp(&right.key));
    kept.dedup_by(|left, right| left.key == right.key);
    join_v4.sort();
    join_v4.dedup();
    FallbackListen {
        plan: kept,
        join_v4,
    }
}

pub(crate) struct Inbound {
    pub bytes: Vec<u8>,
    pub from: SocketAddr,
    pub key: BindKey,
}

pub(crate) struct ListenerSet {
    pub plan: Vec<BindTarget>,
    pub sockets: BTreeMap<BindKey, Arc<tokio::net::UdpSocket>>,
    tasks: BTreeMap<BindKey, tokio::task::JoinHandle<()>>,
    /// Stays on the freebind plan after `SO_BINDTODEVICE` is denied so the
    /// single `255.255.255.255` socket is not closed on the next refresh.
    scope: ScopeMode,
}

impl Drop for ListenerSet {
    fn drop(&mut self) {
        for task in self.tasks.values() {
            task.abort();
        }
        self.tasks.clear();
        self.sockets.clear();
    }
}

impl ListenerSet {
    pub(crate) fn empty() -> Self {
        Self {
            plan: Vec::new(),
            sockets: BTreeMap::new(),
            tasks: BTreeMap::new(),
            scope: ScopeMode::TryDevice,
        }
    }

    pub(crate) fn summary(&self, port: u16) -> String {
        self.sockets
            .keys()
            .map(|key| key.label(port))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Close sockets whose interface address is gone and open the new ones.
    /// A task that already exited is opened again.
    pub(crate) fn reconcile(
        &mut self,
        desired: Vec<BindTarget>,
        port: u16,
        tx: tokio::sync::mpsc::Sender<Inbound>,
    ) {
        let dead: Vec<BindKey> = self
            .tasks
            .iter()
            .filter(|(_, task)| task.is_finished())
            .map(|(key, _)| key.clone())
            .collect();
        for key in dead {
            if let Some(task) = self.tasks.remove(&key) {
                task.abort();
            }
            self.sockets.remove(&key);
        }
        let requested = desired;
        let mut scope = self.scope;
        let mut device_warn: Option<String> = None;
        let mut freebind_warn: Option<String> = None;
        let mut joins = Vec::new();
        let mut applied = requested.clone();
        loop {
            let plan = match scope {
                ScopeMode::TryDevice => requested.clone(),
                ScopeMode::Freebind => {
                    let fallback = fallback_listen(&requested, ScopeError::DeviceDenied);
                    joins = fallback.join_v4;
                    fallback.plan
                }
                ScopeMode::UnicastOnly => {
                    joins.clear();
                    fallback_listen(&requested, ScopeError::FreebindFailed).plan
                }
            };
            let current: Vec<BindKey> = self.sockets.keys().cloned().collect();
            let desired_keys: Vec<BindKey> = plan.iter().map(|target| target.key.clone()).collect();
            let (close, open) = rebind_keys(&current, &desired_keys);
            for key in close {
                if let Some(task) = self.tasks.remove(&key) {
                    task.abort();
                }
                self.sockets.remove(&key);
            }
            let open: HashSet<BindKey> = open.into_iter().collect();
            let mut saw_denied = false;
            let mut saw_freebind = false;
            for target in &plan {
                if !open.contains(&target.key) {
                    continue;
                }
                match spawn_bound(target, port, tx.clone()) {
                    Ok((socket, task)) => {
                        self.sockets.insert(target.key.clone(), socket);
                        self.tasks.insert(target.key.clone(), task);
                    }
                    Err(error) => {
                        let denied = cfg!(any(target_os = "linux", target_os = "android"))
                            && scope == ScopeMode::TryDevice
                            && uses_bind_device(target)
                            && device_bind_denied(&error);
                        let freebind = cfg!(any(target_os = "linux", target_os = "android"))
                            && scope == ScopeMode::Freebind
                            && matches!(target.key, BindKey::Limited { .. });
                        if denied {
                            saw_denied = true;
                            device_warn.get_or_insert(error.to_string());
                        } else if freebind {
                            saw_freebind = true;
                            freebind_warn.get_or_insert(error.to_string());
                        } else {
                            hbb_common::throttled_log!(
                                Duration::from_secs(30),
                                warn,
                                "LAN discovery bind {} skipped: {error}",
                                target.key.label(port)
                            );
                        }
                    }
                }
            }
            applied = plan;
            if saw_denied && scope == ScopeMode::TryDevice {
                scope = ScopeMode::Freebind;
                continue;
            }
            if saw_freebind && scope == ScopeMode::Freebind {
                scope = ScopeMode::UnicastOnly;
                continue;
            }
            break;
        }
        self.scope = scope;
        self.plan = applied;
        if let Some(error) = device_warn {
            hbb_common::throttled_log!(
                Duration::from_secs(30),
                warn,
                "LAN discovery SO_BINDTODEVICE denied ({error}); listening on 255.255.255.255 with IP_FREEBIND only"
            );
        }
        if let Some(error) = freebind_warn {
            hbb_common::throttled_log!(
                Duration::from_secs(30),
                warn,
                "LAN discovery IP_FREEBIND failed ({error}); keeping unicast sockets only"
            );
        }
        for iface in joins {
            join_multicast_on_unicast(&self.sockets, iface);
        }
    }
}

fn join_multicast_on_unicast(
    sockets: &BTreeMap<BindKey, Arc<tokio::net::UdpSocket>>,
    iface: Ipv4Addr,
) {
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = (sockets, iface);
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let Some(socket) = sockets.get(&BindKey::Unicast(IpAddr::V4(iface))) else {
            return;
        };
        let sock = socket2::SockRef::from(socket.as_ref());
        if let Err(error) = sock.join_multicast_v4(&IPV4_ALL_HOSTS, &iface) {
            if error.raw_os_error() == Some(hbb_common::libc::EADDRINUSE) {
                return;
            }
            hbb_common::throttled_log!(
                Duration::from_secs(30),
                warn,
                "LAN discovery multicast join {iface} failed: {error}"
            );
        }
    }
}

pub(crate) fn rebind_keys(
    current: &[BindKey],
    desired: &[BindKey],
) -> (Vec<BindKey>, Vec<BindKey>) {
    let mut close = Vec::new();
    let mut open = Vec::new();
    for key in current {
        if !desired.contains(key) {
            close.push(key.clone());
        }
    }
    for key in desired {
        if !current.contains(key) {
            open.push(key.clone());
        }
    }
    (close, open)
}

pub(crate) fn udp_ip_allowed(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            !ip.is_unspecified()
                && !ip.is_loopback()
                && !ip.is_broadcast()
                && !ip.is_multicast()
                && (ip.is_private() || ip.is_link_local())
        }
        IpAddr::V6(ip) => {
            !ip.is_unspecified()
                && !ip.is_loopback()
                && !ip.is_multicast()
                && (ip.is_unique_local() || ip.is_unicast_link_local())
        }
    }
}

pub(crate) fn directed_broadcast(ip: Ipv4Addr, prefix: u8) -> Option<Ipv4Addr> {
    if prefix == 0 || prefix >= 32 {
        return None;
    }
    let mask = u32::MAX << (32 - prefix);
    let broadcast = Ipv4Addr::from(u32::from(ip) | !mask);
    if broadcast == ip || broadcast.is_broadcast() || broadcast.is_unspecified() {
        None
    } else {
        Some(broadcast)
    }
}

pub(crate) fn discovery_bind_plan(ifaces: &[LanIface]) -> Vec<BindTarget> {
    let mut targets = Vec::new();
    let mut global_limited = false;
    let mut global_reply = None;
    for iface in ifaces {
        let v4: Vec<(Ipv4Addr, u8)> = iface
            .v4
            .iter()
            .copied()
            .filter(|(ip, _)| udp_ip_allowed(IpAddr::V4(*ip)))
            .collect();
        let v6: Vec<Ipv6Addr> = iface
            .v6
            .iter()
            .map(|(ip, _)| *ip)
            .filter(|ip| udp_ip_allowed(IpAddr::V6(*ip)))
            .collect();
        if v4.is_empty() && v6.is_empty() {
            continue;
        }
        let v4_reply = v4.first().map(|(ip, _)| IpAddr::V4(*ip));
        let v6_reply = v6
            .iter()
            .find(|ip| ip.is_unicast_link_local())
            .or_else(|| v6.first())
            .copied()
            .map(IpAddr::V6);
        for (ip, prefix) in v4 {
            targets.push(BindTarget {
                key: BindKey::Unicast(IpAddr::V4(ip)),
                reply: IpAddr::V4(ip),
                index: iface.index,
                name: iface.name.clone(),
            });
            if let Some(broadcast) = directed_broadcast(ip, prefix) {
                targets.push(BindTarget {
                    key: BindKey::Directed(broadcast),
                    reply: IpAddr::V4(ip),
                    index: iface.index,
                    name: iface.name.clone(),
                });
            }
        }
        for ip in v6 {
            targets.push(BindTarget {
                key: BindKey::Unicast(IpAddr::V6(ip)),
                reply: IpAddr::V6(ip),
                index: iface.index,
                name: iface.name.clone(),
            });
        }
        if let Some(reply) = v4_reply {
            if iface.name.is_empty() && iface.index == 0 {
                global_limited = true;
                global_reply.get_or_insert(reply);
            } else {
                targets.push(BindTarget {
                    key: BindKey::Limited {
                        index: iface.index,
                        name: iface.name.clone(),
                    },
                    reply,
                    index: iface.index,
                    name: iface.name.clone(),
                });
                targets.push(BindTarget {
                    key: BindKey::V4Multicast { index: iface.index },
                    reply,
                    index: iface.index,
                    name: iface.name.clone(),
                });
            }
        }
        if let Some(reply) = v6_reply {
            if iface.index != 0 {
                targets.push(BindTarget {
                    key: BindKey::V6Multicast { index: iface.index },
                    reply,
                    index: iface.index,
                    name: iface.name.clone(),
                });
                // Windows binds this socket to `reply` (it cannot bind ff02::1).
                // Drop the unicast listener for that address so the port is free
                // and multicast queries are not delivered twice.
                #[cfg(windows)]
                {
                    let replaced = BindKey::Unicast(reply);
                    if let Some(pos) = targets
                        .iter()
                        .rposition(|target| target.key == replaced && target.index == iface.index)
                    {
                        targets.remove(pos);
                    }
                }
            }
        }
    }
    if global_limited {
        if let Some(reply) = global_reply {
            targets.push(BindTarget {
                key: BindKey::Limited {
                    index: 0,
                    name: String::new(),
                },
                reply,
                index: 0,
                name: String::new(),
            });
            targets.push(BindTarget {
                key: BindKey::V4Multicast { index: 0 },
                reply,
                index: 0,
                name: String::new(),
            });
        }
    }
    targets.sort_by(|left, right| left.key.cmp(&right.key));
    targets.dedup_by(|left, right| left.key == right.key);
    targets
}

pub(crate) fn lan_iface_if_kept(
    name: &str,
    index: u32,
    flags: u32,
    v4: Vec<(Ipv4Addr, u8)>,
    v6: Vec<(Ipv6Addr, u8)>,
) -> Option<LanIface> {
    if !super::keep_discovery_iface(flags) {
        return None;
    }
    Some(LanIface {
        name: name.to_owned(),
        index,
        v4,
        v6,
    })
}

pub(crate) fn interfaces_from_default_net() -> Vec<LanIface> {
    #[cfg(windows)]
    let oper = windows_oper_status();
    default_net::get_interfaces()
        .into_iter()
        .filter_map(|iface| {
            #[cfg(windows)]
            if !windows_keep_iface(&iface, oper.as_ref()) {
                return None;
            }
            lan_iface_if_kept(
                &iface.name,
                iface.index,
                iface.flags,
                iface
                    .ipv4
                    .into_iter()
                    .map(|ip| (ip.addr, ip.prefix_len))
                    .collect(),
                iface
                    .ipv6
                    .into_iter()
                    .map(|ip| (ip.addr, ip.prefix_len))
                    .collect(),
            )
        })
        .collect()
}

#[cfg(windows)]
fn windows_oper_status() -> Option<crate::platform::win_lan::AdapterOperStatus> {
    let status = crate::platform::win_lan::adapter_oper_status();
    if status.is_none() {
        hbb_common::throttled_log!(
            Duration::from_secs(30),
            warn,
            "LAN discovery could not read Windows adapter status; tunnel and virtual-adapter name filters still apply"
        );
    }
    status
}

#[cfg(windows)]
fn windows_keep_iface(
    iface: &default_net::Interface,
    oper: Option<&crate::platform::win_lan::AdapterOperStatus>,
) -> bool {
    let oper_status = match oper {
        Some(oper) => oper.get(&iface.name, iface.index).unwrap_or(0),
        None => super::IF_OPER_STATUS_UP,
    };
    let friendly = iface.friendly_name.as_deref().unwrap_or("");
    let description = iface.description.as_deref().unwrap_or("");
    super::windows_discovery_adapter_allowed(&super::WindowsDiscoveryAdapter {
        if_type: iface.if_type.value(),
        oper_status,
        name: &iface.name,
        friendly_name: friendly,
        description,
    })
}

fn has_discovery_addr(ifaces: &[LanIface]) -> bool {
    ifaces.iter().any(|iface| {
        iface
            .v4
            .iter()
            .any(|(ip, _)| udp_ip_allowed(IpAddr::V4(*ip)))
            || iface
                .v6
                .iter()
                .any(|(ip, _)| udp_ip_allowed(IpAddr::V6(*ip)))
    })
}

/// Interfaces to query, plus whether discovery must fall back to
/// `255.255.255.255`. The fallback is for Android when netlink enumeration is
/// empty or every interface was rejected (flags 0, down, VPN, loopback).
pub(crate) fn query_targets<'a>(
    enumerated: &'a [LanIface],
    android_fallback: &'a [LanIface],
) -> (&'a [LanIface], bool) {
    if has_discovery_addr(enumerated) {
        (enumerated, false)
    } else if has_discovery_addr(android_fallback) {
        (android_fallback, false)
    } else {
        (&[], true)
    }
}

/// Android UI JSON (`name`, `index`, `flags`, `addrs` as `ip/prefix`).
/// Down, loopback, point-to-point, and VPN-named interfaces are skipped.
/// Address filtering happens later.
pub(crate) fn ifaces_from_android_json(payload: &str) -> Vec<LanIface> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
        return Vec::new();
    };
    let Some(list) = value.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for iface in list {
        let flags = iface.get("flags").and_then(|v| v.as_u64()).unwrap_or(0);
        let name = iface
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        // Same exclusions as Unix `discovery_interface_flags`, in Go net.Flags
        // (1 up, 4 loopback, 8 point-to-point) plus tun/ppp/wg names.
        if !crate::tailcat::android_iface_usable(&name, flags) {
            continue;
        }
        let index = iface.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let mut v4 = Vec::new();
        let mut v6 = Vec::new();
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
            let prefix = parts
                .next()
                .and_then(|value| value.parse::<u8>().ok())
                .unwrap_or(0);
            if let Ok(ip) = ip.parse::<Ipv4Addr>() {
                v4.push((ip, prefix));
            } else if let Ok(ip) = ip.parse::<Ipv6Addr>() {
                v6.push((ip, prefix));
            }
        }
        if !v4.is_empty() || !v6.is_empty() {
            out.push(LanIface {
                name,
                index,
                v4,
                v6,
            });
        }
    }
    out
}

pub(crate) fn current_bind_plan() -> Vec<BindTarget> {
    let plan = discovery_bind_plan(&interfaces_from_default_net());
    #[cfg(target_os = "android")]
    if !plan
        .iter()
        .any(|target| matches!(target.key, BindKey::Unicast(_)))
    {
        let fallback = ifaces_from_android_json(&crate::tailcat::android_interface_json());
        if !fallback.is_empty() {
            return discovery_bind_plan(&fallback);
        }
    }
    plan
}

pub(crate) fn ipv4_query_destinations(ip: Ipv4Addr, prefix: u8) -> Vec<Ipv4Addr> {
    let mut dests = Vec::with_capacity(3);
    if let Some(broadcast) = directed_broadcast(ip, prefix) {
        dests.push(broadcast);
    }
    dests.push(Ipv4Addr::BROADCAST);
    dests.push(IPV4_ALL_HOSTS);
    dests
}

pub(crate) fn collect_query_sockets(
    ifaces: &[LanIface],
    port: u16,
    bytes: &[u8],
) -> Vec<UdpSocket> {
    let mut sockets = Vec::new();
    for iface in ifaces {
        for (ip, prefix) in &iface.v4 {
            if !udp_ip_allowed(IpAddr::V4(*ip)) {
                continue;
            }
            let Ok(socket) = UdpSocket::bind((*ip, 0)) else {
                continue;
            };
            if socket.set_broadcast(true).is_err() {
                continue;
            }
            let mut sent = false;
            for dest in ipv4_query_destinations(*ip, *prefix) {
                if socket.send_to(bytes, (dest, port)).is_ok() {
                    sent = true;
                }
            }
            if sent && socket.set_nonblocking(true).is_ok() {
                sockets.push(socket);
            }
        }
        if iface.index == 0 {
            continue;
        }
        for (ip, _) in &iface.v6 {
            if !udp_ip_allowed(IpAddr::V6(*ip)) {
                continue;
            }
            let scope = if ip.is_unicast_link_local() {
                iface.index
            } else {
                0
            };
            let Ok(socket) = UdpSocket::bind(SocketAddrV6::new(*ip, 0, 0, scope)) else {
                continue;
            };
            let dest = SocketAddrV6::new(IPV6_ALL_NODES, port, 0, iface.index);
            if socket.send_to(bytes, dest).is_ok() && socket.set_nonblocking(true).is_ok() {
                sockets.push(socket);
            }
        }
    }
    sockets
}

pub(crate) fn open_target(target: &BindTarget, port: u16) -> io::Result<UdpSocket> {
    cfg_if::cfg_if! {
        if #[cfg(any(target_os = "linux", target_os = "android"))] {
            open_linux(target, port)
        } else if #[cfg(any(target_os = "macos", target_os = "ios"))] {
            open_darwin(target, port)
        } else if #[cfg(windows)] {
            open_windows(target, port)
        } else {
            open_unicast_only(target, port)
        }
    }
}

fn spawn_bound(
    target: &BindTarget,
    port: u16,
    tx: tokio::sync::mpsc::Sender<Inbound>,
) -> io::Result<(Arc<tokio::net::UdpSocket>, tokio::task::JoinHandle<()>)> {
    let std_sock = open_target(target, port)?;
    std_sock.set_nonblocking(true)?;
    let socket = Arc::new(
        tokio::net::UdpSocket::from_std(std_sock)
            .map_err(|error| io::Error::new(io::ErrorKind::Other, error))?,
    );
    let key = target.key.clone();
    let recv = socket.clone();
    let task = tokio::spawn(async move {
        let mut buf = [0u8; 2048];
        loop {
            match recv.recv_from(&mut buf).await {
                Ok((n, from)) if n > 0 => {
                    let bytes = buf[..n].to_vec();
                    if tx
                        .send(Inbound {
                            bytes,
                            from,
                            key: key.clone(),
                        })
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(_) => continue,
                Err(_) => break,
            }
        }
    });
    Ok((socket, task))
}

fn bind_unicast(ip: IpAddr, port: u16, index: u32) -> io::Result<UdpSocket> {
    if !udp_ip_allowed(ip) && !ip.is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            "refusing to bind discovery outside private and link-local ranges",
        ));
    }
    let domain = if ip.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    };
    let socket = Socket::new(domain, Type::DGRAM, None)?;
    if ip.is_ipv6() {
        socket.set_only_v6(true)?;
    } else {
        socket.set_broadcast(true)?;
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    if let Some(index) = NonZeroU32::new(index) {
        if ip.is_ipv4() {
            socket.bind_device_by_index_v4(Some(index))?;
        } else {
            socket.bind_device_by_index_v6(Some(index))?;
        }
    }
    let addr = match ip {
        IpAddr::V4(ip) => SocketAddr::new(IpAddr::V4(ip), port),
        IpAddr::V6(ip) => {
            let scope = if ip.is_unicast_link_local() { index } else { 0 };
            SocketAddr::V6(SocketAddrV6::new(ip, port, 0, scope))
        }
    };
    socket.bind(&SockAddr::from(addr))?;
    #[cfg(windows)]
    {
        // IPv4 all-hosts is joined here because Windows also rejects a bind to
        // 224.0.0.1. IPv6 all-nodes is joined on the V6Multicast socket, which
        // binds this interface's reply address (see `bind_v6_multicast_windows`).
        if let IpAddr::V4(iface) = ip {
            let _ = socket.join_multicast_v4(&IPV4_ALL_HOSTS, &iface);
        }
    }
    Ok(socket.into())
}

fn bind_v6_multicast(index: u32, port: u16) -> io::Result<UdpSocket> {
    let Some(index) = NonZeroU32::new(index) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "IPv6 multicast needs an interface index",
        ));
    };
    let socket = Socket::new(Domain::IPV6, Type::DGRAM, None)?;
    socket.set_only_v6(true)?;
    let addr = SocketAddr::V6(SocketAddrV6::new(IPV6_ALL_NODES, port, 0, index.get()));
    socket.bind(&SockAddr::from(addr))?;
    socket.join_multicast_v6(&IPV6_ALL_NODES, index.get())?;
    Ok(socket.into())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn prepare_free_v4(device: Option<&str>) -> io::Result<Socket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, None)?;
    socket.set_broadcast(true)?;
    socket.set_freebind(true)?;
    if let Some(device) = device {
        if !device.is_empty() {
            let mut name = device.as_bytes().to_vec();
            name.push(0);
            socket.bind_device(Some(&name))?;
        }
    }
    Ok(socket)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn bind_free_v4(ip: Ipv4Addr, port: u16, device: Option<&str>) -> io::Result<UdpSocket> {
    let socket = prepare_free_v4(device)?;
    socket.bind(&SockAddr::from(SocketAddr::new(IpAddr::V4(ip), port)))?;
    Ok(socket.into())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn open_linux(target: &BindTarget, port: u16) -> io::Result<UdpSocket> {
    match &target.key {
        BindKey::Unicast(ip) => bind_unicast(*ip, port, target.index),
        BindKey::Directed(ip) => bind_free_v4(*ip, port, None),
        BindKey::Limited { name, .. } => {
            let device = if name.is_empty() {
                None
            } else {
                Some(name.as_str())
            };
            bind_free_v4(Ipv4Addr::BROADCAST, port, device)
        }
        BindKey::V4Multicast { .. } => {
            let IpAddr::V4(iface) = target.reply else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "IPv4 multicast needs an interface address",
                ));
            };
            let device = if target.name.is_empty() {
                None
            } else {
                Some(target.name.as_str())
            };
            let socket = prepare_free_v4(device)?;
            socket.bind(&SockAddr::from(SocketAddr::new(
                IpAddr::V4(IPV4_ALL_HOSTS),
                port,
            )))?;
            socket.join_multicast_v4(&IPV4_ALL_HOSTS, &iface)?;
            Ok(socket.into())
        }
        BindKey::V6Multicast { index } => bind_v6_multicast(*index, port),
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn scope_v4(socket: &Socket, index: u32) -> io::Result<()> {
    let Some(index) = NonZeroU32::new(index) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "broadcast listen needs an interface index",
        ));
    };
    socket.bind_device_by_index_v4(Some(index))
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn open_darwin(target: &BindTarget, port: u16) -> io::Result<UdpSocket> {
    match &target.key {
        BindKey::Unicast(ip) => bind_unicast(*ip, port, target.index),
        BindKey::Directed(ip) => {
            let socket = Socket::new(Domain::IPV4, Type::DGRAM, None)?;
            socket.set_broadcast(true)?;
            scope_v4(&socket, target.index)?;
            socket.bind(&SockAddr::from(SocketAddr::new(IpAddr::V4(*ip), port)))?;
            Ok(socket.into())
        }
        BindKey::Limited { .. } => {
            // Unicast binds do not receive 255.255.255.255 here. `IP_BOUND_IF`
            // keeps this unspecified socket on one private interface.
            let socket = Socket::new(Domain::IPV4, Type::DGRAM, None)?;
            socket.set_broadcast(true)?;
            socket.set_reuse_address(true)?;
            scope_v4(&socket, target.index)?;
            socket.bind(&SockAddr::from(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                port,
            )))?;
            if let IpAddr::V4(iface) = target.reply {
                let _ = socket.join_multicast_v4(&IPV4_ALL_HOSTS, &iface);
            }
            Ok(socket.into())
        }
        BindKey::V4Multicast { .. } => {
            let IpAddr::V4(iface) = target.reply else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "IPv4 multicast needs an interface address",
                ));
            };
            let socket = Socket::new(Domain::IPV4, Type::DGRAM, None)?;
            socket.set_reuse_address(true)?;
            scope_v4(&socket, target.index)?;
            socket.bind(&SockAddr::from(SocketAddr::new(
                IpAddr::V4(IPV4_ALL_HOSTS),
                port,
            )))?;
            socket.join_multicast_v4(&IPV4_ALL_HOSTS, &iface)?;
            Ok(socket.into())
        }
        BindKey::V6Multicast { index } => bind_v6_multicast(*index, port),
    }
}

#[cfg(windows)]
fn open_windows(target: &BindTarget, port: u16) -> io::Result<UdpSocket> {
    match &target.key {
        BindKey::Unicast(ip) => bind_unicast(*ip, port, target.index),
        BindKey::Directed(ip) => {
            let socket = Socket::new(Domain::IPV4, Type::DGRAM, None)?;
            socket.set_broadcast(true)?;
            socket.bind(&SockAddr::from(SocketAddr::new(IpAddr::V4(*ip), port)))?;
            Ok(socket.into())
        }
        BindKey::Limited { .. } => {
            // Interface unicast sockets already receive broadcasts on Windows.
            // This bind covers stacks that only deliver 255.255.255.255 to a
            // socket bound to that address. It is not 0.0.0.0.
            let socket = Socket::new(Domain::IPV4, Type::DGRAM, None)?;
            socket.set_broadcast(true)?;
            socket.bind(&SockAddr::from(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::BROADCAST),
                port,
            )))?;
            Ok(socket.into())
        }
        BindKey::V4Multicast { .. } => {
            let IpAddr::V4(iface) = target.reply else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "IPv4 multicast needs an interface address",
                ));
            };
            let socket = Socket::new(Domain::IPV4, Type::DGRAM, None)?;
            socket.set_reuse_address(true)?;
            socket.bind(&SockAddr::from(SocketAddr::new(
                IpAddr::V4(IPV4_ALL_HOSTS),
                port,
            )))?;
            socket.join_multicast_v4(&IPV4_ALL_HOSTS, &iface)?;
            Ok(socket.into())
        }
        BindKey::V6Multicast { index } => {
            let IpAddr::V6(ip) = target.reply else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "IPv6 multicast needs an interface address",
                ));
            };
            bind_v6_multicast_windows(ip, *index, port)
        }
    }
}

/// Windows `bind` of `ff02::1` fails with `WSAEADDRNOTAVAIL` (10049). Winsock
/// requires a local unicast address (not the group, and not a global `::`
/// listen) followed by `IPV6_ADD_MEMBERSHIP` on the interface index.
#[cfg(windows)]
fn bind_v6_multicast_windows(ip: Ipv6Addr, index: u32, port: u16) -> io::Result<UdpSocket> {
    if !udp_ip_allowed(IpAddr::V6(ip)) {
        return Err(io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            "refusing to bind discovery outside private and link-local ranges",
        ));
    }
    let Some(index) = NonZeroU32::new(index) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "IPv6 multicast needs an interface index",
        ));
    };
    let socket = Socket::new(Domain::IPV6, Type::DGRAM, None)?;
    socket.set_only_v6(true)?;
    let scope = if ip.is_unicast_link_local() {
        index.get()
    } else {
        0
    };
    socket.bind(&SockAddr::from(SocketAddr::V6(SocketAddrV6::new(
        ip, port, 0, scope,
    ))))?;
    socket.join_multicast_v6(&IPV6_ALL_NODES, index.get())?;
    Ok(socket.into())
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "windows"
)))]
fn open_unicast_only(target: &BindTarget, port: u16) -> io::Result<UdpSocket> {
    match &target.key {
        BindKey::Unicast(ip) => bind_unicast(*ip, port, target.index),
        _ => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "broadcast discovery sockets are not implemented on this OS",
        )),
    }
}
