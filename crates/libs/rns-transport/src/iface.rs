pub mod auto;
pub mod driver;
pub mod hdlc;
pub mod kiss;
pub mod lora;
pub mod rnode_ble;
pub mod serial;
pub mod tcp_client;
pub mod tcp_server;
pub mod udp;
pub mod vrn76_kiss_ble;

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::hash::AddressHash;
use crate::hash::Hash;
use crate::packet::Packet;

include!("iface_types.rs");

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum TxMessageType {
    Broadcast(Option<AddressHash>),
    Direct(AddressHash),
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub struct TxMessage {
    pub tx_type: TxMessageType,
    pub packet: Packet,
}

#[derive(Debug, Default, PartialEq, Eq, Copy, Clone)]
pub struct TxDispatchTrace {
    pub matched_ifaces: usize,
    pub sent_ifaces: usize,
    pub queued_ifaces: usize,
    pub failed_ifaces: usize,
}

#[derive(Debug, Default, PartialEq, Eq, Copy, Clone)]
pub enum InterfaceMode {
    #[default]
    Full,
    PointToPoint,
    AccessPoint,
    Roaming,
    Boundary,
    Gateway,
}

impl InterfaceMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "full" => Some(Self::Full),
            "pointtopoint" | "point_to_point" | "point-to-point" | "ptp" => {
                Some(Self::PointToPoint)
            }
            "access_point" | "accesspoint" | "access-point" | "ap" => Some(Self::AccessPoint),
            "roaming" => Some(Self::Roaming),
            "boundary" => Some(Self::Boundary),
            "gateway" | "gw" => Some(Self::Gateway),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::PointToPoint => "pointtopoint",
            Self::AccessPoint => "access_point",
            Self::Roaming => "roaming",
            Self::Boundary => "boundary",
            Self::Gateway => "gateway",
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq, Copy, Clone)]
pub struct AnnounceBroadcastPolicy {
    pub local_destination: bool,
    pub next_hop_iface_mode: Option<InterfaceMode>,
}

/// Where a received packet came from at the wire level.
///
/// Packets arriving over a stream/serial medium have `None`; UDP packets
/// carry the sender's socket address so the transport can route replies
/// back unicast instead of re-broadcasting them onto a multicast group.
#[derive(Debug, Default, PartialEq, Eq, Copy, Clone)]
pub enum IfaceSource {
    #[default]
    None,
    Udp(SocketAddr),
}

/// Tags an interface's transmit semantics.
///
/// - `Unicast` (default): TCP / serial / point-to-point UDP. Carries
///   both `Broadcast` and `Direct` tx.
/// - `Multicast`: shared-group UDP. Carries `Broadcast` tx; `Direct` tx
///   addressed at the iface itself is dropped by the tx-guard in
///   `iface::udp` (nonsensical — multicast sockets broadcast every tx).
///   Per-peer unicast traffic on this medium goes via `VirtualUnicast`
///   siblings that share the host multicast socket.
/// - `VirtualUnicast`: a *virtual* iface pinned to one peer over a host
///   multicast iface. Registered via
///   `InterfaceManager::register_virtual_iface`; shares its tx channel
///   with the host iface so the host iface's tx task routes by
///   destination. Skipped on `Broadcast` tx — the host iface already
///   delivers broadcasts for the whole group.
#[derive(Debug, Default, PartialEq, Eq, Copy, Clone)]
pub enum IfaceRole {
    #[default]
    Unicast,
    Multicast,
    VirtualUnicast,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub struct RxMessage {
    pub address: AddressHash,
    pub packet: Packet,
    pub source: IfaceSource,
}

pub struct InterfaceChannel {
    pub address: AddressHash,
    pub rx_channel: InterfaceRxSender,
    pub tx_channel: InterfaceTxReceiver,
    pub stop: CancellationToken,
}

impl InterfaceChannel {
    pub fn make_rx_channel(cap: usize) -> (InterfaceRxSender, InterfaceRxReceiver) {
        mpsc::channel(cap)
    }

    pub fn make_tx_channel(cap: usize) -> (InterfaceTxSender, InterfaceTxReceiver) {
        mpsc::channel(cap)
    }

    pub fn new(
        rx_channel: InterfaceRxSender,
        tx_channel: InterfaceTxReceiver,
        address: AddressHash,
        stop: CancellationToken,
    ) -> Self {
        Self { address, rx_channel, tx_channel, stop }
    }

    pub fn address(&self) -> &AddressHash {
        &self.address
    }

    pub fn split(self) -> (InterfaceRxSender, InterfaceTxReceiver) {
        (self.rx_channel, self.tx_channel)
    }
}

pub trait Interface {
    fn mtu() -> usize;
}

struct LocalInterface {
    address: AddressHash,
    full_hash: Hash,
    tx_send: InterfaceTxSender,
    stop: CancellationToken,
    role: IfaceRole,
    mode: InterfaceMode,
    outgoing: bool,
    announce_queue: VecDeque<QueuedAnnounce>,
    announce_allowed_at: Instant,
    announce_bitrate_bps: u64,
    announce_cap_percent: u64,
}

#[derive(Debug, Copy, Clone)]
struct QueuedAnnounce {
    message: TxMessage,
    queued_at: Instant,
    emitted: u64,
}

pub struct InterfaceContext<T: Interface> {
    pub inner: Arc<Mutex<T>>,
    pub channel: InterfaceChannel,
    pub cancel: CancellationToken,
}

pub struct InterfaceManager {
    counter: usize,
    rx_recv: Arc<tokio::sync::Mutex<InterfaceRxReceiver>>,
    rx_send: InterfaceRxSender,
    cancel: CancellationToken,
    ifaces: Vec<LocalInterface>,
}

include!("iface_runtime.rs");

impl InterfaceManager {
    pub fn new(rx_cap: usize) -> Self {
        let (rx_send, rx_recv) = InterfaceChannel::make_rx_channel(rx_cap);
        let rx_recv = Arc::new(tokio::sync::Mutex::new(rx_recv));

        Self { counter: 0, rx_recv, rx_send, cancel: CancellationToken::new(), ifaces: Vec::new() }
    }

    pub fn new_channel(&mut self, tx_cap: usize) -> InterfaceChannel {
        self.new_channel_with_role(tx_cap, IfaceRole::default())
    }

    pub fn new_channel_with_role(&mut self, tx_cap: usize, role: IfaceRole) -> InterfaceChannel {
        self.new_channel_with_role_and_mode(tx_cap, role, InterfaceMode::default())
    }

    pub fn new_channel_with_role_and_mode(
        &mut self,
        tx_cap: usize,
        role: IfaceRole,
        mode: InterfaceMode,
    ) -> InterfaceChannel {
        self.counter += 1;

        let counter_bytes = self.counter.to_le_bytes();
        let full_hash = Hash::new_from_slice(&counter_bytes[..]);
        let address = AddressHash::new_from_hash(&full_hash);

        let (tx_send, tx_recv) = InterfaceChannel::make_tx_channel(tx_cap);

        log::debug!("create channel {} role={:?} mode={:?}", address, role, mode);

        let stop = CancellationToken::new();

        self.ifaces.push(LocalInterface {
            address,
            full_hash,
            tx_send,
            stop: stop.clone(),
            role,
            mode,
            outgoing: true,
            announce_queue: VecDeque::new(),
            announce_allowed_at: Instant::now(),
            announce_bitrate_bps: DEFAULT_IFACE_BITRATE_BPS,
            announce_cap_percent: DEFAULT_ANNOUNCE_CAP_PERCENT,
        });

        InterfaceChannel { rx_channel: self.rx_send.clone(), tx_channel: tx_recv, address, stop }
    }

    pub fn new_context<T: Interface>(&mut self, inner: T) -> InterfaceContext<T> {
        self.new_context_with_role(inner, IfaceRole::default())
    }

    pub fn new_context_with_role<T: Interface>(
        &mut self,
        inner: T,
        role: IfaceRole,
    ) -> InterfaceContext<T> {
        self.new_context_with_role_and_mode(inner, role, InterfaceMode::default())
    }

    pub fn new_context_with_role_and_mode<T: Interface>(
        &mut self,
        inner: T,
        role: IfaceRole,
        mode: InterfaceMode,
    ) -> InterfaceContext<T> {
        let channel =
            self.new_channel_with_role_and_mode(DEFAULT_IFACE_TX_QUEUE_CAPACITY, role, mode);
        let inner = Arc::new(Mutex::new(inner));
        InterfaceContext::<T> { inner: inner.clone(), channel, cancel: self.cancel.clone() }
    }

    pub fn spawn<T: Interface, F, R>(&mut self, inner: T, worker: F) -> AddressHash
    where
        F: FnOnce(InterfaceContext<T>) -> R,
        R: std::future::Future<Output = ()> + Send + 'static,
        R::Output: Send + 'static,
    {
        self.spawn_as(inner, worker, IfaceRole::default())
    }

    pub fn spawn_as<T: Interface, F, R>(
        &mut self,
        inner: T,
        worker: F,
        role: IfaceRole,
    ) -> AddressHash
    where
        F: FnOnce(InterfaceContext<T>) -> R,
        R: std::future::Future<Output = ()> + Send + 'static,
        R::Output: Send + 'static,
    {
        self.spawn_as_with_mode(inner, worker, role, InterfaceMode::default())
    }

    pub fn spawn_as_with_mode<T: Interface, F, R>(
        &mut self,
        inner: T,
        worker: F,
        role: IfaceRole,
        mode: InterfaceMode,
    ) -> AddressHash
    where
        F: FnOnce(InterfaceContext<T>) -> R,
        R: std::future::Future<Output = ()> + Send + 'static,
        R::Output: Send + 'static,
    {
        let context = self.new_context_with_role_and_mode(inner, role, mode);
        let address = *context.channel.address();

        task::spawn(worker(context));

        address
    }

    pub fn role(&self, address: &AddressHash) -> Option<IfaceRole> {
        self.ifaces.iter().find(|i| i.address == *address).map(|i| i.role)
    }

    pub fn mode(&self, address: &AddressHash) -> Option<InterfaceMode> {
        self.ifaces.iter().find(|i| i.address == *address).map(|i| i.mode)
    }

    pub fn outgoing(&self, address: &AddressHash) -> Option<bool> {
        self.ifaces.iter().find(|i| i.address == *address).map(|i| i.outgoing)
    }

    pub fn announce_pacing(&self, address: &AddressHash) -> Option<(u64, u64)> {
        self.ifaces
            .iter()
            .find(|i| i.address == *address)
            .map(|i| (i.announce_bitrate_bps, i.announce_cap_percent))
    }

    pub fn full_hash(&self, address: &AddressHash) -> Option<Hash> {
        self.ifaces.iter().find(|i| i.address == *address).map(|i| i.full_hash)
    }

    pub fn address_for_full_hash(&self, full_hash: &Hash) -> Option<AddressHash> {
        self.ifaces.iter().find(|i| i.full_hash == *full_hash).map(|i| i.address)
    }

    pub fn set_mode(&mut self, address: AddressHash, mode: InterfaceMode) -> bool {
        if let Some(iface) = self.ifaces.iter_mut().find(|i| i.address == address) {
            iface.mode = mode;
            true
        } else {
            false
        }
    }

    pub fn set_outgoing(&mut self, address: AddressHash, outgoing: bool) -> bool {
        if let Some(iface) = self.ifaces.iter_mut().find(|i| i.address == address) {
            iface.outgoing = outgoing;
            true
        } else {
            false
        }
    }

    pub fn set_announce_pacing(
        &mut self,
        address: AddressHash,
        bitrate_bps: u64,
        cap_percent: u64,
    ) -> bool {
        if let Some(iface) = self.ifaces.iter_mut().find(|i| i.address == address) {
            iface.announce_bitrate_bps = bitrate_bps;
            iface.announce_cap_percent = cap_percent;
            true
        } else {
            false
        }
    }

    /// Register a virtual iface that shares its tx channel with an
    /// existing host iface. Used by the UDP multicast iface to pin
    /// per-peer point-to-point routes without spawning additional
    /// sockets. Returns `None` if `host` is not registered.
    ///
    /// The returned `AddressHash` is used by the transport's routing
    /// tables (path_table, link ingress_iface) so `Direct` tx
    /// targeting this address is delivered into the host iface's tx
    /// task, which then sends the packet unicast to the pinned peer
    /// via its own socket.
    pub fn register_virtual_iface(
        &mut self,
        host: AddressHash,
        role: IfaceRole,
    ) -> Option<AddressHash> {
        let host_iface = self.ifaces.iter().find(|i| i.address == host)?;
        let host_tx = host_iface.tx_send.clone();
        let mode = host_iface.mode;

        // Virtual iface gets its own CancellationToken so it can be
        // stopped (and GC'd by `cleanup()`) independently of the host.
        // A cloned token would share cancel state with the host,
        // meaning stopping one would tear down the other.
        let stop = CancellationToken::new();

        self.counter += 1;
        let counter_bytes = self.counter.to_le_bytes();
        let full_hash = Hash::new_from_slice(&counter_bytes[..]);
        let address = AddressHash::new_from_hash(&full_hash);

        log::debug!(
            "register virtual iface {} on host {} role={:?} mode={:?}",
            address,
            host,
            role,
            mode
        );

        self.ifaces.push(LocalInterface {
            address,
            full_hash,
            tx_send: host_tx,
            stop,
            role,
            mode,
            outgoing: host_iface.outgoing,
            announce_queue: VecDeque::new(),
            announce_allowed_at: Instant::now(),
            announce_bitrate_bps: host_iface.announce_bitrate_bps,
            announce_cap_percent: host_iface.announce_cap_percent,
        });

        Some(address)
    }

    pub fn receiver(&self) -> Arc<tokio::sync::Mutex<InterfaceRxReceiver>> {
        self.rx_recv.clone()
    }

    pub fn cleanup(&mut self) {
        self.ifaces.retain(|iface| !iface.stop.is_cancelled());
    }

    pub fn stop_interface(&mut self, address: AddressHash) -> bool {
        let mut stopped = false;
        for iface in &self.ifaces {
            if iface.address == address {
                iface.stop.cancel();
                stopped = true;
            }
        }
        self.cleanup();
        stopped
    }

    /// Test-only: returns the number of tracked ifaces (live or stopped).
    #[cfg(test)]
    pub fn iface_count(&self) -> usize {
        self.ifaces.len()
    }

    /// Test-only: returns true if any tracked iface has the given role.
    #[cfg(test)]
    pub fn has_role(&self, role: IfaceRole) -> bool {
        self.ifaces.iter().any(|i| i.role == role)
    }

    async fn send_to_iface(iface: &LocalInterface, message: TxMessage) -> bool {
        match iface.tx_send.try_send(message) {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Full(_)) => {
                match tokio::time::timeout(
                    Duration::from_millis(IFACE_TX_ENQUEUE_TIMEOUT_MS),
                    iface.tx_send.send(message),
                )
                .await
                {
                    Ok(Ok(())) => {
                        if tx_diag_enabled() {
                            log::warn!(
                                "recovered from full tx queue on {} for {:?}",
                                iface.address,
                                message.tx_type
                            );
                        }
                        true
                    }
                    Ok(Err(_)) => {
                        log::warn!(
                            "tx queue closed on {} for {:?}",
                            iface.address,
                            message.tx_type
                        );
                        false
                    }
                    Err(_) => {
                        log::warn!(
                            "tx queue full timeout on {} for {:?}",
                            iface.address,
                            message.tx_type
                        );
                        false
                    }
                }
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                log::warn!("tx queue closed on {} for {:?}", iface.address, message.tx_type);
                false
            }
        }
    }

    fn queue_announce(iface: &mut LocalInterface, message: TxMessage, now: Instant) -> bool {
        iface
            .announce_queue
            .retain(|entry| now.duration_since(entry.queued_at) <= QUEUED_ANNOUNCE_LIFE);

        let emitted = announce_emitted(&message.packet);
        if let Some(existing) = iface
            .announce_queue
            .iter_mut()
            .find(|entry| entry.message.packet.destination == message.packet.destination)
        {
            if emitted > existing.emitted {
                existing.message = message;
                existing.queued_at = now;
                existing.emitted = emitted;
            }
            return true;
        }

        if iface.announce_queue.len() >= MAX_QUEUED_ANNOUNCES_PER_IFACE {
            log::warn!(
                "dropping announce for {} on {} because announce queue is full",
                message.packet.destination,
                iface.address
            );
            return false;
        }

        iface.announce_queue.push_back(QueuedAnnounce { message, queued_at: now, emitted });
        true
    }

    fn pop_next_announce(iface: &mut LocalInterface, now: Instant) -> Option<TxMessage> {
        iface
            .announce_queue
            .retain(|entry| now.duration_since(entry.queued_at) <= QUEUED_ANNOUNCE_LIFE);
        let index = iface
            .announce_queue
            .iter()
            .enumerate()
            .min_by_key(|(_, entry)| (entry.message.packet.header.hops, entry.queued_at))
            .map(|(index, _)| index)?;
        iface.announce_queue.remove(index).map(|entry| entry.message)
    }

    pub async fn release_queued_announces(&mut self) -> TxDispatchTrace {
        let mut trace = TxDispatchTrace::default();
        let now = Instant::now();

        for iface in &mut self.ifaces {
            if iface.stop.is_cancelled()
                || !iface.outgoing
                || iface.announce_queue.is_empty()
                || now < iface.announce_allowed_at
            {
                continue;
            }

            let Some(message) = Self::pop_next_announce(iface, now) else {
                continue;
            };

            trace.matched_ifaces += 1;
            iface.announce_allowed_at = now
                + announce_wait(
                    &message.packet,
                    iface.announce_bitrate_bps,
                    iface.announce_cap_percent,
                );
            if Self::send_to_iface(iface, message).await {
                trace.sent_ifaces += 1;
            } else {
                trace.failed_ifaces += 1;
            }
        }

        trace
    }

    pub async fn send(&mut self, message: TxMessage) -> TxDispatchTrace {
        self.send_with_announce_policy(message, None).await
    }

    pub async fn send_with_announce_policy(
        &mut self,
        message: TxMessage,
        announce_policy: Option<AnnounceBroadcastPolicy>,
    ) -> TxDispatchTrace {
        let mut trace = TxDispatchTrace::default();
        for iface in &mut self.ifaces {
            let should_send = match message.tx_type {
                TxMessageType::Broadcast(address) => {
                    // VirtualUnicast ifaces share their tx channel with a
                    // host (multicast) iface, so broadcasting to both
                    // would double-enqueue each packet. Skip them — the
                    // host iface will carry the broadcast.
                    if iface.role == IfaceRole::VirtualUnicast {
                        false
                    } else {
                        let mut should_send = true;
                        if let Some(address) = address {
                            should_send = address != iface.address;
                        }
                        if should_send {
                            should_send = allows_announce_broadcast(
                                &message.packet,
                                iface.mode,
                                announce_policy,
                            );
                        }
                        should_send
                    }
                }
                TxMessageType::Direct(address) => address == iface.address,
            };

            if should_send && iface.outgoing && !iface.stop.is_cancelled() {
                trace.matched_ifaces += 1;
                let is_paced_announce = message.packet.header.packet_type == PacketType::Announce
                    && message.packet.header.hops > 0
                    && matches!(message.tx_type, TxMessageType::Broadcast(_));
                let now = Instant::now();
                if is_paced_announce
                    && (!iface.announce_queue.is_empty() || now < iface.announce_allowed_at)
                {
                    if Self::queue_announce(iface, message, now) {
                        trace.queued_ifaces += 1;
                    } else {
                        trace.failed_ifaces += 1;
                    }
                    continue;
                }

                if is_paced_announce {
                    iface.announce_allowed_at = now
                        + announce_wait(
                            &message.packet,
                            iface.announce_bitrate_bps,
                            iface.announce_cap_percent,
                        );
                }

                if Self::send_to_iface(iface, message).await {
                    trace.sent_ifaces += 1;
                } else {
                    trace.failed_ifaces += 1;
                }
            }
        }

        trace
    }
}

include!("iface_tests.rs");
