use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;
use tokio::sync::Mutex as TokioMutex;
use tokio_util::sync::CancellationToken;

use crate::buffer::{InputBuffer, OutputBuffer};
use crate::error::RnsError;
use crate::hash::AddressHash;
use crate::iface::{IfaceRole, IfaceSource, InterfaceManager, RxMessage, TxMessageType};
use crate::packet::Packet;
use crate::serde::Serialize;

use super::{Interface, InterfaceContext};

fn bind_udp(bind_addr: &str, forward_addr: Option<&str>) -> std::io::Result<UdpSocket> {
    let parsed: SocketAddr = bind_addr.parse().map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("bad bind address {}: {}", bind_addr, e),
        )
    })?;

    let domain = if parsed.is_ipv6() { Domain::IPV6 } else { Domain::IPV4 };
    let socket = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP))?;

    // Allow multiple nodes (or restart of the same node) to bind the same port.
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    socket.set_reuse_port(true)?;

    // If binding to a multicast group directly, bind to the unspecified address on
    // the same port instead; then join the group. This works cross-platform.
    let (bound_addr, multicast_group) = match parsed.ip() {
        IpAddr::V6(ip) if ip.is_multicast() => {
            let any: SocketAddr = (std::net::Ipv6Addr::UNSPECIFIED, parsed.port()).into();
            (any, Some(IpAddr::V6(ip)))
        }
        IpAddr::V4(ip) if ip.is_multicast() => {
            let any: SocketAddr = (std::net::Ipv4Addr::UNSPECIFIED, parsed.port()).into();
            (any, Some(IpAddr::V4(ip)))
        }
        _ => (parsed, None),
    };

    socket.bind(&bound_addr.into())?;

    if let Some(group) = multicast_group {
        match group {
            IpAddr::V6(g) => socket.join_multicast_v6(&g, 0)?,
            IpAddr::V4(g) => socket.join_multicast_v4(&g, &std::net::Ipv4Addr::UNSPECIFIED)?,
        }
    }
    if let (IpAddr::V4(bind_ip), Some(forward_addr)) = (parsed.ip(), forward_addr) {
        if !bind_ip.is_unspecified() && !bind_ip.is_multicast() && is_multicast_addr(forward_addr) {
            socket.set_multicast_if_v4(&bind_ip)?;
            socket.set_multicast_loop_v4(true)?;
        }
    }

    socket.set_nonblocking(true)?;
    let std_socket: std::net::UdpSocket = socket.into();
    UdpSocket::from_std(std_socket)
}

// UDP trace logging stays on by default for packet-level network bring-up visibility.
const PACKET_TRACE: bool = true;

/// Returns true if `addr` parses as a SocketAddr whose IP is multicast
/// (IPv4 `224.0.0.0/4` or IPv6 `ff00::/8`).
fn is_multicast_addr(addr: &str) -> bool {
    addr.parse::<SocketAddr>().ok().map(|sa| sa.ip().is_multicast()).unwrap_or(false)
}

/// Bidirectional map between a peer's reply `SocketAddr` and the
/// virtual `AddressHash` the transport layer uses to route
/// point-to-point traffic to that peer. Shared (via `Arc<Mutex<_>>`)
/// between the multicast `UdpInterface`'s tx and rx tasks and the
/// transport layer, which registers peers from received announces.
///
/// The same physical socket on the host carries both multicast
/// broadcasts and per-peer unicast sends; this map is what lets the
/// transport treat "reply to peer X" as a separate logical iface
/// without spawning additional sockets.
#[derive(Debug, Default)]
pub struct PeerRouting {
    by_addr: HashMap<SocketAddr, AddressHash>,
    by_hash: HashMap<AddressHash, SocketAddr>,
}

impl PeerRouting {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a (peer addr ↔ virtual iface hash) mapping, overwriting
    /// any prior entries for either key. Both directions stay
    /// consistent.
    pub fn insert(&mut self, addr: SocketAddr, hash: AddressHash) {
        if let Some(prev_hash) = self.by_addr.insert(addr, hash) {
            if prev_hash != hash {
                self.by_hash.remove(&prev_hash);
            }
        }
        if let Some(prev_addr) = self.by_hash.insert(hash, addr) {
            if prev_addr != addr {
                self.by_addr.remove(&prev_addr);
            }
        }
    }

    pub fn addr_for_hash(&self, hash: &AddressHash) -> Option<SocketAddr> {
        self.by_hash.get(hash).copied()
    }

    pub fn hash_for_addr(&self, addr: &SocketAddr) -> Option<AddressHash> {
        self.by_addr.get(addr).copied()
    }

    /// Remove by virtual iface hash. Used by the transport's GC pass
    /// when a peer hasn't announced for `UNICAST_IFACE_IDLE_TIMEOUT`.
    pub fn remove_by_hash(&mut self, hash: &AddressHash) -> Option<SocketAddr> {
        let addr = self.by_hash.remove(hash)?;
        self.by_addr.remove(&addr);
        Some(addr)
    }

    pub fn len(&self) -> usize {
        self.by_hash.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_hash.is_empty()
    }
}

pub struct UdpInterface {
    bind_addr: String,
    forward_addr: Option<String>,
    is_multicast: bool,
    /// Peer-routing map for multicast ifaces. When present, the tx task
    /// resolves `TxMessageType::Direct` targets through this map (to
    /// send unicast to a specific peer from the same socket that
    /// carries multicast broadcasts), and the rx task attributes
    /// received packets to a virtual iface hash based on the sender's
    /// `SocketAddr`. For unicast ifaces this is `None`.
    peer_routing: Option<Arc<TokioMutex<PeerRouting>>>,
}

impl UdpInterface {
    /// Plain unicast UDP iface. Send/receive through a single
    /// `(bind, forward)` pair. No per-peer routing.
    pub fn new<T: Into<String>>(bind_addr: T, forward_addr: Option<T>) -> Self {
        let bind_addr = bind_addr.into();
        let forward_addr = forward_addr.map(Into::into);
        let is_multicast = is_multicast_addr(&bind_addr)
            || forward_addr.as_deref().map(is_multicast_addr).unwrap_or(false);
        Self { bind_addr, forward_addr, is_multicast, peer_routing: None }
    }

    /// Multicast UDP iface with a shared peer-routing map. Announces
    /// (and other `TxMessageType::Broadcast` traffic) still go to the
    /// multicast group via `forward_addr`. Point-to-point sends target
    /// a virtual iface hash registered in `peer_routing`; the tx task
    /// resolves the hash to the peer's `SocketAddr` and sends unicast
    /// from this same socket — so the peer sees replies as coming from
    /// the well-known multicast port, matching its own entry for this
    /// host and avoiding ephemeral-port proliferation.
    pub fn new_multicast<T: Into<String>>(
        bind_addr: T,
        forward_addr: Option<T>,
        peer_routing: Arc<TokioMutex<PeerRouting>>,
    ) -> Self {
        let mut iface = Self::new(bind_addr, forward_addr);
        iface.peer_routing = Some(peer_routing);
        iface
    }

    /// True if this iface is configured as multicast (by bind or
    /// forward address, or by being constructed with a peer-routing
    /// map). Used by `InterfaceManager` role tagging at spawn time.
    pub fn is_multicast(&self) -> bool {
        self.is_multicast || self.peer_routing.is_some()
    }

    pub async fn spawn(context: InterfaceContext<Self>) {
        let (bind_addr, forward_addr, is_multicast, peer_routing) = {
            let inner = context.inner.lock().unwrap();
            (
                inner.bind_addr.clone(),
                inner.forward_addr.clone(),
                inner.is_multicast,
                inner.peer_routing.clone(),
            )
        };
        let iface_address = context.channel.address;

        let (rx_channel, tx_channel) = context.channel.split();
        let tx_channel = Arc::new(tokio::sync::Mutex::new(tx_channel));

        loop {
            if context.cancel.is_cancelled() {
                break;
            }

            let socket = bind_udp(&bind_addr, forward_addr.as_deref())
                .map_err(|_| RnsError::ConnectionError);

            if socket.is_err() {
                log::warn!("couldn't bind to <{}>", bind_addr);
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }

            let cancel = context.cancel.clone();
            let stop = CancellationToken::new();

            let socket = socket.unwrap();
            let read_socket = Arc::new(socket);
            let write_socket = read_socket.clone();

            log::info!(
                "udp_interface bound to <{}> (multicast={}, has_peer_routing={})",
                bind_addr,
                is_multicast,
                peer_routing.is_some(),
            );

            const BUFFER_SIZE: usize = core::mem::size_of::<Packet>() * 3;

            // Start receive task
            let rx_task = {
                let cancel = cancel.clone();
                let stop = stop.clone();
                let socket = read_socket;
                let rx_channel = rx_channel.clone();
                let peer_routing = peer_routing.clone();

                tokio::spawn(async move {
                    loop {
                        let mut rx_buffer = [0u8; BUFFER_SIZE];

                        tokio::select! {
                            _ = cancel.cancelled() => {
                                    break;
                            }
                            _ = stop.cancelled() => {
                                    break;
                            }
                            result = socket.recv_from(&mut rx_buffer) => {
                                match result {
                                    Ok((0, _)) => {
                                        log::warn!("connection closed");
                                        stop.cancel();
                                        break;
                                    }
                                    Ok((n, in_addr)) => {
                                        if let Ok(packet) = Packet::deserialize(&mut InputBuffer::new(&rx_buffer[..n])) {
                                            // Re-attribute to a virtual per-peer iface if
                                            // this source is in the routing map. This is
                                            // what makes link.iface_matches succeed for a
                                            // unicast proof that arrives on the same
                                            // physical socket as multicast announces.
                                            let attributed_iface = match peer_routing.as_ref() {
                                                Some(routing) => routing
                                                    .lock()
                                                    .await
                                                    .hash_for_addr(&in_addr)
                                                    .unwrap_or(iface_address),
                                                None => iface_address,
                                            };
                                            if PACKET_TRACE {
                                                log::trace!(
                                                    "rx << (iface {} / attributed {}) from {} {}",
                                                    iface_address, attributed_iface, in_addr, packet
                                                );
                                            }
                                            let _ = rx_channel.send(RxMessage {
                                                address: attributed_iface,
                                                packet,
                                                source: IfaceSource::Udp(in_addr),
                                            }).await;
                                        } else {
                                            log::warn!("couldn't decode packet");
                                        }
                                    }
                                    Err(e) => {
                                        log::warn!("connection error {}", e);
                                        break;
                                    }
                                }
                            },
                        };
                    }
                })
            };

            if let Some(forward_addr) = forward_addr.clone() {
                // Start transmit task
                let tx_task = {
                    let cancel = cancel.clone();
                    let tx_channel = tx_channel.clone();
                    let socket = write_socket;
                    let peer_routing = peer_routing.clone();

                    tokio::spawn(async move {
                        loop {
                            if stop.is_cancelled() {
                                break;
                            }

                            let mut tx_buffer = [0u8; BUFFER_SIZE];

                            let mut tx_channel = tx_channel.lock().await;

                            tokio::select! {
                                _ = cancel.cancelled() => {
                                        break;
                                }
                                _ = stop.cancelled() => {
                                        break;
                                }
                                Some(message) = tx_channel.recv() => {
                                    // Route the tx through the peer map:
                                    //   Broadcast → multicast forward_addr (unchanged).
                                    //   Direct(iface_hash):
                                    //     - if hash resolves in peer_routing → unicast to that peer
                                    //     - else if hash == this iface's own address → drop
                                    //       (the tx-guard: Direct tx to a multicast iface is
                                    //       nonsensical — every packet would flood the group)
                                    //     - else → drop (unknown virtual iface)
                                    let target = match message.tx_type {
                                        TxMessageType::Broadcast(_) => Some(forward_addr.clone()),
                                        TxMessageType::Direct(addr) => {
                                            if let Some(ref routing) = peer_routing {
                                                if let Some(peer) = routing.lock().await.addr_for_hash(&addr) {
                                                    Some(peer.to_string())
                                                } else if addr == iface_address {
                                                    if PACKET_TRACE {
                                                        log::trace!(
                                                            "dropping Direct tx targeting multicast iface {} (type={:?})",
                                                            iface_address,
                                                            message.packet.header.packet_type,
                                                        );
                                                    }
                                                    None
                                                } else {
                                                    if PACKET_TRACE {
                                                        log::trace!(
                                                            "dropping Direct tx for unknown virtual iface {}",
                                                            addr,
                                                        );
                                                    }
                                                    None
                                                }
                                            } else {
                                                // Unicast iface with no peer routing — forward_addr is
                                                // the fixed target, same as broadcast.
                                                Some(forward_addr.clone())
                                            }
                                        }
                                    };

                                    if let Some(dest) = target {
                                        let packet = message.packet;
                                        if PACKET_TRACE {
                                            log::trace!(
                                                "tx >> ({}) to {} {}",
                                                iface_address, dest, packet
                                            );
                                        }
                                        let mut output = OutputBuffer::new(&mut tx_buffer);
                                        if packet.serialize(&mut output).is_ok() {
                                            let _ = socket.send_to(output.as_slice(), &dest).await;
                                        }
                                    }
                                }
                            };
                        }
                    })
                };
                tx_task.await.unwrap();
            }

            rx_task.await.unwrap();

            log::info!("udp_interface <{}>: closed", bind_addr);
        }
    }
}

impl Interface for UdpInterface {
    fn mtu() -> usize {
        2048
    }
}

/// Spawn a multicast UDP interface with per-peer routing enabled.
/// Returns the iface's `AddressHash` plus the shared `PeerRouting` map
/// — pass the latter to the transport so it can `register` peers
/// (typically from received announces), which then makes `Direct` tx
/// targeting the virtual iface hash land as unicast on that peer.
pub fn spawn_multicast_udp(
    mgr: &mut InterfaceManager,
    bind_addr: String,
    forward_addr: Option<String>,
) -> (AddressHash, Arc<TokioMutex<PeerRouting>>) {
    let peer_routing = Arc::new(TokioMutex::new(PeerRouting::new()));
    let iface = UdpInterface::new_multicast(bind_addr, forward_addr, peer_routing.clone());
    let hash = mgr.spawn_as(iface, UdpInterface::spawn, IfaceRole::Multicast);
    (hash, peer_routing)
}

pub fn encode_frame(data: &[u8]) -> Result<Vec<u8>, RnsError> {
    Ok(data.to_vec())
}

pub fn decode_frame(frame: &[u8]) -> Result<Vec<u8>, RnsError> {
    Ok(frame.to_vec())
}

include!("udp_tests.rs");
