use crate::buffer::InputBuffer;

use crate::hash::AddressHash;

use crate::iface::auto::{
    AutoAdoptedInterfaceChange, AutoDataListenerBinding, AutoDiscoveryEvent,
    AutoDiscoveryListenerBinding, AutoDiscoveryRejectReason, AutoDiscoveryState,
    AutoInboundPacketDeduplicator, AutoInterfaceAdoptedDevice, AutoInterfaceConfig,
    AutoInterfaceDeviceCandidate, AutoInterfaceDeviceFilter, AutoInterfacePlatform,
    AutoInterfaceTiming, AutoLinkLocalAddressUpdate, AutoMulticastCarrierEvent,
    AutoPeerInboundDecision, AutoPeeringPacket, AutoPeeringPacketKind, AutoRuntimeState,
    AutoStartupPlan,
};

use crate::iface::{
    IfaceRole, IfaceSource, InterfaceChannel, InterfaceManager, InterfaceRxSender,
    InterfaceTxReceiver, RxMessage, TxMessage, TxMessageType,
};

use crate::packet::Packet;

use serde_json::{json, Value as JsonValue};

use std::collections::BTreeMap;

use std::net::{IpAddr, SocketAddr, SocketAddrV6};

use std::sync::Arc;

use std::time::Instant;

#[derive(Clone)]
pub struct AutoRuntimePlan {
    pub config: AutoInterfaceConfig,
    pub platform: AutoInterfacePlatform,
    pub device_filter: AutoInterfaceDeviceFilter,
    pub candidates: Vec<AutoInterfaceDeviceCandidate>,
    pub adopted_devices: Vec<AutoInterfaceAdoptedDevice>,
    peering_packets: Vec<AutoPeeringPacket>,
    pub startup_plan: AutoStartupPlan,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoInterfaceIndexResolver {
    indexes_by_ifname: BTreeMap<String, u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoPeerAnnounceDatagram {
    pub kind: AutoPeeringPacketKind,
    pub ifname: String,
    pub source_link_local_address: String,
    pub destination_address: String,
    pub destination_port: u16,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoPeerAnnounceSocketTarget {
    pub host: String,
    pub port: u16,
    pub scope_ifname: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoDiscoverySocketKind {
    Unicast,
    Multicast,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoDiscoverySocketBindTarget {
    pub kind: AutoDiscoverySocketKind,
    pub ifname: String,
    pub bind_host: String,
    pub bind_port: u16,
    pub scope_ifname: Option<String>,
    pub multicast_group_host: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoDataSocketBindTarget {
    pub ifname: String,
    pub bind_host: String,
    pub bind_port: u16,
    pub scope_ifname: Option<String>,
}

#[allow(dead_code)]
pub struct AutoBoundDiscoverySocket {
    pub kind: AutoDiscoverySocketKind,
    pub ifname: String,
    pub bind_addr: SocketAddr,
    pub multicast_group_addr: Option<SocketAddr>,
    pub socket: tokio::net::UdpSocket,
}

#[allow(dead_code)]
pub struct AutoBoundDataSocket {
    pub ifname: String,
    pub bind_addr: SocketAddr,
    pub socket: Arc<tokio::net::UdpSocket>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoDiscoveryDatagram {
    pub kind: AutoDiscoverySocketKind,
    pub ifname: String,
    pub bind_addr: SocketAddr,
    pub multicast_group_addr: Option<SocketAddr>,
    pub source_addr: SocketAddr,
    pub payload: Vec<u8>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoPeerDataDatagram {
    pub ifname: String,
    pub bind_addr: SocketAddr,
    pub source_addr: SocketAddr,
    pub payload: Vec<u8>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoProcessedDiscoveryDatagram {
    pub datagram: AutoDiscoveryDatagram,
    pub source_address: String,
    pub event: AutoDiscoveryEvent,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoProcessedPeerDataDatagram {
    pub datagram: AutoPeerDataDatagram,
    pub peer_address: String,
    pub decision: AutoPeerInboundDecision,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoPeerDataForwardResult {
    NotForwarded,
    Delivered,
    VirtualIfaceUnavailable,
    DecodeFailed,
    RxChannelClosed,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoPeerDataRuntimeSummary {
    pub ifname: String,
    pub peer_address: String,
    pub decision: String,
    pub forwarding: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoDiscoveryLoopEvent {
    Processed(AutoProcessedDiscoveryDatagram),
    Rejected {
        datagram: AutoDiscoveryDatagram,
        source_address: String,
        reason: AutoDiscoveryRejectReason,
    },
    ReceiveFailed {
        ifname: String,
        kind: AutoDiscoverySocketKind,
        bind_addr: SocketAddr,
        error: String,
    },
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoPeerDataLoopEvent {
    Processed(AutoProcessedPeerDataDatagram),
    ReceiveFailed { ifname: String, bind_addr: SocketAddr, error: String },
}

#[allow(dead_code)]
#[derive(Clone)]
struct AutoPeerDataReceiveLoopRuntime {
    state: Arc<tokio::sync::Mutex<AutoDiscoveryState>>,
    dedupe: Arc<tokio::sync::Mutex<AutoInboundPacketDeduplicator>>,
    transport: Option<AutoInterfaceTransportBridge>,
    runtime_status: Option<AutoRuntimeStatusHandle>,
    events: tokio::sync::mpsc::Sender<AutoPeerDataLoopEvent>,
    shutdown: tokio::sync::watch::Receiver<bool>,
    started_at: Instant,
}

#[allow(dead_code)]
pub struct AutoDiscoveryListenerSupervisor {
    plan: AutoRuntimePlan,
    state: Arc<tokio::sync::Mutex<AutoDiscoveryState>>,
    shutdown: tokio::sync::watch::Receiver<bool>,
    started_at: Instant,
    listeners: BTreeMap<String, AutoDiscoveryListenerHandle>,
    pending_stops: Vec<tokio::task::JoinHandle<()>>,
}

#[allow(dead_code)]
struct AutoDiscoveryListenerHandle {
    joins: Vec<tokio::task::JoinHandle<()>>,
}

#[allow(dead_code)]
pub struct AutoPeerDataListenerSupervisor {
    plan: AutoRuntimePlan,
    state: Arc<tokio::sync::Mutex<AutoDiscoveryState>>,
    dedupe: Arc<tokio::sync::Mutex<AutoInboundPacketDeduplicator>>,
    transport: Option<AutoInterfaceTransportBridge>,
    runtime_status: Option<AutoRuntimeStatusHandle>,
    shutdown: tokio::sync::watch::Receiver<bool>,
    started_at: Instant,
    listeners: BTreeMap<String, AutoPeerDataListenerHandle>,
    pending_stops: Vec<tokio::task::JoinHandle<()>>,
}

#[allow(dead_code)]
struct AutoPeerDataListenerHandle {
    socket: Arc<tokio::net::UdpSocket>,
    join: tokio::task::JoinHandle<()>,
}

/// A running AutoInterface: what came up, and the means to take it down.
///
/// The daemon never stops one, but an embedder that rebuilds its transport
/// has to, and the sockets must be closed before a replacement can bind the
/// same ports. Dropping this detaches nothing: the runtime keeps running.
pub struct AutoDiscoveryRuntime {
    pub summary: AutoDiscoveryRuntimeSummary,
    shutdown: Arc<tokio::sync::watch::Sender<bool>>,
    supervisor: tokio::task::JoinHandle<()>,
}

impl AutoDiscoveryRuntime {
    /// Signals every listener and scheduler to stop and waits for the
    /// supervisor to release the sockets.
    pub async fn stop(self) {
        self.shutdown.send_replace(true);
        if let Err(err) = self.supervisor.await {
            log::warn!("[auto] runtime supervisor stopped: {err}");
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoDiscoveryRuntimeSummary {
    pub bound_socket_count: usize,
    pub receive_loop_count: usize,
    pub initial_peer_announce_count: usize,
    pub repeat_peer_announce_scheduler_count: usize,
    pub peer_job_scheduler_count: usize,
    pub adopted_interface_reconciler_count: usize,
    pub data_socket_count: usize,
    pub data_receive_loop_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoPeerJobRuntimeSummary {
    pub expired_peer_count: usize,
    pub reverse_peer_announce_count: usize,
    pub missing_initial_echo_count: usize,
    pub carrier_changed: bool,
    pub carrier_event_count: usize,
    pub carrier_events: Vec<AutoMulticastCarrierEvent>,
    pub peer_count_after: usize,
}

#[derive(Clone)]
pub struct AutoRuntimeStatusHandle {
    inner: Arc<std::sync::Mutex<AutoRuntimeStatus>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AutoRuntimeStatus {
    state: AutoRuntimeState,
    started_at: Instant,
    carrier_events: Vec<AutoMulticastCarrierEvent>,
    last_peer_job: Option<AutoPeerJobRuntimeSummary>,
    link_local_update: Option<AutoLinkLocalAddressUpdate>,
    adopted_devices: Vec<AutoInterfaceAdoptedDevice>,
    adopted_add_count: u64,
    adopted_remove_count: u64,
    link_local_replacement_count: u64,
    last_adopted_change: Option<AutoAdoptedInterfaceChange>,
    peer_data_admitted_count: u64,
    peer_data_duplicate_count: u64,
    peer_data_unknown_count: u64,
    peer_data_delivered_count: u64,
    peer_data_decode_failed_count: u64,
    peer_data_rx_closed_count: u64,
    last_peer_data: Option<AutoPeerDataRuntimeSummary>,
}

#[allow(dead_code)]
pub struct AutoInterfaceTransportRuntime {
    bridge: AutoInterfaceTransportBridge,
    tx_channel: InterfaceTxReceiver,
}

#[allow(dead_code)]
#[derive(Clone)]
pub struct AutoInterfaceTransportBridge {
    host_iface: AddressHash,
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    rx_channel: InterfaceRxSender,
    peer_ifaces: Arc<tokio::sync::Mutex<BTreeMap<SocketAddr, AddressHash>>>,
    outbound_routes: Arc<tokio::sync::Mutex<BTreeMap<AddressHash, AutoPeerOutboundRoute>>>,
}

#[allow(dead_code)]
#[derive(Clone)]
struct AutoPeerOutboundRoute {
    socket: Arc<tokio::net::UdpSocket>,
    destination: SocketAddr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoResolvedMulticastDiscoveryBind {
    pub bind_addr: SocketAddr,
    pub multicast_group_addr: SocketAddr,
    pub multicast_scope_id: u32,
}

const AUTO_DISCOVERY_DATAGRAM_BUFFER_SIZE: usize = 2_048;

impl AutoBoundDiscoverySocket {
    #[allow(dead_code)]
    pub async fn recv_discovery_datagram(&self) -> Result<AutoDiscoveryDatagram, String> {
        let mut payload = vec![0u8; AUTO_DISCOVERY_DATAGRAM_BUFFER_SIZE];
        let (received, source_addr) = self.socket.recv_from(&mut payload).await.map_err(|err| {
            format!(
                "receive auto discovery datagram iface={} kind={} bind={} failed: {err}",
                self.ifname,
                discovery_socket_kind(self.kind),
                self.bind_addr
            )
        })?;
        payload.truncate(received);
        Ok(AutoDiscoveryDatagram {
            kind: self.kind,
            ifname: self.ifname.clone(),
            bind_addr: self.bind_addr,
            multicast_group_addr: self.multicast_group_addr,
            source_addr,
            payload,
        })
    }
}

impl AutoBoundDataSocket {
    #[allow(dead_code)]
    pub async fn recv_peer_data_datagram(&self) -> Result<AutoPeerDataDatagram, String> {
        let mut payload = vec![0u8; AUTO_DISCOVERY_DATAGRAM_BUFFER_SIZE];
        let (received, source_addr) = self.socket.recv_from(&mut payload).await.map_err(|err| {
            format!(
                "receive auto peer data datagram iface={} bind={} failed: {err}",
                self.ifname, self.bind_addr
            )
        })?;
        payload.truncate(received);
        Ok(AutoPeerDataDatagram {
            ifname: self.ifname.clone(),
            bind_addr: self.bind_addr,
            source_addr,
            payload,
        })
    }
}

impl AutoInterfaceIndexResolver {
    #[allow(dead_code)]
    pub fn from_system() -> Result<Self, String> {
        let interfaces =
            if_addrs::get_if_addrs().map_err(|err| format!("enumerate interfaces: {err}"))?;
        Ok(Self::from_index_entries(interfaces.into_iter().map(|iface| (iface.name, iface.index))))
    }

    fn from_index_entries(entries: impl IntoIterator<Item = (String, Option<u32>)>) -> Self {
        let indexes_by_ifname = entries
            .into_iter()
            .filter_map(|(ifname, index)| index.map(|index| (ifname, index)))
            .collect();
        Self { indexes_by_ifname }
    }

    #[allow(dead_code)]
    pub fn resolve(&self, ifname: &str) -> Result<u32, String> {
        self.indexes_by_ifname
            .get(ifname)
            .copied()
            .ok_or_else(|| format!("interface index for {ifname} was not found"))
    }
}

impl AutoPeerAnnounceDatagram {
    pub fn socket_target(&self) -> AutoPeerAnnounceSocketTarget {
        let (host, explicit_scope) = split_ipv6_scope(&self.destination_address);
        let scope_ifname = if let Some(scope) = explicit_scope {
            Some(scope.to_string())
        } else if self.kind == AutoPeeringPacketKind::Multicast
            && is_link_scope_ipv6_multicast(host)
        {
            Some(self.ifname.clone())
        } else {
            None
        };
        AutoPeerAnnounceSocketTarget {
            host: host.to_string(),
            port: self.destination_port,
            scope_ifname,
        }
    }

    pub fn destination_socket_target(&self) -> String {
        self.socket_target().display()
    }
}

impl AutoPeerAnnounceSocketTarget {
    pub fn display(&self) -> String {
        let host = if let Some(scope_ifname) = &self.scope_ifname {
            format!("{}%{scope_ifname}", self.host)
        } else {
            self.host.clone()
        };
        socket_target(&host, self.port)
    }

    // Shared by startup and tests to keep scoped IPv6 target resolution
    // deterministic before a UDP send is attempted.
    #[allow(dead_code)]
    pub fn resolve_socket_addr(
        &self,
        mut scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<SocketAddr, String> {
        let ip = self.host.parse::<IpAddr>().map_err(|err| {
            format!("parse auto peer announce destination host {}: {err}", self.host)
        })?;
        match (ip, self.scope_ifname.as_deref()) {
            (IpAddr::V6(host), Some(ifname)) => {
                let scope_id = scope_id_for_ifname(ifname).map_err(|err| {
                    format!("resolve auto peer announce scope id for interface {ifname}: {err}")
                })?;
                Ok(SocketAddr::V6(SocketAddrV6::new(host, self.port, 0, scope_id)))
            }
            (IpAddr::V6(host), None) => {
                Ok(SocketAddr::V6(SocketAddrV6::new(host, self.port, 0, 0)))
            }
            (IpAddr::V4(host), None) => Ok(SocketAddr::from((host, self.port))),
            (IpAddr::V4(_), Some(ifname)) => Err(format!(
                "auto peer announce IPv4 destination {} cannot use scope interface {ifname}",
                self.host
            )),
        }
    }
}
