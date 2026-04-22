pub mod driver;
pub mod hdlc;
pub mod serial;
pub mod tcp_client;
pub mod tcp_server;
pub mod udp;

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task;
use tokio_util::sync::CancellationToken;

use crate::hash::AddressHash;
use crate::hash::Hash;
use crate::packet::Packet;

pub use driver::{InterfaceDriver, InterfaceDriverFactory};

pub type InterfaceTxSender = mpsc::Sender<TxMessage>;
pub type InterfaceTxReceiver = mpsc::Receiver<TxMessage>;

pub type InterfaceRxSender = mpsc::Sender<RxMessage>;
pub type InterfaceRxReceiver = mpsc::Receiver<RxMessage>;

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
    pub failed_ifaces: usize,
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
    tx_send: InterfaceTxSender,
    stop: CancellationToken,
    role: IfaceRole,
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

const DEFAULT_IFACE_TX_QUEUE_CAPACITY: usize = 128;
const IFACE_TX_ENQUEUE_TIMEOUT_MS: u64 = 200;

fn tx_diag_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("RETICULUMD_DIAGNOSTICS")
            .or_else(|_| std::env::var("RETICULUM_TRANSPORT_DIAGNOSTICS"))
            .ok()
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on" | "debug"
                )
            })
            .unwrap_or(false)
    })
}

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
        self.counter += 1;

        let counter_bytes = self.counter.to_le_bytes();
        let address = AddressHash::new_from_hash(&Hash::new_from_slice(&counter_bytes[..]));

        let (tx_send, tx_recv) = InterfaceChannel::make_tx_channel(tx_cap);

        log::debug!("iface: create channel {} role={:?}", address, role);

        let stop = CancellationToken::new();

        self.ifaces.push(LocalInterface { address, tx_send, stop: stop.clone(), role });

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
        let channel = self.new_channel_with_role(DEFAULT_IFACE_TX_QUEUE_CAPACITY, role);
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
        let context = self.new_context_with_role(inner, role);
        let address = *context.channel.address();

        task::spawn(worker(context));

        address
    }

    pub fn role(&self, address: &AddressHash) -> Option<IfaceRole> {
        self.ifaces.iter().find(|i| i.address == *address).map(|i| i.role)
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
        let host_tx = self.ifaces.iter().find(|i| i.address == host).map(|i| i.tx_send.clone())?;

        // Virtual iface gets its own CancellationToken so it can be
        // stopped (and GC'd by `cleanup()`) independently of the host.
        // A cloned token would share cancel state with the host,
        // meaning stopping one would tear down the other.
        let stop = CancellationToken::new();

        self.counter += 1;
        let counter_bytes = self.counter.to_le_bytes();
        let address = AddressHash::new_from_hash(&Hash::new_from_slice(&counter_bytes[..]));

        log::debug!("iface: register virtual iface {} on host {} role={:?}", address, host, role);

        self.ifaces.push(LocalInterface { address, tx_send: host_tx, stop, role });

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

    pub async fn send(&self, message: TxMessage) -> TxDispatchTrace {
        let mut trace = TxDispatchTrace::default();
        for iface in &self.ifaces {
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
                        should_send
                    }
                }
                TxMessageType::Direct(address) => address == iface.address,
            };

            if should_send && !iface.stop.is_cancelled() {
                trace.matched_ifaces += 1;
                match iface.tx_send.try_send(message) {
                    Ok(()) => {
                        trace.sent_ifaces += 1;
                    }
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        match tokio::time::timeout(
                            Duration::from_millis(IFACE_TX_ENQUEUE_TIMEOUT_MS),
                            iface.tx_send.send(message),
                        )
                        .await
                        {
                            Ok(Ok(())) => {
                                trace.sent_ifaces += 1;
                                if tx_diag_enabled() {
                                    log::warn!(
                                        "iface: recovered from full tx queue on {} for {:?}",
                                        iface.address,
                                        message.tx_type
                                    );
                                }
                            }
                            Ok(Err(_)) => {
                                trace.failed_ifaces += 1;
                                log::warn!(
                                    "iface: tx queue closed on {} for {:?}",
                                    iface.address,
                                    message.tx_type
                                );
                            }
                            Err(_) => {
                                trace.failed_ifaces += 1;
                                log::warn!(
                                    "iface: tx queue full timeout on {} for {:?}",
                                    iface.address,
                                    message.tx_type
                                );
                            }
                        }
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        trace.failed_ifaces += 1;
                        log::warn!(
                            "iface: tx queue closed on {} for {:?}",
                            iface.address,
                            message.tx_type
                        );
                    }
                }
            }
        }

        trace
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A zero-sized Interface impl for tests that only exercise the
    // manager's bookkeeping (registration, role tagging, lookup). No
    // actual worker task is spawned by these tests; we only drive
    // `new_channel_with_role` and the `role`/`stop_interface`/`cleanup`
    // helpers.

    #[test]
    fn new_channel_defaults_to_unicast_role() {
        let mut mgr = InterfaceManager::new(16);
        let channel = mgr.new_channel(16);
        assert_eq!(mgr.role(channel.address()), Some(IfaceRole::Unicast));
    }

    #[test]
    fn new_channel_with_role_records_multicast_tag() {
        let mut mgr = InterfaceManager::new(16);
        let channel = mgr.new_channel_with_role(16, IfaceRole::Multicast);
        assert_eq!(mgr.role(channel.address()), Some(IfaceRole::Multicast));
        assert!(mgr.has_role(IfaceRole::Multicast));
    }

    #[test]
    fn role_returns_none_for_unknown_address() {
        let mgr = InterfaceManager::new(16);
        let fake = AddressHash::new_from_hash(&Hash::new_from_slice(&[0u8; 32]));
        assert_eq!(mgr.role(&fake), None);
    }

    #[test]
    fn each_new_channel_gets_a_unique_address_hash() {
        let mut mgr = InterfaceManager::new(16);
        let a = *mgr.new_channel(16).address();
        let b = *mgr.new_channel(16).address();
        let c = *mgr.new_channel_with_role(16, IfaceRole::Multicast).address();
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_ne!(b, c);
    }

    #[test]
    fn stop_interface_marks_iface_stopped_and_cleanup_removes_it() {
        let mut mgr = InterfaceManager::new(16);
        let channel = mgr.new_channel_with_role(16, IfaceRole::Multicast);
        let addr = *channel.address();
        assert_eq!(mgr.iface_count(), 1);
        assert!(mgr.stop_interface(addr));
        // stop_interface also calls cleanup() which prunes cancelled ifaces.
        assert_eq!(mgr.iface_count(), 0);
    }

    #[test]
    fn iface_source_default_is_none() {
        let src = IfaceSource::default();
        assert_eq!(src, IfaceSource::None);
    }

    #[test]
    fn iface_role_default_is_unicast() {
        let role = IfaceRole::default();
        assert_eq!(role, IfaceRole::Unicast);
    }
}
