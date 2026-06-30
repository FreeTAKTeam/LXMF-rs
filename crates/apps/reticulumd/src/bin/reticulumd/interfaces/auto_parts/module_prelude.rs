use reticulum_daemon::config::InterfaceConfig;

use rns_transport::buffer::InputBuffer;

use rns_transport::hash::AddressHash;

use rns_transport::iface::auto::{
    AutoAdoptedInterfaceChange, AutoDataListenerBinding, AutoDiscoveryEvent,
    AutoDiscoveryListenerBinding,
    AutoDiscoveryRejectReason, AutoDiscoveryScope, AutoDiscoveryState,
    AutoInboundPacketDeduplicator, AutoInterfaceAdoptedDevice, AutoInterfaceConfig,
    AutoInterfaceDeviceCandidate, AutoInterfaceDeviceFilter, AutoInterfacePlatform,
    AutoInterfaceTiming, AutoLinkLocalAddressUpdate, AutoMulticastCarrierEvent,
    AutoPeerInboundDecision, AutoPeeringPacket, AutoPeeringPacketKind, AutoRuntimeState,
    AutoStartupPlan, MulticastAddressType,
};

use rns_transport::iface::{
    IfaceRole, IfaceSource, InterfaceChannel, InterfaceManager, InterfaceRxSender,
    InterfaceTxReceiver, RxMessage, TxMessage, TxMessageType,
};

use rns_transport::packet::Packet;

use serde_json::{json, Value as JsonValue};

use std::collections::BTreeMap;

use std::net::{IpAddr, SocketAddr, SocketAddrV6};

use std::sync::Arc;

use std::time::Instant;

#[derive(Clone)]
pub(crate) struct AutoDaemonStartupPlan {
    pub(crate) config: AutoInterfaceConfig,
    pub(crate) platform: AutoInterfacePlatform,
    pub(crate) device_filter: AutoInterfaceDeviceFilter,
    pub(crate) candidates: Vec<AutoInterfaceDeviceCandidate>,
    pub(crate) adopted_devices: Vec<AutoInterfaceAdoptedDevice>,
    peering_packets: Vec<AutoPeeringPacket>,
    pub(crate) startup_plan: AutoStartupPlan,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutoInterfaceIndexResolver {
    indexes_by_ifname: BTreeMap<String, u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutoPeerAnnounceDatagram {
    pub(crate) kind: AutoPeeringPacketKind,
    pub(crate) ifname: String,
    pub(crate) source_link_local_address: String,
    pub(crate) destination_address: String,
    pub(crate) destination_port: u16,
    pub(crate) payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutoPeerAnnounceSocketTarget {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) scope_ifname: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoDiscoverySocketKind {
    Unicast,
    Multicast,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutoDiscoverySocketBindTarget {
    pub(crate) kind: AutoDiscoverySocketKind,
    pub(crate) ifname: String,
    pub(crate) bind_host: String,
    pub(crate) bind_port: u16,
    pub(crate) scope_ifname: Option<String>,
    pub(crate) multicast_group_host: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutoDataSocketBindTarget {
    pub(crate) ifname: String,
    pub(crate) bind_host: String,
    pub(crate) bind_port: u16,
    pub(crate) scope_ifname: Option<String>,
}

#[allow(dead_code)]
pub(crate) struct AutoBoundDiscoverySocket {
    pub(crate) kind: AutoDiscoverySocketKind,
    pub(crate) ifname: String,
    pub(crate) bind_addr: SocketAddr,
    pub(crate) multicast_group_addr: Option<SocketAddr>,
    pub(crate) socket: tokio::net::UdpSocket,
}

#[allow(dead_code)]
pub(crate) struct AutoBoundDataSocket {
    pub(crate) ifname: String,
    pub(crate) bind_addr: SocketAddr,
    pub(crate) socket: Arc<tokio::net::UdpSocket>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutoDiscoveryDatagram {
    pub(crate) kind: AutoDiscoverySocketKind,
    pub(crate) ifname: String,
    pub(crate) bind_addr: SocketAddr,
    pub(crate) multicast_group_addr: Option<SocketAddr>,
    pub(crate) source_addr: SocketAddr,
    pub(crate) payload: Vec<u8>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutoPeerDataDatagram {
    pub(crate) ifname: String,
    pub(crate) bind_addr: SocketAddr,
    pub(crate) source_addr: SocketAddr,
    pub(crate) payload: Vec<u8>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutoProcessedDiscoveryDatagram {
    pub(crate) datagram: AutoDiscoveryDatagram,
    pub(crate) source_address: String,
    pub(crate) event: AutoDiscoveryEvent,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutoProcessedPeerDataDatagram {
    pub(crate) datagram: AutoPeerDataDatagram,
    pub(crate) peer_address: String,
    pub(crate) decision: AutoPeerInboundDecision,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoPeerDataForwardResult {
    NotForwarded,
    Delivered,
    VirtualIfaceUnavailable,
    DecodeFailed,
    RxChannelClosed,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutoPeerDataRuntimeSummary {
    pub(crate) ifname: String,
    pub(crate) peer_address: String,
    pub(crate) decision: String,
    pub(crate) forwarding: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AutoDiscoveryLoopEvent {
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
pub(crate) enum AutoPeerDataLoopEvent {
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
pub(crate) struct AutoDiscoveryListenerSupervisor {
    plan: AutoDaemonStartupPlan,
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
pub(crate) struct AutoPeerDataListenerSupervisor {
    plan: AutoDaemonStartupPlan,
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

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutoDiscoveryRuntimeSummary {
    pub(crate) bound_socket_count: usize,
    pub(crate) receive_loop_count: usize,
    pub(crate) initial_peer_announce_count: usize,
    pub(crate) repeat_peer_announce_scheduler_count: usize,
    pub(crate) peer_job_scheduler_count: usize,
    pub(crate) adopted_interface_reconciler_count: usize,
    pub(crate) data_socket_count: usize,
    pub(crate) data_receive_loop_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutoPeerJobRuntimeSummary {
    pub(crate) expired_peer_count: usize,
    pub(crate) reverse_peer_announce_count: usize,
    pub(crate) missing_initial_echo_count: usize,
    pub(crate) carrier_changed: bool,
    pub(crate) carrier_event_count: usize,
    pub(crate) carrier_events: Vec<AutoMulticastCarrierEvent>,
    pub(crate) peer_count_after: usize,
}

#[derive(Clone)]
pub(crate) struct AutoRuntimeStatusHandle {
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
pub(crate) struct AutoInterfaceTransportRuntime {
    bridge: AutoInterfaceTransportBridge,
    tx_channel: InterfaceTxReceiver,
}

#[allow(dead_code)]
#[derive(Clone)]
pub(crate) struct AutoInterfaceTransportBridge {
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
pub(crate) struct AutoResolvedMulticastDiscoveryBind {
    pub(crate) bind_addr: SocketAddr,
    pub(crate) multicast_group_addr: SocketAddr,
    pub(crate) multicast_scope_id: u32,
}

const AUTO_DISCOVERY_DATAGRAM_BUFFER_SIZE: usize = 2_048;

impl AutoBoundDiscoverySocket {
    #[allow(dead_code)]
    pub(crate) async fn recv_discovery_datagram(&self) -> Result<AutoDiscoveryDatagram, String> {
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
    pub(crate) async fn recv_peer_data_datagram(&self) -> Result<AutoPeerDataDatagram, String> {
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
    pub(crate) fn from_system() -> Result<Self, String> {
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
    pub(crate) fn resolve(&self, ifname: &str) -> Result<u32, String> {
        self.indexes_by_ifname
            .get(ifname)
            .copied()
            .ok_or_else(|| format!("interface index for {ifname} was not found"))
    }
}

impl AutoPeerAnnounceDatagram {
    pub(crate) fn socket_target(&self) -> AutoPeerAnnounceSocketTarget {
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

    pub(crate) fn destination_socket_target(&self) -> String {
        self.socket_target().display()
    }
}

impl AutoPeerAnnounceSocketTarget {
    pub(crate) fn display(&self) -> String {
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
    pub(crate) fn resolve_socket_addr(
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
