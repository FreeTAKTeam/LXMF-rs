use reticulum_daemon::config::InterfaceConfig;
use rns_transport::buffer::InputBuffer;
use rns_transport::hash::AddressHash;
use rns_transport::iface::auto::{
    AutoDataListenerBinding, AutoDiscoveryEvent, AutoDiscoveryListenerBinding,
    AutoDiscoveryRejectReason, AutoDiscoveryScope, AutoDiscoveryState,
    AutoInboundPacketDeduplicator, AutoInterfaceAdoptedDevice, AutoInterfaceConfig,
    AutoInterfaceDeviceCandidate, AutoInterfaceDeviceFilter, AutoInterfacePlatform,
    AutoInterfaceTiming, AutoPeerInboundDecision, AutoPeeringPacket, AutoPeeringPacketKind,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutoDiscoveryRuntimeSummary {
    pub(crate) bound_socket_count: usize,
    pub(crate) receive_loop_count: usize,
    pub(crate) initial_peer_announce_count: usize,
    pub(crate) repeat_peer_announce_scheduler_count: usize,
    pub(crate) peer_job_scheduler_count: usize,
    pub(crate) data_socket_count: usize,
    pub(crate) data_receive_loop_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AutoPeerJobRuntimeSummary {
    pub(crate) expired_peer_count: usize,
    pub(crate) reverse_peer_announce_count: usize,
    pub(crate) missing_initial_echo_count: usize,
    pub(crate) carrier_event_count: usize,
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

impl AutoInterfaceTransportRuntime {
    #[allow(dead_code)]
    pub(crate) fn from_channel(
        channel: InterfaceChannel,
        iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    ) -> Self {
        let host_iface = channel.address;
        Self {
            bridge: AutoInterfaceTransportBridge {
                host_iface,
                iface_manager,
                rx_channel: channel.rx_channel,
                peer_ifaces: Arc::new(tokio::sync::Mutex::new(BTreeMap::new())),
                outbound_routes: Arc::new(tokio::sync::Mutex::new(BTreeMap::new())),
            },
            tx_channel: channel.tx_channel,
        }
    }

    fn split(self) -> (AutoInterfaceTransportBridge, InterfaceTxReceiver) {
        (self.bridge, self.tx_channel)
    }
}

impl AutoInterfaceTransportBridge {
    async fn ensure_peer_iface(
        &self,
        peer: SocketAddr,
        route: AutoPeerOutboundRoute,
    ) -> Option<AddressHash> {
        if let Some(existing) = self.peer_ifaces.lock().await.get(&peer).copied() {
            self.outbound_routes.lock().await.insert(existing, route);
            return Some(existing);
        }

        let virtual_iface = {
            let mut manager = self.iface_manager.lock().await;
            manager.register_virtual_iface(self.host_iface, IfaceRole::VirtualUnicast)?
        };
        self.peer_ifaces.lock().await.insert(peer, virtual_iface);
        self.outbound_routes.lock().await.insert(virtual_iface, route);
        Some(virtual_iface)
    }

    async fn forward_peer_data(
        &self,
        processed: &AutoProcessedPeerDataDatagram,
        socket: Arc<tokio::net::UdpSocket>,
    ) {
        if !matches!(processed.decision, AutoPeerInboundDecision::Accepted { .. }) {
            return;
        }
        let Some(virtual_iface) = self
            .ensure_peer_iface(
                processed.datagram.source_addr,
                AutoPeerOutboundRoute { socket, destination: processed.datagram.source_addr },
            )
            .await
        else {
            log::warn!(
                "[daemon-auto] failed to register virtual peer iface for {}",
                processed.datagram.source_addr
            );
            return;
        };
        let packet = match Packet::deserialize(&mut InputBuffer::new(&processed.datagram.payload)) {
            Ok(packet) => packet,
            Err(err) => {
                log::warn!(
                    "[daemon-auto] failed to decode peer data packet from {}: {:?}",
                    processed.datagram.source_addr,
                    err
                );
                return;
            }
        };
        let _ = self
            .rx_channel
            .send(RxMessage {
                address: virtual_iface,
                packet,
                source: IfaceSource::Udp(processed.datagram.source_addr),
            })
            .await;
    }

    async fn send_outbound(&self, message: TxMessage) {
        match message.tx_type {
            TxMessageType::Direct(iface) => {
                self.send_to_route(iface, message.packet).await;
            }
            TxMessageType::Broadcast(_) => {
                let routes = self.outbound_routes.lock().await.clone();
                for (iface, _) in routes {
                    self.send_to_route(iface, message.packet).await;
                }
            }
        }
    }

    async fn send_to_route(&self, iface: AddressHash, packet: Packet) {
        let Some(route) = self.outbound_routes.lock().await.get(&iface).cloned() else {
            return;
        };
        let payload = match packet.to_bytes() {
            Ok(payload) => payload,
            Err(err) => {
                log::warn!("[daemon-auto] failed to serialize outbound peer data packet: {err:?}");
                return;
            }
        };
        if let Err(err) = route.socket.send_to(&payload, route.destination).await {
            log::warn!(
                "[daemon-auto] failed to send outbound peer data packet to {}: {err}",
                route.destination
            );
        }
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

impl AutoDiscoverySocketBindTarget {
    fn unicast(listener: &AutoDiscoveryListenerBinding) -> Self {
        let (bind_host, scope_ifname) =
            bind_host_and_scope(&listener.unicast_bind_address, &listener.ifname);
        Self {
            kind: AutoDiscoverySocketKind::Unicast,
            ifname: listener.ifname.clone(),
            bind_host,
            bind_port: listener.unicast_bind_port,
            scope_ifname,
            multicast_group_host: None,
        }
    }

    fn multicast(listener: &AutoDiscoveryListenerBinding) -> Self {
        let (bind_host, scope_ifname) =
            bind_host_and_scope(&listener.multicast_bind_address, &listener.ifname);
        Self {
            kind: AutoDiscoverySocketKind::Multicast,
            ifname: listener.ifname.clone(),
            bind_host,
            bind_port: listener.multicast_bind_port,
            scope_ifname,
            multicast_group_host: Some(listener.multicast_group_address.clone()),
        }
    }

    pub(crate) fn display_bind_addr(&self) -> String {
        let host = if let Some(scope_ifname) = &self.scope_ifname {
            format!("{}%{scope_ifname}", self.bind_host)
        } else {
            self.bind_host.clone()
        };
        socket_target(&host, self.bind_port)
    }

    fn resolve_bind_addr(
        &self,
        mut scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<SocketAddr, String> {
        let ip = self
            .bind_host
            .parse::<IpAddr>()
            .map_err(|err| format!("parse auto discovery bind host {}: {err}", self.bind_host))?;
        match (ip, self.scope_ifname.as_deref()) {
            (IpAddr::V6(host), Some(ifname)) => {
                let scope_id = scope_id_for_ifname(ifname).map_err(|err| {
                    format!("resolve auto discovery scope id for interface {ifname}: {err}")
                })?;
                Ok(SocketAddr::V6(SocketAddrV6::new(host, self.bind_port, 0, scope_id)))
            }
            (IpAddr::V6(host), None) => {
                Ok(SocketAddr::V6(SocketAddrV6::new(host, self.bind_port, 0, 0)))
            }
            (IpAddr::V4(host), None) => Ok(SocketAddr::from((host, self.bind_port))),
            (IpAddr::V4(_), Some(ifname)) => Err(format!(
                "auto discovery IPv4 bind host {} cannot use scope interface {ifname}",
                self.bind_host
            )),
        }
    }

    fn resolve_multicast_bind(
        &self,
        mut scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<AutoResolvedMulticastDiscoveryBind, String> {
        if self.kind != AutoDiscoverySocketKind::Multicast {
            return Err("auto discovery multicast bind resolver requires a multicast target".into());
        }
        let group_host = self.multicast_group_host.as_ref().ok_or_else(|| {
            "auto discovery multicast target is missing multicast group".to_string()
        })?;
        let group_ip = group_host.parse::<IpAddr>().map_err(|err| {
            format!("parse auto discovery multicast group host {group_host}: {err}")
        })?;
        if !group_ip.is_multicast() {
            return Err(format!("auto discovery group host {group_host} is not multicast"));
        }
        let join_scope_ifname = match group_ip {
            IpAddr::V6(_) if is_link_scope_ipv6_multicast(group_host) => {
                Some(self.scope_ifname.as_deref().unwrap_or(self.ifname.as_str()))
            }
            IpAddr::V6(_) => self.scope_ifname.as_deref(),
            IpAddr::V4(_) => None,
        };
        let multicast_scope_id = if let Some(ifname) = join_scope_ifname {
            scope_id_for_ifname(ifname).map_err(|err| {
                format!("resolve auto discovery multicast scope id for interface {ifname}: {err}")
            })?
        } else {
            0
        };
        let multicast_group_addr = match group_ip {
            IpAddr::V6(group) => {
                SocketAddr::V6(SocketAddrV6::new(group, self.bind_port, 0, multicast_scope_id))
            }
            IpAddr::V4(group) => SocketAddr::from((group, self.bind_port)),
        };
        let bind_addr = self.resolve_multicast_socket_bind_addr(&mut scope_id_for_ifname)?;
        Ok(AutoResolvedMulticastDiscoveryBind {
            bind_addr,
            multicast_group_addr,
            multicast_scope_id,
        })
    }

    fn resolve_multicast_socket_bind_addr(
        &self,
        scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<SocketAddr, String> {
        let bind_ip = self.bind_host.parse::<IpAddr>().map_err(|err| {
            format!("parse auto discovery multicast bind host {}: {err}", self.bind_host)
        })?;
        if bind_ip.is_multicast() {
            return Ok(match bind_ip {
                IpAddr::V6(_) => SocketAddr::V6(SocketAddrV6::new(
                    std::net::Ipv6Addr::UNSPECIFIED,
                    self.bind_port,
                    0,
                    0,
                )),
                IpAddr::V4(_) => {
                    SocketAddr::from((std::net::Ipv4Addr::UNSPECIFIED, self.bind_port))
                }
            });
        }
        self.resolve_bind_addr(scope_id_for_ifname)
    }
}

impl AutoDataSocketBindTarget {
    fn from_listener(listener: &AutoDataListenerBinding) -> Self {
        let (bind_host, scope_ifname) =
            bind_host_and_scope(&listener.bind_address, &listener.ifname);
        Self {
            ifname: listener.ifname.clone(),
            bind_host,
            bind_port: listener.bind_port,
            scope_ifname,
        }
    }

    pub(crate) fn display_bind_addr(&self) -> String {
        let host = if let Some(scope_ifname) = &self.scope_ifname {
            format!("{}%{scope_ifname}", self.bind_host)
        } else {
            self.bind_host.clone()
        };
        socket_target(&host, self.bind_port)
    }

    fn resolve_bind_addr(
        &self,
        mut scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<SocketAddr, String> {
        let ip = self
            .bind_host
            .parse::<IpAddr>()
            .map_err(|err| format!("parse auto peer data bind host {}: {err}", self.bind_host))?;
        match (ip, self.scope_ifname.as_deref()) {
            (IpAddr::V6(host), Some(ifname)) => {
                let scope_id = scope_id_for_ifname(ifname).map_err(|err| {
                    format!("resolve auto peer data scope id for interface {ifname}: {err}")
                })?;
                Ok(SocketAddr::V6(SocketAddrV6::new(host, self.bind_port, 0, scope_id)))
            }
            (IpAddr::V6(host), None) => {
                Ok(SocketAddr::V6(SocketAddrV6::new(host, self.bind_port, 0, 0)))
            }
            (IpAddr::V4(host), None) => Ok(SocketAddr::from((host, self.bind_port))),
            (IpAddr::V4(_), Some(ifname)) => Err(format!(
                "auto peer data IPv4 bind host {} cannot use scope interface {ifname}",
                self.bind_host
            )),
        }
    }
}

impl AutoDaemonStartupPlan {
    pub(crate) fn runtime_json(&self) -> JsonValue {
        let mut initial_peer_announces = Vec::new();
        let _ = self.send_initial_peer_announces(|datagram| {
            initial_peer_announces.push(peering_datagram_json(datagram));
            Ok(())
        });
        json!({
            "auto_runtime_status": "complete",
            "platform": platform_name(self.platform),
            "group_id": self.config.group_id.clone(),
            "candidate_devices": self.candidates.iter().map(candidate_json).collect::<Vec<_>>(),
            "adopted_devices": self.adopted_devices.iter().map(adopted_json).collect::<Vec<_>>(),
            "startup_plan": startup_plan_json(&self.startup_plan),
            "planned_initial_peer_announce_count": initial_peer_announces.len(),
            "planned_repeat_peer_announce_scheduler_count": usize::from(!self.adopted_devices.is_empty()),
            "planned_peer_job_scheduler_count": usize::from(!self.adopted_devices.is_empty()),
            "initial_peer_announces": initial_peer_announces,
            "native_scope_id_source": "if-addrs interface index",
            "planned_discovery_receive_loop_count": self.discovery_socket_bind_targets().len(),
            "planned_discovery_socket_binds": self.discovery_socket_bind_targets().iter().map(discovery_socket_bind_json).collect::<Vec<_>>(),
            "planned_data_receive_loop_count": self.data_socket_bind_targets().len(),
            "planned_data_socket_binds": self.data_socket_bind_targets().iter().map(data_socket_bind_json).collect::<Vec<_>>(),
        })
    }

    pub(crate) fn initial_peer_announce_datagrams(&self) -> Vec<AutoPeerAnnounceDatagram> {
        self.peering_packets.iter().map(AutoPeerAnnounceDatagram::from).collect()
    }

    #[allow(dead_code)]
    pub(crate) fn due_multicast_peer_announce_datagrams(
        &self,
        state: &mut AutoDiscoveryState,
        now: core::time::Duration,
    ) -> Vec<AutoPeerAnnounceDatagram> {
        let timing = AutoInterfaceTiming::for_platform(self.platform);
        state
            .run_multicast_announce_job(
                &self.config,
                &self.adopted_devices,
                now,
                timing.announce_interval,
            )
            .iter()
            .map(AutoPeerAnnounceDatagram::from)
            .collect()
    }

    pub(crate) fn discovery_socket_bind_targets(&self) -> Vec<AutoDiscoverySocketBindTarget> {
        self.startup_plan
            .discovery_listeners
            .iter()
            .flat_map(|listener| {
                [
                    AutoDiscoverySocketBindTarget::unicast(listener),
                    AutoDiscoverySocketBindTarget::multicast(listener),
                ]
            })
            .collect()
    }

    pub(crate) fn data_socket_bind_targets(&self) -> Vec<AutoDataSocketBindTarget> {
        self.startup_plan
            .data_listeners
            .iter()
            .map(AutoDataSocketBindTarget::from_listener)
            .collect()
    }

    #[allow(dead_code)]
    pub(crate) fn discovery_state(&self) -> AutoDiscoveryState {
        AutoDiscoveryState::from_timing(
            self.adopted_devices.clone(),
            AutoInterfaceTiming::for_platform(self.platform),
        )
    }

    #[allow(dead_code)]
    pub(crate) fn process_discovery_datagram(
        &self,
        state: &mut AutoDiscoveryState,
        datagram: AutoDiscoveryDatagram,
        now: core::time::Duration,
    ) -> Result<AutoProcessedDiscoveryDatagram, AutoDiscoveryRejectReason> {
        let source_address = discovery_source_address(&datagram);
        let event = state.observe_authenticated_discovery_packet(
            &datagram.payload,
            self.config.group_id.as_bytes(),
            &source_address,
            &datagram.ifname,
            now,
        )?;
        Ok(AutoProcessedDiscoveryDatagram { datagram, source_address, event })
    }

    #[allow(dead_code)]
    pub(crate) fn process_peer_data_datagram(
        &self,
        state: &mut AutoDiscoveryState,
        dedupe: &mut AutoInboundPacketDeduplicator,
        datagram: AutoPeerDataDatagram,
        now: core::time::Duration,
    ) -> AutoProcessedPeerDataDatagram {
        let peer_address = peer_data_source_address(&datagram);
        let decision =
            state.handle_spawned_peer_inbound(dedupe, &peer_address, &datagram.payload, now);
        AutoProcessedPeerDataDatagram { datagram, peer_address, decision }
    }

    pub(crate) fn send_initial_peer_announces(
        &self,
        mut send: impl FnMut(&AutoPeerAnnounceDatagram) -> Result<(), String>,
    ) -> Result<usize, String> {
        let datagrams = self.initial_peer_announce_datagrams();
        Self::send_peer_announce_datagrams(&datagrams, "auto peer announce", &mut send)
    }

    #[allow(dead_code)]
    pub(crate) fn run_multicast_peer_announce_job(
        &self,
        state: &mut AutoDiscoveryState,
        now: core::time::Duration,
        mut send: impl FnMut(&AutoPeerAnnounceDatagram) -> Result<(), String>,
    ) -> Result<usize, String> {
        let datagrams = self.due_multicast_peer_announce_datagrams(state, now);
        Self::send_peer_announce_datagrams(&datagrams, "auto multicast peer announce", &mut send)
    }

    #[allow(dead_code)]
    pub(crate) fn run_peer_job(
        &self,
        state: &mut AutoDiscoveryState,
        now: core::time::Duration,
        mut send: impl FnMut(&AutoPeerAnnounceDatagram) -> Result<(), String>,
    ) -> Result<AutoPeerJobRuntimeSummary, String> {
        let (summary, datagrams) = self.run_peer_job_datagrams(state, now);
        Self::send_peer_announce_datagrams(&datagrams, "auto reverse peer announce", &mut send)?;
        Ok(summary)
    }

    fn run_peer_job_datagrams(
        &self,
        state: &mut AutoDiscoveryState,
        now: core::time::Duration,
    ) -> (AutoPeerJobRuntimeSummary, Vec<AutoPeerAnnounceDatagram>) {
        let timing = AutoInterfaceTiming::for_platform(self.platform);
        let run = state.run_peer_job(
            &self.config,
            &self.adopted_devices,
            now,
            timing.multicast_echo_timeout,
        );
        let datagrams = run
            .reverse_peering_packets
            .iter()
            .map(AutoPeerAnnounceDatagram::from)
            .collect::<Vec<_>>();
        (
            AutoPeerJobRuntimeSummary {
                expired_peer_count: run.expired_peers.len(),
                reverse_peer_announce_count: datagrams.len(),
                missing_initial_echo_count: run.missing_initial_echo_interfaces.len(),
                carrier_event_count: run.carrier_events.len(),
            },
            datagrams,
        )
    }

    fn send_peer_announce_datagrams(
        datagrams: &[AutoPeerAnnounceDatagram],
        label: &str,
        mut send: impl FnMut(&AutoPeerAnnounceDatagram) -> Result<(), String>,
    ) -> Result<usize, String> {
        let mut sent = 0;
        for datagram in datagrams {
            send(datagram).map_err(|err| {
                format!(
                    "send {label} {}/{} to {} failed: {err}",
                    sent + 1,
                    datagrams.len(),
                    datagram.destination_socket_target()
                )
            })?;
            sent += 1;
        }
        Ok(sent)
    }

    // Shared by startup and tests to send a fixed set of peer-announce
    // datagrams through a caller-owned UDP socket.
    #[allow(dead_code)]
    pub(crate) async fn send_initial_peer_announces_with_udp_socket(
        &self,
        socket: &tokio::net::UdpSocket,
        mut scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<usize, String> {
        let datagrams = self.initial_peer_announce_datagrams();
        self.send_peer_announce_datagrams_with_udp_socket(
            &datagrams,
            "auto peer announce",
            socket,
            &mut scope_id_for_ifname,
        )
        .await
    }

    #[allow(dead_code)]
    async fn send_peer_announce_datagrams_with_udp_socket(
        &self,
        datagrams: &[AutoPeerAnnounceDatagram],
        label: &str,
        socket: &tokio::net::UdpSocket,
        mut scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<usize, String> {
        let mut sent = 0;
        for datagram in datagrams {
            let target = datagram.socket_target();
            let destination =
                target.resolve_socket_addr(&mut scope_id_for_ifname).map_err(|err| {
                    format!(
                        "resolve {label} {}/{} target {} failed: {err}",
                        sent + 1,
                        datagrams.len(),
                        target.display()
                    )
                })?;
            let sent_bytes =
                socket.send_to(&datagram.payload, destination).await.map_err(|err| {
                    format!(
                        "send {label} {}/{} to {} failed: {err}",
                        sent + 1,
                        datagrams.len(),
                        target.display()
                    )
                })?;
            if sent_bytes != datagram.payload.len() {
                return Err(format!(
                    "send {label} {}/{} to {} sent {sent_bytes}/{} byte(s)",
                    sent + 1,
                    datagrams.len(),
                    target.display(),
                    datagram.payload.len()
                ));
            }
            sent += 1;
        }
        Ok(sent)
    }

    #[allow(dead_code)]
    pub(crate) async fn send_initial_peer_announces_with_native_scope_ids(
        &self,
        socket: &tokio::net::UdpSocket,
    ) -> Result<usize, String> {
        let resolver = AutoInterfaceIndexResolver::from_system()?;
        self.send_initial_peer_announces_with_udp_socket(socket, |ifname| resolver.resolve(ifname))
            .await
    }

    #[allow(dead_code)]
    pub(crate) async fn bind_discovery_sockets_with_native_scope_ids(
        &self,
    ) -> Result<Vec<AutoBoundDiscoverySocket>, String> {
        let resolver = AutoInterfaceIndexResolver::from_system()?;
        let mut sockets =
            self.bind_unicast_discovery_sockets(|ifname| resolver.resolve(ifname)).await?;
        sockets.extend(
            self.bind_multicast_discovery_sockets(|ifname| resolver.resolve(ifname)).await?,
        );
        Ok(sockets)
    }

    #[allow(dead_code)]
    pub(crate) async fn bind_data_sockets_with_native_scope_ids(
        &self,
    ) -> Result<Vec<AutoBoundDataSocket>, String> {
        let resolver = AutoInterfaceIndexResolver::from_system()?;
        self.bind_data_sockets(|ifname| resolver.resolve(ifname)).await
    }

    #[allow(dead_code)]
    pub(crate) async fn spawn_discovery_runtime_with_native_scope_ids(
        &self,
    ) -> Result<AutoDiscoveryRuntimeSummary, String> {
        self.spawn_discovery_runtime_with_native_scope_ids_and_transport(None).await
    }

    #[allow(dead_code)]
    pub(crate) async fn spawn_discovery_runtime_with_native_scope_ids_and_transport(
        &self,
        transport_runtime: Option<AutoInterfaceTransportRuntime>,
    ) -> Result<AutoDiscoveryRuntimeSummary, String> {
        let (transport_bridge, transport_tx_channel) = match transport_runtime {
            Some(runtime) => {
                let (bridge, tx_channel) = runtime.split();
                (Some(bridge), Some(tx_channel))
            }
            None => (None, None),
        };
        let sockets = self.bind_discovery_sockets_with_native_scope_ids().await?;
        let bound_socket_count = sockets.len();
        let data_sockets = self.bind_data_sockets_with_native_scope_ids().await?;
        let data_socket_count = data_sockets.len();
        let state = Arc::new(tokio::sync::Mutex::new(self.discovery_state()));
        let dedupe = Arc::new(tokio::sync::Mutex::new(AutoInboundPacketDeduplicator::from_timing(
            AutoInterfaceTiming::for_platform(self.platform),
        )));
        let announce_socket = if self.adopted_devices.is_empty() {
            None
        } else {
            Some(self.bind_peer_announce_runtime_socket().await?)
        };
        let initial_peer_announce_count = if let Some(socket) = &announce_socket {
            self.send_due_multicast_peer_announces_with_runtime_socket(
                Arc::clone(&state),
                Arc::clone(socket),
                core::time::Duration::ZERO,
            )
            .await?
        } else {
            0
        };
        if sockets.is_empty() {
            return Ok(AutoDiscoveryRuntimeSummary {
                bound_socket_count,
                receive_loop_count: 0,
                initial_peer_announce_count,
                repeat_peer_announce_scheduler_count: 0,
                peer_job_scheduler_count: 0,
                data_socket_count,
                data_receive_loop_count: 0,
            });
        }
        let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(bound_socket_count * 8);
        let data_events_capacity = usize::max(data_socket_count * 8, 1);
        let (data_events_tx, mut data_events_rx) = tokio::sync::mpsc::channel(data_events_capacity);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let handles = self.spawn_discovery_receive_loops(
            sockets,
            Arc::clone(&state),
            events_tx,
            shutdown_rx.clone(),
        );
        let receive_loop_count = handles.len();
        let data_handles = self.spawn_peer_data_receive_loops(
            data_sockets,
            Arc::clone(&state),
            dedupe,
            transport_bridge.clone(),
            data_events_tx,
            shutdown_rx.clone(),
        );
        let data_receive_loop_count = data_handles.len();
        let transport_tx_handle = transport_tx_channel.map(|tx_channel| {
            self.spawn_peer_data_transport_tx_loop(
                transport_bridge.expect("transport bridge exists with tx channel"),
                tx_channel,
                shutdown_rx.clone(),
            )
        });
        let scheduler_handle = announce_socket.as_ref().map(|socket| {
            self.spawn_repeat_peer_announce_scheduler(
                Arc::clone(&state),
                Arc::clone(socket),
                shutdown_rx.clone(),
            )
        });
        let repeat_peer_announce_scheduler_count = usize::from(scheduler_handle.is_some());
        let peer_job_scheduler_handle = announce_socket.as_ref().map(|socket| {
            self.spawn_peer_job_scheduler(
                Arc::clone(&state),
                Arc::clone(socket),
                shutdown_rx.clone(),
            )
        });
        let peer_job_scheduler_count = usize::from(peer_job_scheduler_handle.is_some());
        tokio::spawn(async move {
            let _shutdown_guard = shutdown_tx;
            let mut discovery_events_open = true;
            let mut data_events_open = true;
            while discovery_events_open || data_events_open {
                tokio::select! {
                    event = events_rx.recv(), if discovery_events_open => {
                        match event {
                            Some(event) => log_auto_discovery_loop_event(event),
                            None => discovery_events_open = false,
                        }
                    }
                    event = data_events_rx.recv(), if data_events_open => {
                        match event {
                            Some(event) => log_auto_peer_data_loop_event(event),
                            None => data_events_open = false,
                        }
                    }
                }
            }
            for handle in handles {
                if let Err(err) = handle.await {
                    log::warn!("[daemon-auto] discovery receive loop task stopped: {err}");
                }
            }
            for handle in data_handles {
                if let Err(err) = handle.await {
                    log::warn!("[daemon-auto] peer data receive loop task stopped: {err}");
                }
            }
            if let Some(handle) = scheduler_handle {
                if let Err(err) = handle.await {
                    log::warn!("[daemon-auto] repeat peer-announce scheduler stopped: {err}");
                }
            }
            if let Some(handle) = peer_job_scheduler_handle {
                if let Err(err) = handle.await {
                    log::warn!("[daemon-auto] peer-job scheduler stopped: {err}");
                }
            }
            if let Some(handle) = transport_tx_handle {
                if let Err(err) = handle.await {
                    log::warn!("[daemon-auto] peer data transport tx loop stopped: {err}");
                }
            }
        });
        Ok(AutoDiscoveryRuntimeSummary {
            bound_socket_count,
            receive_loop_count,
            initial_peer_announce_count,
            repeat_peer_announce_scheduler_count,
            peer_job_scheduler_count,
            data_socket_count,
            data_receive_loop_count,
        })
    }

    async fn bind_peer_announce_runtime_socket(
        &self,
    ) -> Result<Arc<tokio::net::UdpSocket>, String> {
        let socket = tokio::net::UdpSocket::bind("[::]:0").await.map_err(|err| {
            format!("bind auto peer-announce scheduler socket [::]:0 failed: {err}")
        })?;
        Ok(Arc::new(socket))
    }

    async fn send_due_multicast_peer_announces_with_runtime_socket(
        &self,
        state: Arc<tokio::sync::Mutex<AutoDiscoveryState>>,
        socket: Arc<tokio::net::UdpSocket>,
        now: core::time::Duration,
    ) -> Result<usize, String> {
        let datagrams = {
            let mut state = state.lock().await;
            self.due_multicast_peer_announce_datagrams(&mut state, now)
        };
        if datagrams.is_empty() {
            return Ok(0);
        }
        let resolver = AutoInterfaceIndexResolver::from_system()?;
        self.send_peer_announce_datagrams_with_udp_socket(
            &datagrams,
            "auto multicast peer announce",
            &socket,
            |ifname| resolver.resolve(ifname),
        )
        .await
    }

    fn spawn_repeat_peer_announce_scheduler(
        &self,
        state: Arc<tokio::sync::Mutex<AutoDiscoveryState>>,
        socket: Arc<tokio::net::UdpSocket>,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> tokio::task::JoinHandle<()> {
        let plan = self.clone();
        let timing = AutoInterfaceTiming::for_platform(self.platform);
        tokio::spawn(async move {
            if *shutdown.borrow() {
                return;
            }
            let started_at = Instant::now();
            let mut interval = tokio::time::interval(timing.announce_interval);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            break;
                        }
                    }
                    _ = interval.tick() => {
                        match plan
                            .send_due_multicast_peer_announces_with_runtime_socket(
                                Arc::clone(&state),
                                Arc::clone(&socket),
                                started_at.elapsed(),
                            )
                            .await
                        {
                            Ok(sent) if sent > 0 => {
                                log::debug!("[daemon-auto] repeat peer-announce scheduler sent {sent} packet(s)");
                            }
                            Ok(_) => {}
                            Err(err) => {
                                log::warn!("[daemon-auto] repeat peer-announce scheduler failed: {err}");
                            }
                        }
                    }
                }
            }
        })
    }

    async fn send_due_peer_job_with_runtime_socket(
        &self,
        state: Arc<tokio::sync::Mutex<AutoDiscoveryState>>,
        socket: Arc<tokio::net::UdpSocket>,
        now: core::time::Duration,
    ) -> Result<AutoPeerJobRuntimeSummary, String> {
        let (summary, datagrams) = {
            let mut state = state.lock().await;
            self.run_peer_job_datagrams(&mut state, now)
        };
        if datagrams.is_empty() {
            return Ok(summary);
        }
        let resolver = AutoInterfaceIndexResolver::from_system()?;
        self.send_peer_announce_datagrams_with_udp_socket(
            &datagrams,
            "auto reverse peer announce",
            &socket,
            |ifname| resolver.resolve(ifname),
        )
        .await?;
        Ok(summary)
    }

    fn spawn_peer_job_scheduler(
        &self,
        state: Arc<tokio::sync::Mutex<AutoDiscoveryState>>,
        socket: Arc<tokio::net::UdpSocket>,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> tokio::task::JoinHandle<()> {
        let plan = self.clone();
        let timing = AutoInterfaceTiming::for_platform(self.platform);
        tokio::spawn(async move {
            if *shutdown.borrow() {
                return;
            }
            let started_at = Instant::now();
            let mut interval = tokio::time::interval(timing.peer_job_interval);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            break;
                        }
                    }
                    _ = interval.tick() => {
                        match plan
                            .send_due_peer_job_with_runtime_socket(
                                Arc::clone(&state),
                                Arc::clone(&socket),
                                started_at.elapsed(),
                            )
                            .await
                        {
                            Ok(summary)
                                if summary.expired_peer_count > 0
                                    || summary.reverse_peer_announce_count > 0
                                    || summary.carrier_event_count > 0 =>
                            {
                                log::debug!(
                                    "[daemon-auto] peer-job scheduler expired={} reverse_announces={} missing_initial_echoes={} carrier_events={}",
                                    summary.expired_peer_count,
                                    summary.reverse_peer_announce_count,
                                    summary.missing_initial_echo_count,
                                    summary.carrier_event_count
                                );
                            }
                            Ok(_) => {}
                            Err(err) => {
                                log::warn!("[daemon-auto] peer-job scheduler failed: {err}");
                            }
                        }
                    }
                }
            }
        })
    }

    // Binds only the unicast side of discovery; startup combines these sockets
    // with multicast sockets before spawning receive loops.
    #[allow(dead_code)]
    pub(crate) async fn bind_unicast_discovery_sockets(
        &self,
        mut scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<Vec<AutoBoundDiscoverySocket>, String> {
        let mut sockets = Vec::new();
        for target in self
            .discovery_socket_bind_targets()
            .into_iter()
            .filter(|target| target.kind == AutoDiscoverySocketKind::Unicast)
        {
            let bind_addr = target.resolve_bind_addr(&mut scope_id_for_ifname).map_err(|err| {
                format!(
                    "resolve auto discovery unicast bind {} failed: {err}",
                    target.display_bind_addr()
                )
            })?;
            let socket = tokio::net::UdpSocket::bind(bind_addr).await.map_err(|err| {
                format!(
                    "bind auto discovery unicast socket {} failed: {err}",
                    target.display_bind_addr()
                )
            })?;
            sockets.push(AutoBoundDiscoverySocket {
                kind: target.kind,
                ifname: target.ifname,
                bind_addr: socket.local_addr().unwrap_or(bind_addr),
                multicast_group_addr: None,
                socket,
            });
        }
        Ok(sockets)
    }

    #[allow(dead_code)]
    pub(crate) async fn bind_data_sockets(
        &self,
        mut scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<Vec<AutoBoundDataSocket>, String> {
        let mut sockets = Vec::new();
        for target in self.data_socket_bind_targets() {
            let bind_addr = target.resolve_bind_addr(&mut scope_id_for_ifname).map_err(|err| {
                format!("resolve auto peer data bind {} failed: {err}", target.display_bind_addr())
            })?;
            let socket = tokio::net::UdpSocket::bind(bind_addr).await.map_err(|err| {
                format!("bind auto peer data socket {} failed: {err}", target.display_bind_addr())
            })?;
            sockets.push(AutoBoundDataSocket {
                ifname: target.ifname,
                bind_addr: socket.local_addr().unwrap_or(bind_addr),
                socket: Arc::new(socket),
            });
        }
        Ok(sockets)
    }

    // Binds and joins only the multicast side of discovery; startup combines
    // these sockets with unicast sockets before spawning receive loops.
    #[allow(dead_code)]
    pub(crate) async fn bind_multicast_discovery_sockets(
        &self,
        mut scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<Vec<AutoBoundDiscoverySocket>, String> {
        let mut sockets = Vec::new();
        for target in self
            .discovery_socket_bind_targets()
            .into_iter()
            .filter(|target| target.kind == AutoDiscoverySocketKind::Multicast)
        {
            let resolved =
                target.resolve_multicast_bind(&mut scope_id_for_ifname).map_err(|err| {
                    format!(
                        "resolve auto discovery multicast bind {} failed: {err}",
                        target.display_bind_addr()
                    )
                })?;
            let std_socket = std::net::UdpSocket::bind(resolved.bind_addr).map_err(|err| {
                format!(
                    "bind auto discovery multicast socket {} failed: {err}",
                    target.display_bind_addr()
                )
            })?;
            match resolved.multicast_group_addr.ip() {
                IpAddr::V6(group) => std_socket
                    .join_multicast_v6(&group, resolved.multicast_scope_id)
                    .map_err(|err| {
                        format!(
                            "join auto discovery multicast group {} on ifindex {} failed: {err}",
                            resolved.multicast_group_addr, resolved.multicast_scope_id
                        )
                    })?,
                IpAddr::V4(group) => std_socket
                    .join_multicast_v4(&group, &std::net::Ipv4Addr::UNSPECIFIED)
                    .map_err(|err| {
                        format!(
                            "join auto discovery multicast group {} failed: {err}",
                            resolved.multicast_group_addr
                        )
                    })?,
            }
            std_socket.set_nonblocking(true).map_err(|err| {
                format!("set auto discovery multicast socket nonblocking failed: {err}")
            })?;
            let socket = tokio::net::UdpSocket::from_std(std_socket).map_err(|err| {
                format!("convert auto discovery multicast socket to tokio failed: {err}")
            })?;
            sockets.push(AutoBoundDiscoverySocket {
                kind: target.kind,
                ifname: target.ifname,
                bind_addr: socket.local_addr().unwrap_or(resolved.bind_addr),
                multicast_group_addr: Some(resolved.multicast_group_addr),
                socket,
            });
        }
        Ok(sockets)
    }

    #[allow(dead_code)]
    pub(crate) fn spawn_discovery_receive_loops(
        &self,
        sockets: Vec<AutoBoundDiscoverySocket>,
        state: Arc<tokio::sync::Mutex<AutoDiscoveryState>>,
        events: tokio::sync::mpsc::Sender<AutoDiscoveryLoopEvent>,
        shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> Vec<tokio::task::JoinHandle<()>> {
        sockets
            .into_iter()
            .map(|socket| {
                self.spawn_discovery_receive_loop(
                    socket,
                    Arc::clone(&state),
                    events.clone(),
                    shutdown.clone(),
                )
            })
            .collect()
    }

    #[allow(dead_code)]
    fn spawn_discovery_receive_loop(
        &self,
        socket: AutoBoundDiscoverySocket,
        state: Arc<tokio::sync::Mutex<AutoDiscoveryState>>,
        events: tokio::sync::mpsc::Sender<AutoDiscoveryLoopEvent>,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> tokio::task::JoinHandle<()> {
        let group_id = self.config.group_id.clone();
        tokio::spawn(async move {
            if *shutdown.borrow() {
                return;
            }
            let started_at = Instant::now();
            loop {
                tokio::select! {
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            break;
                        }
                    }
                    received = socket.recv_discovery_datagram() => {
                        let datagram = match received {
                            Ok(datagram) => datagram,
                            Err(error) => {
                                let _ = events
                                    .send(AutoDiscoveryLoopEvent::ReceiveFailed {
                                        ifname: socket.ifname.clone(),
                                        kind: socket.kind,
                                        bind_addr: socket.bind_addr,
                                        error,
                                    })
                                    .await;
                                break;
                            }
                        };
                        let source_address = discovery_source_address(&datagram);
                        let event = {
                            let mut state = state.lock().await;
                            state.observe_authenticated_discovery_packet(
                                &datagram.payload,
                                group_id.as_bytes(),
                                &source_address,
                                &datagram.ifname,
                                started_at.elapsed(),
                            )
                        };
                        let loop_event = match event {
                            Ok(event) => AutoDiscoveryLoopEvent::Processed(
                                AutoProcessedDiscoveryDatagram {
                                    datagram,
                                    source_address,
                                    event,
                                },
                            ),
                            Err(reason) => AutoDiscoveryLoopEvent::Rejected {
                                datagram,
                                source_address,
                                reason,
                            },
                        };
                        if events.send(loop_event).await.is_err() {
                            break;
                        }
                    }
                }
            }
        })
    }

    #[allow(dead_code)]
    pub(crate) fn spawn_peer_data_receive_loops(
        &self,
        sockets: Vec<AutoBoundDataSocket>,
        state: Arc<tokio::sync::Mutex<AutoDiscoveryState>>,
        dedupe: Arc<tokio::sync::Mutex<AutoInboundPacketDeduplicator>>,
        transport: Option<AutoInterfaceTransportBridge>,
        events: tokio::sync::mpsc::Sender<AutoPeerDataLoopEvent>,
        shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> Vec<tokio::task::JoinHandle<()>> {
        sockets
            .into_iter()
            .map(|socket| {
                self.spawn_peer_data_receive_loop(
                    socket,
                    Arc::clone(&state),
                    Arc::clone(&dedupe),
                    transport.clone(),
                    events.clone(),
                    shutdown.clone(),
                )
            })
            .collect()
    }

    #[allow(dead_code)]
    fn spawn_peer_data_receive_loop(
        &self,
        socket: AutoBoundDataSocket,
        state: Arc<tokio::sync::Mutex<AutoDiscoveryState>>,
        dedupe: Arc<tokio::sync::Mutex<AutoInboundPacketDeduplicator>>,
        transport: Option<AutoInterfaceTransportBridge>,
        events: tokio::sync::mpsc::Sender<AutoPeerDataLoopEvent>,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> tokio::task::JoinHandle<()> {
        let plan = self.clone();
        tokio::spawn(async move {
            if *shutdown.borrow() {
                return;
            }
            let started_at = Instant::now();
            loop {
                tokio::select! {
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            break;
                        }
                    }
                    received = socket.recv_peer_data_datagram() => {
                        let datagram = match received {
                            Ok(datagram) => datagram,
                            Err(error) => {
                                let _ = events
                                    .send(AutoPeerDataLoopEvent::ReceiveFailed {
                                        ifname: socket.ifname.clone(),
                                        bind_addr: socket.bind_addr,
                                        error,
                                    })
                                    .await;
                                break;
                            }
                        };
                        let processed = {
                            let mut state = state.lock().await;
                            let mut dedupe = dedupe.lock().await;
                            plan.process_peer_data_datagram(
                                &mut state,
                                &mut dedupe,
                                datagram,
                                started_at.elapsed(),
                            )
                        };
                        if let Some(transport) = &transport {
                            transport
                                .forward_peer_data(&processed, Arc::clone(&socket.socket))
                                .await;
                        }
                        if events.send(AutoPeerDataLoopEvent::Processed(processed)).await.is_err() {
                            break;
                        }
                    }
                }
            }
        })
    }

    fn spawn_peer_data_transport_tx_loop(
        &self,
        transport: AutoInterfaceTransportBridge,
        mut tx_channel: InterfaceTxReceiver,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            if *shutdown.borrow() {
                return;
            }
            loop {
                tokio::select! {
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            break;
                        }
                    }
                    message = tx_channel.recv() => {
                        let Some(message) = message else {
                            break;
                        };
                        transport.send_outbound(message).await;
                    }
                }
            }
        })
    }
}

impl From<&AutoPeeringPacket> for AutoPeerAnnounceDatagram {
    fn from(packet: &AutoPeeringPacket) -> Self {
        Self {
            kind: packet.kind,
            ifname: packet.ifname.clone(),
            source_link_local_address: packet.source_link_local_address.clone(),
            destination_address: packet.destination_address.clone(),
            destination_port: packet.destination_port,
            payload: packet.payload().to_vec(),
        }
    }
}

pub(crate) fn build_native_startup_plan(
    iface: &InterfaceConfig,
) -> Result<AutoDaemonStartupPlan, String> {
    let candidates = enumerate_link_local_candidates()?;
    build_startup_plan_from_candidates(iface, candidates)
}

fn build_startup_plan_from_candidates(
    iface: &InterfaceConfig,
    candidates: Vec<AutoInterfaceDeviceCandidate>,
) -> Result<AutoDaemonStartupPlan, String> {
    let config = auto_config(iface)?;
    let platform = current_platform();
    let timing = AutoInterfaceTiming::for_platform(platform);
    let filter = AutoInterfaceDeviceFilter {
        allowed: iface.devices.clone().unwrap_or_default(),
        ignored: iface.ignored_devices.clone().unwrap_or_default(),
    };
    let adopted_devices = filter.adopt_devices(&candidates, platform);
    let startup_plan = config.startup_plan(&adopted_devices, platform, timing);
    let peering_packets =
        adopted_devices.iter().map(|adopted| config.multicast_peering_packet(adopted)).collect();
    Ok(AutoDaemonStartupPlan {
        config,
        platform,
        candidates,
        adopted_devices,
        peering_packets,
        startup_plan,
    })
}

fn enumerate_link_local_candidates() -> Result<Vec<AutoInterfaceDeviceCandidate>, String> {
    let mut by_name = BTreeMap::<String, Vec<String>>::new();
    for iface in if_addrs::get_if_addrs().map_err(|err| format!("enumerate interfaces: {err}"))? {
        if !iface.is_oper_up() || iface.is_loopback() || !iface.is_link_local() {
            continue;
        }
        let if_addrs::IfAddr::V6(addr) = iface.addr else {
            continue;
        };
        by_name.entry(iface.name).or_default().push(addr.ip.to_string());
    }
    Ok(by_name
        .into_iter()
        .map(|(ifname, ipv6_addresses)| AutoInterfaceDeviceCandidate { ifname, ipv6_addresses })
        .collect())
}

fn auto_config(iface: &InterfaceConfig) -> Result<AutoInterfaceConfig, String> {
    Ok(AutoInterfaceConfig {
        group_id: iface.group_id.clone().unwrap_or_else(|| "reticulum".to_string()),
        discovery_scope: AutoDiscoveryScope::parse(
            iface.discovery_scope.as_deref().unwrap_or("link"),
        )
        .ok_or_else(|| "auto discovery_scope was not normalized".to_string())?,
        multicast_address_type: MulticastAddressType::parse(
            iface.multicast_address_type.as_deref().unwrap_or("temporary"),
        )
        .ok_or_else(|| "auto multicast_address_type was not normalized".to_string())?,
        discovery_port: iface.discovery_port.unwrap_or(29_716),
        data_port: iface.data_port.unwrap_or(42_671),
    })
}

fn startup_plan_json(plan: &AutoStartupPlan) -> JsonValue {
    json!({
        "discovery_listeners": plan.discovery_listeners.iter().map(discovery_listener_json).collect::<Vec<_>>(),
        "data_listeners": plan.data_listeners.iter().map(data_listener_json).collect::<Vec<_>>(),
        "peer_job_interval_ms": plan.peer_job_interval.as_millis() as u64,
        "initial_peering_wait_ms": plan.initial_peering_wait.as_millis() as u64,
    })
}

fn discovery_listener_json(listener: &AutoDiscoveryListenerBinding) -> JsonValue {
    json!({
        "ifname": listener.ifname,
        "link_local_address": listener.link_local_address,
        "unicast_bind_address": listener.unicast_bind_address,
        "unicast_bind_port": listener.unicast_bind_port,
        "multicast_group_address": listener.multicast_group_address,
        "multicast_bind_address": listener.multicast_bind_address,
        "multicast_bind_port": listener.multicast_bind_port,
    })
}

fn data_listener_json(listener: &AutoDataListenerBinding) -> JsonValue {
    json!({
        "ifname": listener.ifname,
        "link_local_address": listener.link_local_address,
        "bind_address": listener.bind_address,
        "bind_port": listener.bind_port,
    })
}

fn candidate_json(candidate: &AutoInterfaceDeviceCandidate) -> JsonValue {
    json!({
        "ifname": candidate.ifname,
        "ipv6_addresses": candidate.ipv6_addresses,
    })
}

fn adopted_json(adopted: &AutoInterfaceAdoptedDevice) -> JsonValue {
    json!({
        "ifname": adopted.ifname,
        "link_local_address": adopted.link_local_address,
    })
}

fn peering_datagram_json(datagram: &AutoPeerAnnounceDatagram) -> JsonValue {
    let target = datagram.socket_target();
    json!({
        "kind": peering_packet_kind(datagram.kind),
        "ifname": datagram.ifname,
        "source_link_local_address": datagram.source_link_local_address,
        "destination_address": datagram.destination_address,
        "destination_port": datagram.destination_port,
        "destination_host": target.host,
        "destination_scope_ifname": target.scope_ifname,
        "destination_socket_target": target.display(),
        "payload_hex": hex::encode(&datagram.payload),
    })
}

fn discovery_socket_bind_json(target: &AutoDiscoverySocketBindTarget) -> JsonValue {
    json!({
        "kind": discovery_socket_kind(target.kind),
        "ifname": target.ifname,
        "bind_host": target.bind_host,
        "bind_port": target.bind_port,
        "scope_ifname": target.scope_ifname,
        "bind_socket_target": target.display_bind_addr(),
        "multicast_group_host": target.multicast_group_host,
    })
}

fn data_socket_bind_json(target: &AutoDataSocketBindTarget) -> JsonValue {
    json!({
        "ifname": target.ifname,
        "bind_host": target.bind_host,
        "bind_port": target.bind_port,
        "scope_ifname": target.scope_ifname,
        "bind_socket_target": target.display_bind_addr(),
    })
}

pub(crate) fn discovery_runtime_summary_json(summary: &AutoDiscoveryRuntimeSummary) -> JsonValue {
    json!({
        "bound_socket_count": summary.bound_socket_count,
        "receive_loop_count": summary.receive_loop_count,
        "initial_peer_announce_count": summary.initial_peer_announce_count,
        "repeat_peer_announce_scheduler_count": summary.repeat_peer_announce_scheduler_count,
        "peer_job_scheduler_count": summary.peer_job_scheduler_count,
        "data_socket_count": summary.data_socket_count,
        "data_receive_loop_count": summary.data_receive_loop_count,
    })
}

fn discovery_source_address(datagram: &AutoDiscoveryDatagram) -> String {
    datagram.source_addr.ip().to_string()
}

fn peer_data_source_address(datagram: &AutoPeerDataDatagram) -> String {
    datagram.source_addr.ip().to_string()
}

fn log_auto_discovery_loop_event(event: AutoDiscoveryLoopEvent) {
    match event {
        AutoDiscoveryLoopEvent::Processed(processed) => {
            log::debug!(
                "[daemon-auto] discovery accepted iface={} source={} event={:?}",
                processed.datagram.ifname,
                processed.source_address,
                processed.event
            );
        }
        AutoDiscoveryLoopEvent::Rejected { datagram, source_address, reason } => {
            log::debug!(
                "[daemon-auto] discovery rejected iface={} source={} reason={:?}",
                datagram.ifname,
                source_address,
                reason
            );
        }
        AutoDiscoveryLoopEvent::ReceiveFailed { ifname, kind, bind_addr, error } => {
            log::warn!(
                "[daemon-auto] discovery receive failed iface={} kind={} bind={} err={}",
                ifname,
                discovery_socket_kind(kind),
                bind_addr,
                error
            );
        }
    }
}

fn log_auto_peer_data_loop_event(event: AutoPeerDataLoopEvent) {
    match event {
        AutoPeerDataLoopEvent::Processed(processed) => {
            log::debug!(
                "[daemon-auto] peer data processed iface={} peer={} decision={:?}",
                processed.datagram.ifname,
                processed.peer_address,
                processed.decision
            );
        }
        AutoPeerDataLoopEvent::ReceiveFailed { ifname, bind_addr, error } => {
            log::warn!(
                "[daemon-auto] peer data receive failed iface={} bind={} err={}",
                ifname,
                bind_addr,
                error
            );
        }
    }
}

fn peering_packet_kind(kind: AutoPeeringPacketKind) -> &'static str {
    match kind {
        AutoPeeringPacketKind::Multicast => "multicast",
        AutoPeeringPacketKind::ReverseUnicast => "reverse_unicast",
    }
}

fn discovery_socket_kind(kind: AutoDiscoverySocketKind) -> &'static str {
    match kind {
        AutoDiscoverySocketKind::Unicast => "unicast",
        AutoDiscoverySocketKind::Multicast => "multicast",
    }
}

fn current_platform() -> AutoInterfacePlatform {
    if cfg!(target_os = "windows") {
        AutoInterfacePlatform::Windows
    } else if cfg!(target_os = "macos") {
        AutoInterfacePlatform::Darwin
    } else if cfg!(target_os = "android") {
        AutoInterfacePlatform::Android
    } else {
        AutoInterfacePlatform::Other
    }
}

fn platform_name(platform: AutoInterfacePlatform) -> &'static str {
    match platform {
        AutoInterfacePlatform::Other => "other",
        AutoInterfacePlatform::Darwin => "darwin",
        AutoInterfacePlatform::Windows => "windows",
        AutoInterfacePlatform::Android => "android",
    }
}

fn socket_target(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

fn is_link_scope_ipv6_multicast(address: &str) -> bool {
    let first_segment = address.split(':').next().unwrap_or_default();
    let bytes = first_segment.as_bytes();
    bytes.len() >= 4
        && bytes[0].eq_ignore_ascii_case(&b'f')
        && bytes[1].eq_ignore_ascii_case(&b'f')
        && bytes[3] == b'2'
}

fn split_ipv6_scope(address: &str) -> (&str, Option<&str>) {
    match address.split_once('%') {
        Some((host, scope)) => (host, Some(scope)),
        None => (address, None),
    }
}

fn bind_host_and_scope(address: &str, fallback_scope_ifname: &str) -> (String, Option<String>) {
    if address.trim().is_empty() {
        return ("::".to_string(), None);
    }
    let (host, explicit_scope) = split_ipv6_scope(address);
    let scope_ifname = explicit_scope
        .map(str::to_string)
        .or_else(|| is_link_scope_ipv6_multicast(host).then(|| fallback_scope_ifname.to_string()));
    (host.to_string(), scope_ifname)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auto_iface() -> InterfaceConfig {
        InterfaceConfig {
            kind: "auto".to_string(),
            group_id: Some("field-net".to_string()),
            discovery_scope: Some("global".to_string()),
            multicast_address_type: Some("permanent".to_string()),
            discovery_port: Some(48_555),
            data_port: Some(49_555),
            devices: Some(vec!["eth0".to_string()]),
            ignored_devices: Some(vec!["tun0".to_string()]),
            ..InterfaceConfig::default()
        }
    }

    fn default_link_auto_iface() -> InterfaceConfig {
        InterfaceConfig {
            kind: "auto".to_string(),
            group_id: Some("reticulum".to_string()),
            discovery_scope: Some("link".to_string()),
            multicast_address_type: Some("temporary".to_string()),
            discovery_port: Some(29_716),
            data_port: Some(42_671),
            devices: Some(vec!["eth0".to_string()]),
            ..InterfaceConfig::default()
        }
    }

    fn empty_startup_plan() -> AutoStartupPlan {
        AutoStartupPlan {
            discovery_listeners: Vec::new(),
            data_listeners: Vec::new(),
            peer_job_interval: core::time::Duration::ZERO,
            initial_peering_wait: core::time::Duration::ZERO,
        }
    }

    fn plan_with_discovery_listener(
        listener: AutoDiscoveryListenerBinding,
    ) -> AutoDaemonStartupPlan {
        AutoDaemonStartupPlan {
            config: AutoInterfaceConfig::default(),
            platform: AutoInterfacePlatform::Other,
            candidates: Vec::new(),
            adopted_devices: Vec::new(),
            peering_packets: Vec::new(),
            startup_plan: AutoStartupPlan {
                discovery_listeners: vec![listener],
                data_listeners: Vec::new(),
                peer_job_interval: core::time::Duration::ZERO,
                initial_peering_wait: core::time::Duration::ZERO,
            },
        }
    }

    fn plan_with_data_listener(listener: AutoDataListenerBinding) -> AutoDaemonStartupPlan {
        AutoDaemonStartupPlan {
            config: AutoInterfaceConfig::default(),
            platform: AutoInterfacePlatform::Other,
            candidates: Vec::new(),
            adopted_devices: Vec::new(),
            peering_packets: Vec::new(),
            startup_plan: AutoStartupPlan {
                discovery_listeners: Vec::new(),
                data_listeners: vec![listener],
                peer_job_interval: core::time::Duration::ZERO,
                initial_peering_wait: core::time::Duration::ZERO,
            },
        }
    }

    #[test]
    fn auto_interface_index_resolver_uses_indexed_interfaces_only() {
        let resolver = AutoInterfaceIndexResolver::from_index_entries([
            ("eth0".to_string(), Some(7)),
            ("lo".to_string(), None),
            ("wlan0".to_string(), Some(11)),
        ]);

        assert_eq!(resolver.resolve("eth0"), Ok(7));
        assert_eq!(resolver.resolve("wlan0"), Ok(11));
        assert_eq!(resolver.resolve("lo"), Err("interface index for lo was not found".to_string()));
        assert_eq!(
            resolver.resolve("missing0"),
            Err("interface index for missing0 was not found".to_string())
        );
    }

    #[test]
    fn auto_interface_index_resolver_drives_scoped_socket_resolution() {
        let resolver =
            AutoInterfaceIndexResolver::from_index_entries([("eth0".to_string(), Some(7))]);
        let target = AutoPeerAnnounceSocketTarget {
            host: "ff12:0:d70b:fb1c:16e4:5e39:485e:31e1".to_string(),
            port: 29_716,
            scope_ifname: Some("eth0".to_string()),
        };

        let resolved = target.resolve_socket_addr(|ifname| resolver.resolve(ifname)).unwrap();

        assert_eq!(resolved.to_string(), "[ff12:0:d70b:fb1c:16e4:5e39:485e:31e1%7]:29716");
    }

    #[test]
    fn auto_startup_plan_adopts_configured_link_local_candidates() {
        let plan = build_startup_plan_from_candidates(
            &auto_iface(),
            vec![
                AutoInterfaceDeviceCandidate {
                    ifname: "eth0".to_string(),
                    ipv6_addresses: vec!["fe80::1234".to_string()],
                },
                AutoInterfaceDeviceCandidate {
                    ifname: "wlan0".to_string(),
                    ipv6_addresses: vec!["fe80::5678".to_string()],
                },
                AutoInterfaceDeviceCandidate {
                    ifname: "tun0".to_string(),
                    ipv6_addresses: vec!["fe80::9999".to_string()],
                },
            ],
        )
        .expect("startup plan");

        assert_eq!(
            plan.adopted_devices,
            vec![AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1234".to_string(),
            }]
        );
        assert_eq!(plan.startup_plan.discovery_listeners.len(), 1);
        assert_eq!(plan.startup_plan.data_listeners.len(), 1);
        assert_eq!(plan.startup_plan.data_listeners[0].bind_port, 49_555);
        assert_eq!(plan.peering_packets.len(), 1);
        assert_eq!(plan.peering_packets[0].kind, AutoPeeringPacketKind::Multicast);
        assert_eq!(plan.peering_packets[0].ifname, "eth0");
        assert_eq!(plan.peering_packets[0].destination_port, 48_555);
        assert_eq!(plan.peering_packets[0].payload(), &plan.peering_packets[0].token);
        assert_eq!(plan.initial_peer_announce_datagrams().len(), 1);
        assert_eq!(
            plan.initial_peer_announce_datagrams()[0].payload,
            plan.peering_packets[0].token.to_vec()
        );
    }

    #[test]
    fn auto_runtime_json_exposes_complete_socket_runtime_plan() {
        let plan = build_startup_plan_from_candidates(
            &auto_iface(),
            vec![AutoInterfaceDeviceCandidate {
                ifname: "eth0".to_string(),
                ipv6_addresses: vec!["fe80::1234".to_string()],
            }],
        )
        .expect("startup plan");
        let runtime = plan.runtime_json();

        assert_eq!(
            runtime.get("auto_runtime_status").and_then(JsonValue::as_str),
            Some("complete")
        );
        assert_eq!(
            runtime
                .get("startup_plan")
                .and_then(|value| value.get("data_listeners"))
                .and_then(JsonValue::as_array)
                .map(Vec::len),
            Some(1)
        );
        assert_eq!(
            runtime
                .get("initial_peer_announces")
                .and_then(JsonValue::as_array)
                .and_then(|items| items.first())
                .and_then(|item| item.get("kind"))
                .and_then(JsonValue::as_str),
            Some("multicast")
        );
        assert!(runtime
            .get("initial_peer_announces")
            .and_then(JsonValue::as_array)
            .and_then(|items| items.first())
            .and_then(|item| item.get("payload_hex"))
            .and_then(JsonValue::as_str)
            .is_some_and(|payload| payload.len() == rns_transport::hash::HASH_SIZE * 2));
        assert_eq!(
            runtime
                .get("initial_peer_announces")
                .and_then(JsonValue::as_array)
                .and_then(|items| items.first())
                .and_then(|item| item.get("destination_socket_target"))
                .and_then(JsonValue::as_str),
            Some("[ff0e:0:77b9:4bfd:9488:364b:4bbe:119d]:48555")
        );
        assert_eq!(
            runtime.get("planned_initial_peer_announce_count").and_then(JsonValue::as_u64),
            Some(1)
        );
        assert_eq!(
            runtime.get("planned_repeat_peer_announce_scheduler_count").and_then(JsonValue::as_u64),
            Some(1)
        );
        assert_eq!(
            runtime.get("planned_peer_job_scheduler_count").and_then(JsonValue::as_u64),
            Some(1)
        );
        assert_eq!(
            runtime
                .get("planned_discovery_socket_binds")
                .and_then(JsonValue::as_array)
                .map(Vec::len),
            Some(2)
        );
        assert_eq!(
            runtime.get("planned_discovery_receive_loop_count").and_then(JsonValue::as_u64),
            Some(2)
        );
        assert_eq!(
            runtime.get("planned_data_socket_binds").and_then(JsonValue::as_array).map(Vec::len),
            Some(1)
        );
        assert_eq!(
            runtime.get("planned_data_receive_loop_count").and_then(JsonValue::as_u64),
            Some(1)
        );
        assert_eq!(
            runtime.get("native_scope_id_source").and_then(JsonValue::as_str),
            Some("if-addrs interface index")
        );
        assert_eq!(
            runtime
                .get("initial_peer_announces")
                .and_then(JsonValue::as_array)
                .and_then(|items| items.first())
                .and_then(|item| item.get("destination_scope_ifname"))
                .and_then(JsonValue::as_str),
            None
        );
    }

    #[test]
    fn auto_initial_peer_announce_sender_exposes_datagram_payloads() {
        let plan = build_startup_plan_from_candidates(
            &auto_iface(),
            vec![AutoInterfaceDeviceCandidate {
                ifname: "eth0".to_string(),
                ipv6_addresses: vec!["fe80::1234".to_string()],
            }],
        )
        .expect("startup plan");
        let mut sent = Vec::new();

        let count = plan
            .send_initial_peer_announces(|datagram| {
                sent.push(datagram.clone());
                Ok(())
            })
            .expect("send planned datagrams");

        assert_eq!(count, 1);
        assert_eq!(sent[0].kind, AutoPeeringPacketKind::Multicast);
        assert_eq!(sent[0].destination_port, 48_555);
        assert_eq!(sent[0].payload, plan.peering_packets[0].token.to_vec());
    }

    #[test]
    fn auto_initial_peer_announce_sender_reports_destination_on_error() {
        let plan = build_startup_plan_from_candidates(
            &default_link_auto_iface(),
            vec![AutoInterfaceDeviceCandidate {
                ifname: "eth0".to_string(),
                ipv6_addresses: vec!["fe80::1234".to_string()],
            }],
        )
        .expect("startup plan");

        let err = plan
            .send_initial_peer_announces(|_| Err("socket unavailable".to_string()))
            .expect_err("send failure should propagate");

        assert!(err.contains("send auto peer announce 1/1"));
        assert!(err.contains("[ff12:0:d70b:fb1c:16e4:5e39:485e:31e1%eth0]:29716"));
        assert!(err.contains("socket unavailable"));
    }

    #[test]
    fn auto_repeat_peer_announce_job_uses_python_interval_after_initial_send() {
        let plan = build_startup_plan_from_candidates(
            &auto_iface(),
            vec![AutoInterfaceDeviceCandidate {
                ifname: "eth0".to_string(),
                ipv6_addresses: vec!["fe80::1234".to_string()],
            }],
        )
        .expect("startup plan");
        let mut state = plan.discovery_state();
        let mut sent = Vec::new();

        let initial = plan
            .run_multicast_peer_announce_job(&mut state, core::time::Duration::ZERO, |datagram| {
                sent.push(datagram.clone());
                Ok(())
            })
            .expect("initial multicast peer announce");
        let early = plan
            .run_multicast_peer_announce_job(
                &mut state,
                core::time::Duration::from_millis(1_599),
                |_| panic!("announce should not be due before the interval"),
            )
            .expect("early multicast peer announce check");
        let repeat = plan
            .run_multicast_peer_announce_job(
                &mut state,
                core::time::Duration::from_millis(1_600),
                |datagram| {
                    sent.push(datagram.clone());
                    Ok(())
                },
            )
            .expect("repeat multicast peer announce");

        assert_eq!(initial, 1);
        assert_eq!(early, 0);
        assert_eq!(repeat, 1);
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].kind, AutoPeeringPacketKind::Multicast);
        assert_eq!(sent[0], sent[1]);
    }

    #[test]
    fn auto_peer_job_sends_reverse_announces_on_python_interval() {
        let plan = build_startup_plan_from_candidates(
            &auto_iface(),
            vec![AutoInterfaceDeviceCandidate {
                ifname: "eth0".to_string(),
                ipv6_addresses: vec!["fe80::1234".to_string()],
            }],
        )
        .expect("startup plan");
        let mut state = plan.discovery_state();
        state.observe_discovery_packet("fe80::2222%eth0", "eth0", core::time::Duration::ZERO);

        let early = plan
            .run_peer_job(&mut state, core::time::Duration::from_millis(5_200), |_| {
                panic!("reverse announce should not be due at the interval boundary")
            })
            .expect("early peer job");
        let mut sent = Vec::new();
        let due = plan
            .run_peer_job(&mut state, core::time::Duration::from_millis(5_201), |datagram| {
                sent.push(datagram.clone());
                Ok(())
            })
            .expect("due peer job");
        let repeated = plan
            .run_peer_job(&mut state, core::time::Duration::from_millis(10_401), |_| {
                panic!("reverse announce should be marked sent")
            })
            .expect("repeated peer job");

        assert_eq!(early.reverse_peer_announce_count, 0);
        assert_eq!(due.reverse_peer_announce_count, 1);
        assert_eq!(repeated.reverse_peer_announce_count, 0);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].kind, AutoPeeringPacketKind::ReverseUnicast);
        assert_eq!(sent[0].destination_address, "fe80::2222%eth0");
        assert_eq!(sent[0].destination_port, 48_556);
        assert_eq!(sent[0].source_link_local_address, "fe80::1234");
    }

    #[test]
    fn auto_discovery_socket_bind_targets_format_unicast_and_multicast_scopes() {
        let plan = plan_with_discovery_listener(AutoDiscoveryListenerBinding {
            ifname: "eth0".to_string(),
            link_local_address: "fe80::1234".to_string(),
            unicast_bind_address: "fe80::1234%eth0".to_string(),
            unicast_bind_port: 29_717,
            multicast_group_address: "ff12:0:d70b:fb1c:16e4:5e39:485e:31e1".to_string(),
            multicast_bind_address: "ff12:0:d70b:fb1c:16e4:5e39:485e:31e1%eth0".to_string(),
            multicast_bind_port: 29_716,
        });

        let targets = plan.discovery_socket_bind_targets();

        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].kind, AutoDiscoverySocketKind::Unicast);
        assert_eq!(targets[0].display_bind_addr(), "[fe80::1234%eth0]:29717");
        assert_eq!(targets[1].kind, AutoDiscoverySocketKind::Multicast);
        assert_eq!(
            targets[1].display_bind_addr(),
            "[ff12:0:d70b:fb1c:16e4:5e39:485e:31e1%eth0]:29716"
        );
        assert_eq!(
            targets[1].multicast_group_host.as_deref(),
            Some("ff12:0:d70b:fb1c:16e4:5e39:485e:31e1")
        );
    }

    #[test]
    fn auto_data_socket_bind_targets_format_scoped_listener() {
        let plan = plan_with_data_listener(AutoDataListenerBinding {
            ifname: "eth0".to_string(),
            link_local_address: "fe80::1234".to_string(),
            bind_address: "fe80::1234%eth0".to_string(),
            bind_port: 42_671,
        });

        let targets = plan.data_socket_bind_targets();

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].ifname, "eth0");
        assert_eq!(targets[0].display_bind_addr(), "[fe80::1234%eth0]:42671");
        assert_eq!(targets[0].scope_ifname.as_deref(), Some("eth0"));
    }

    #[test]
    fn auto_discovery_socket_bind_targets_use_unspecified_for_windows_empty_hosts() {
        let plan = plan_with_discovery_listener(AutoDiscoveryListenerBinding {
            ifname: "Ethernet".to_string(),
            link_local_address: "fe80::1234".to_string(),
            unicast_bind_address: String::new(),
            unicast_bind_port: 29_717,
            multicast_group_address: "ff12:0:d70b:fb1c:16e4:5e39:485e:31e1".to_string(),
            multicast_bind_address: String::new(),
            multicast_bind_port: 29_716,
        });

        let targets = plan.discovery_socket_bind_targets();

        assert_eq!(targets[0].display_bind_addr(), "[::]:29717");
        assert_eq!(targets[0].scope_ifname, None);
        assert_eq!(targets[1].display_bind_addr(), "[::]:29716");
        assert_eq!(targets[1].scope_ifname, None);
    }

    #[test]
    fn auto_multicast_discovery_bind_resolves_link_scope_group_to_unspecified_bind() {
        let target = AutoDiscoverySocketBindTarget {
            kind: AutoDiscoverySocketKind::Multicast,
            ifname: "eth0".to_string(),
            bind_host: "ff12:0:d70b:fb1c:16e4:5e39:485e:31e1".to_string(),
            bind_port: 29_716,
            scope_ifname: Some("eth0".to_string()),
            multicast_group_host: Some("ff12:0:d70b:fb1c:16e4:5e39:485e:31e1".to_string()),
        };

        let resolved = target
            .resolve_multicast_bind(|ifname| {
                assert_eq!(ifname, "eth0");
                Ok(7)
            })
            .expect("resolve multicast bind");

        assert_eq!(resolved.bind_addr.to_string(), "[::]:29716");
        assert_eq!(
            resolved.multicast_group_addr.to_string(),
            "[ff12:0:d70b:fb1c:16e4:5e39:485e:31e1%7]:29716"
        );
        assert_eq!(resolved.multicast_scope_id, 7);
    }

    #[test]
    fn auto_multicast_discovery_bind_uses_ifname_scope_for_windows_empty_host() {
        let target = AutoDiscoverySocketBindTarget {
            kind: AutoDiscoverySocketKind::Multicast,
            ifname: "Ethernet".to_string(),
            bind_host: "::".to_string(),
            bind_port: 29_716,
            scope_ifname: None,
            multicast_group_host: Some("ff12:0:d70b:fb1c:16e4:5e39:485e:31e1".to_string()),
        };

        let resolved = target
            .resolve_multicast_bind(|ifname| {
                assert_eq!(ifname, "Ethernet");
                Ok(11)
            })
            .expect("resolve multicast bind");

        assert_eq!(resolved.bind_addr.to_string(), "[::]:29716");
        assert_eq!(
            resolved.multicast_group_addr.to_string(),
            "[ff12:0:d70b:fb1c:16e4:5e39:485e:31e1%11]:29716"
        );
        assert_eq!(resolved.multicast_scope_id, 11);
    }

    #[test]
    fn auto_peer_announce_datagram_formats_socket_targets_for_ipv6_multicast() {
        let link_plan = build_startup_plan_from_candidates(
            &default_link_auto_iface(),
            vec![AutoInterfaceDeviceCandidate {
                ifname: "eth0".to_string(),
                ipv6_addresses: vec!["fe80::1234".to_string()],
            }],
        )
        .expect("link startup plan");
        let link_datagram = link_plan.initial_peer_announce_datagrams().remove(0);
        assert_eq!(
            link_datagram.destination_socket_target(),
            "[ff12:0:d70b:fb1c:16e4:5e39:485e:31e1%eth0]:29716"
        );
        assert_eq!(
            link_datagram.socket_target(),
            AutoPeerAnnounceSocketTarget {
                host: "ff12:0:d70b:fb1c:16e4:5e39:485e:31e1".to_string(),
                port: 29_716,
                scope_ifname: Some("eth0".to_string()),
            }
        );

        let global_plan = build_startup_plan_from_candidates(
            &auto_iface(),
            vec![AutoInterfaceDeviceCandidate {
                ifname: "eth0".to_string(),
                ipv6_addresses: vec!["fe80::1234".to_string()],
            }],
        )
        .expect("global startup plan");
        let global_datagram = global_plan.initial_peer_announce_datagrams().remove(0);
        assert_eq!(
            global_datagram.destination_socket_target(),
            "[ff0e:0:77b9:4bfd:9488:364b:4bbe:119d]:48555"
        );
        assert_eq!(
            global_datagram.socket_target(),
            AutoPeerAnnounceSocketTarget {
                host: "ff0e:0:77b9:4bfd:9488:364b:4bbe:119d".to_string(),
                port: 48_555,
                scope_ifname: None,
            }
        );
    }

    #[test]
    fn auto_peer_announce_socket_target_preserves_explicit_scope() {
        let datagram = AutoPeerAnnounceDatagram {
            kind: AutoPeeringPacketKind::ReverseUnicast,
            ifname: "wlan0".to_string(),
            source_link_local_address: "fe80::1111".to_string(),
            destination_address: "fe80::2222%wlan0".to_string(),
            destination_port: 29_717,
            payload: vec![0; rns_transport::hash::HASH_SIZE],
        };

        assert_eq!(
            datagram.socket_target(),
            AutoPeerAnnounceSocketTarget {
                host: "fe80::2222".to_string(),
                port: 29_717,
                scope_ifname: Some("wlan0".to_string()),
            }
        );
        assert_eq!(datagram.destination_socket_target(), "[fe80::2222%wlan0]:29717");
    }

    #[test]
    fn auto_peer_announce_socket_target_resolves_scoped_ipv6() {
        let target = AutoPeerAnnounceSocketTarget {
            host: "fe80::2222".to_string(),
            port: 29_717,
            scope_ifname: Some("eth0".to_string()),
        };

        let resolved = target
            .resolve_socket_addr(|ifname| {
                assert_eq!(ifname, "eth0");
                Ok(7)
            })
            .expect("resolve scoped address");

        assert_eq!(resolved.to_string(), "[fe80::2222%7]:29717");
    }

    #[test]
    fn auto_peer_announce_socket_target_rejects_scope_on_ipv4() {
        let target = AutoPeerAnnounceSocketTarget {
            host: "127.0.0.1".to_string(),
            port: 29_717,
            scope_ifname: Some("eth0".to_string()),
        };

        let err = target.resolve_socket_addr(|_| Ok(7)).expect_err("IPv4 scope should be rejected");

        assert!(err.contains("IPv4 destination 127.0.0.1 cannot use scope interface eth0"));
    }

    #[tokio::test]
    async fn auto_initial_peer_announces_udp_socket_sender_transmits_payload() {
        let receiver = tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("bind receiver");
        let receiver_addr = receiver.local_addr().expect("receiver addr");
        let sender = tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("bind sender");
        let token = [0x42; rns_transport::hash::HASH_SIZE];
        let plan = AutoDaemonStartupPlan {
            config: AutoInterfaceConfig::default(),
            platform: AutoInterfacePlatform::Other,
            candidates: Vec::new(),
            adopted_devices: Vec::new(),
            peering_packets: vec![AutoPeeringPacket {
                kind: AutoPeeringPacketKind::ReverseUnicast,
                ifname: "lo".to_string(),
                source_link_local_address: "127.0.0.1".to_string(),
                destination_address: receiver_addr.ip().to_string(),
                destination_port: receiver_addr.port(),
                token,
            }],
            startup_plan: empty_startup_plan(),
        };

        let count = plan
            .send_initial_peer_announces_with_udp_socket(&sender, |_| {
                panic!("IPv4 target should not need a scope id")
            })
            .await
            .expect("send datagram");

        let mut payload = [0u8; rns_transport::hash::HASH_SIZE];
        let (received, _) = receiver.recv_from(&mut payload).await.expect("receive datagram");
        assert_eq!(count, 1);
        assert_eq!(received, rns_transport::hash::HASH_SIZE);
        assert_eq!(payload, token);
    }

    #[tokio::test]
    async fn auto_bind_unicast_discovery_sockets_binds_loopback_listener() {
        let plan = plan_with_discovery_listener(AutoDiscoveryListenerBinding {
            ifname: "lo".to_string(),
            link_local_address: "::1".to_string(),
            unicast_bind_address: "::1".to_string(),
            unicast_bind_port: 0,
            multicast_group_address: "ff0e:0:77b9:4bfd:9488:364b:4bbe:119d".to_string(),
            multicast_bind_address: "ff0e:0:77b9:4bfd:9488:364b:4bbe:119d".to_string(),
            multicast_bind_port: 48_555,
        });

        let sockets = plan
            .bind_unicast_discovery_sockets(|_| panic!("loopback unicast bind is unscoped"))
            .await
            .expect("bind unicast discovery socket");

        assert_eq!(sockets.len(), 1);
        assert_eq!(sockets[0].kind, AutoDiscoverySocketKind::Unicast);
        assert_eq!(sockets[0].ifname, "lo");
        assert_eq!(sockets[0].multicast_group_addr, None);
        assert!(sockets[0].bind_addr.is_ipv6());
        assert_ne!(sockets[0].bind_addr.port(), 0);
    }

    #[tokio::test]
    async fn auto_bind_peer_data_socket_receives_typed_datagram() {
        let plan = plan_with_data_listener(AutoDataListenerBinding {
            ifname: "lo".to_string(),
            link_local_address: "127.0.0.1".to_string(),
            bind_address: "127.0.0.1".to_string(),
            bind_port: 0,
        });
        let sockets = plan
            .bind_data_sockets(|_| panic!("IPv4 data bind is unscoped"))
            .await
            .expect("bind peer data socket");
        let sender = tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("bind sender");
        let payload = b"auto-peer-data";

        sender.send_to(payload, sockets[0].bind_addr).await.expect("send peer data datagram");
        let datagram = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            sockets[0].recv_peer_data_datagram(),
        )
        .await
        .expect("receive timeout")
        .expect("receive peer data datagram");

        assert_eq!(datagram.ifname, "lo");
        assert_eq!(datagram.bind_addr, sockets[0].bind_addr);
        assert_eq!(datagram.source_addr.ip(), sender.local_addr().expect("sender addr").ip());
        assert_eq!(datagram.payload, payload);
    }

    #[tokio::test]
    async fn auto_bound_discovery_socket_receives_typed_datagram() {
        let plan = plan_with_discovery_listener(AutoDiscoveryListenerBinding {
            ifname: "lo".to_string(),
            link_local_address: "127.0.0.1".to_string(),
            unicast_bind_address: "127.0.0.1".to_string(),
            unicast_bind_port: 0,
            multicast_group_address: "239.255.0.1".to_string(),
            multicast_bind_address: "239.255.0.1".to_string(),
            multicast_bind_port: 0,
        });
        let sockets = plan
            .bind_unicast_discovery_sockets(|_| panic!("IPv4 unicast bind is unscoped"))
            .await
            .expect("bind unicast discovery socket");
        let sender = tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("bind sender");
        let payload = b"auto-discovery-token";

        sender.send_to(payload, sockets[0].bind_addr).await.expect("send discovery datagram");
        let datagram = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            sockets[0].recv_discovery_datagram(),
        )
        .await
        .expect("receive timeout")
        .expect("receive discovery datagram");

        assert_eq!(datagram.kind, AutoDiscoverySocketKind::Unicast);
        assert_eq!(datagram.ifname, "lo");
        assert_eq!(datagram.bind_addr, sockets[0].bind_addr);
        assert_eq!(datagram.multicast_group_addr, None);
        assert_eq!(datagram.source_addr.ip(), sender.local_addr().expect("sender addr").ip());
        assert_eq!(datagram.payload, payload);
    }

    #[tokio::test]
    async fn auto_discovery_receive_loop_authenticates_datagrams_and_reports_events() {
        let plan = plan_with_discovery_listener(AutoDiscoveryListenerBinding {
            ifname: "lo".to_string(),
            link_local_address: "127.0.0.1".to_string(),
            unicast_bind_address: "127.0.0.1".to_string(),
            unicast_bind_port: 0,
            multicast_group_address: "239.255.0.1".to_string(),
            multicast_bind_address: "239.255.0.1".to_string(),
            multicast_bind_port: 0,
        });
        let sockets = plan
            .bind_unicast_discovery_sockets(|_| panic!("IPv4 unicast bind is unscoped"))
            .await
            .expect("bind unicast discovery socket");
        let bind_addr = sockets[0].bind_addr;
        let state = Arc::new(tokio::sync::Mutex::new(plan.discovery_state()));
        let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(4);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let handles =
            plan.spawn_discovery_receive_loops(sockets, Arc::clone(&state), events_tx, shutdown_rx);
        let sender = tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("bind sender");
        let source_address = sender.local_addr().expect("sender addr").ip().to_string();
        let payload = rns_transport::iface::auto::peering_token(
            plan.config.group_id.as_bytes(),
            &source_address,
        );

        sender.send_to(&payload, bind_addr).await.expect("send valid discovery datagram");
        let accepted = tokio::time::timeout(std::time::Duration::from_secs(1), events_rx.recv())
            .await
            .expect("accepted event timeout")
            .expect("accepted event");

        match accepted {
            AutoDiscoveryLoopEvent::Processed(processed) => {
                assert_eq!(processed.source_address, source_address);
                assert_eq!(
                    processed.event,
                    AutoDiscoveryEvent::Peer(rns_transport::iface::auto::AutoPeerEvent::Added)
                );
            }
            other => panic!("unexpected accepted event: {other:?}"),
        }
        assert!(state.lock().await.peer(&source_address).is_some());

        sender
            .send_to(&[0; rns_transport::hash::HASH_SIZE], bind_addr)
            .await
            .expect("send invalid discovery datagram");
        let rejected = tokio::time::timeout(std::time::Duration::from_secs(1), events_rx.recv())
            .await
            .expect("rejected event timeout")
            .expect("rejected event");

        match rejected {
            AutoDiscoveryLoopEvent::Rejected {
                source_address: rejected_source, reason, ..
            } => {
                assert_eq!(rejected_source, source_address);
                assert_eq!(reason, AutoDiscoveryRejectReason::InvalidToken);
            }
            other => panic!("unexpected rejected event: {other:?}"),
        }

        shutdown_tx.send(true).expect("send shutdown");
        for handle in handles {
            tokio::time::timeout(std::time::Duration::from_secs(1), handle)
                .await
                .expect("receive loop shutdown timeout")
                .expect("receive loop task");
        }
    }

    #[tokio::test]
    async fn auto_peer_data_receive_loop_accepts_known_peer_and_suppresses_duplicate() {
        let plan = plan_with_data_listener(AutoDataListenerBinding {
            ifname: "lo".to_string(),
            link_local_address: "127.0.0.1".to_string(),
            bind_address: "127.0.0.1".to_string(),
            bind_port: 0,
        });
        let sockets = plan
            .bind_data_sockets(|_| panic!("IPv4 data bind is unscoped"))
            .await
            .expect("bind peer data socket");
        let bind_addr = sockets[0].bind_addr;
        let state = Arc::new(tokio::sync::Mutex::new(plan.discovery_state()));
        let dedupe = Arc::new(tokio::sync::Mutex::new(AutoInboundPacketDeduplicator::from_timing(
            AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other),
        )));
        let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(4);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let handles = plan.spawn_peer_data_receive_loops(
            sockets,
            Arc::clone(&state),
            Arc::clone(&dedupe),
            None,
            events_tx,
            shutdown_rx,
        );
        let sender = tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("bind sender");
        let source_address = sender.local_addr().expect("sender addr").ip().to_string();
        state.lock().await.observe_discovery_packet(
            &source_address,
            "lo",
            core::time::Duration::ZERO,
        );

        sender.send_to(b"packet", bind_addr).await.expect("send peer data datagram");
        let accepted = tokio::time::timeout(std::time::Duration::from_secs(1), events_rx.recv())
            .await
            .expect("accepted event timeout")
            .expect("accepted event");

        match accepted {
            AutoPeerDataLoopEvent::Processed(processed) => {
                assert_eq!(processed.peer_address, source_address);
                assert_eq!(processed.datagram.payload, b"packet");
                assert!(matches!(processed.decision, AutoPeerInboundDecision::Accepted { .. }));
            }
            other => panic!("unexpected accepted peer-data event: {other:?}"),
        }

        sender.send_to(b"packet", bind_addr).await.expect("send duplicate peer data datagram");
        let duplicate = tokio::time::timeout(std::time::Duration::from_secs(1), events_rx.recv())
            .await
            .expect("duplicate event timeout")
            .expect("duplicate event");

        match duplicate {
            AutoPeerDataLoopEvent::Processed(processed) => {
                assert_eq!(processed.peer_address, source_address);
                assert_eq!(processed.decision, AutoPeerInboundDecision::Duplicate);
            }
            other => panic!("unexpected duplicate peer-data event: {other:?}"),
        }

        shutdown_tx.send(true).expect("send shutdown");
        for handle in handles {
            tokio::time::timeout(std::time::Duration::from_secs(1), handle)
                .await
                .expect("peer data loop shutdown timeout")
                .expect("peer data loop task");
        }
    }

    #[tokio::test]
    async fn auto_peer_data_transport_bridge_registers_virtual_iface_and_routes_direct_tx() {
        let plan = plan_with_data_listener(AutoDataListenerBinding {
            ifname: "lo".to_string(),
            link_local_address: "127.0.0.1".to_string(),
            bind_address: "127.0.0.1".to_string(),
            bind_port: 0,
        });
        let sockets = plan
            .bind_data_sockets(|_| panic!("IPv4 data bind is unscoped"))
            .await
            .expect("bind peer data socket");
        let bind_addr = sockets[0].bind_addr;
        let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
        let rx_recv = iface_manager.lock().await.receiver();
        let channel = iface_manager.lock().await.new_channel_with_role(8, IfaceRole::Multicast);
        let host_iface = channel.address;
        let runtime =
            AutoInterfaceTransportRuntime::from_channel(channel, Arc::clone(&iface_manager));
        let (bridge, tx_channel) = runtime.split();
        let state = Arc::new(tokio::sync::Mutex::new(plan.discovery_state()));
        let dedupe = Arc::new(tokio::sync::Mutex::new(AutoInboundPacketDeduplicator::from_timing(
            AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other),
        )));
        let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(4);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let data_handles = plan.spawn_peer_data_receive_loops(
            sockets,
            Arc::clone(&state),
            dedupe,
            Some(bridge.clone()),
            events_tx,
            shutdown_rx.clone(),
        );
        let tx_handle = plan.spawn_peer_data_transport_tx_loop(bridge, tx_channel, shutdown_rx);
        let sender = tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("bind sender");
        let source_address = sender.local_addr().expect("sender addr").ip().to_string();
        state.lock().await.observe_discovery_packet(
            &source_address,
            "lo",
            core::time::Duration::ZERO,
        );
        let inbound_packet = Packet {
            destination: AddressHash::new_from_slice(
                &[0x44; rns_transport::hash::ADDRESS_HASH_SIZE],
            ),
            data: rns_transport::packet::PacketDataBuffer::new_from_slice(b"inbound"),
            ..Default::default()
        };
        let inbound_payload = inbound_packet.to_bytes().expect("serialize inbound packet");

        sender.send_to(&inbound_payload, bind_addr).await.expect("send peer data datagram");
        let processed = tokio::time::timeout(std::time::Duration::from_secs(1), events_rx.recv())
            .await
            .expect("processed event timeout")
            .expect("processed event");
        assert!(matches!(
            processed,
            AutoPeerDataLoopEvent::Processed(AutoProcessedPeerDataDatagram {
                decision: AutoPeerInboundDecision::Accepted { .. },
                ..
            })
        ));

        let rx_message =
            tokio::time::timeout(std::time::Duration::from_secs(1), rx_recv.lock().await.recv())
                .await
                .expect("rx message timeout")
                .expect("rx message");
        assert_ne!(rx_message.address, host_iface);
        assert_eq!(rx_message.packet, inbound_packet);
        assert_eq!(rx_message.source, IfaceSource::Udp(sender.local_addr().expect("sender addr")));
        assert_eq!(
            iface_manager.lock().await.role(&rx_message.address),
            Some(IfaceRole::VirtualUnicast)
        );

        let outbound_packet = Packet {
            destination: AddressHash::new_from_slice(
                &[0x55; rns_transport::hash::ADDRESS_HASH_SIZE],
            ),
            data: rns_transport::packet::PacketDataBuffer::new_from_slice(b"outbound"),
            ..Default::default()
        };
        iface_manager
            .lock()
            .await
            .send(TxMessage {
                tx_type: TxMessageType::Direct(rx_message.address),
                packet: outbound_packet,
            })
            .await;
        let mut outbound_payload = [0u8; 512];
        let (received, _) = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            sender.recv_from(&mut outbound_payload),
        )
        .await
        .expect("outbound receive timeout")
        .expect("outbound receive");
        let decoded = Packet::deserialize(&mut InputBuffer::new(&outbound_payload[..received]))
            .expect("decode outbound packet");
        assert_eq!(decoded, outbound_packet);

        shutdown_tx.send(true).expect("send shutdown");
        for handle in data_handles {
            tokio::time::timeout(std::time::Duration::from_secs(1), handle)
                .await
                .expect("peer data loop shutdown timeout")
                .expect("peer data loop task");
        }
        tokio::time::timeout(std::time::Duration::from_secs(1), tx_handle)
            .await
            .expect("tx loop shutdown timeout")
            .expect("tx loop task");
    }

    #[test]
    fn auto_process_discovery_datagram_authenticates_local_echo() {
        let plan = build_startup_plan_from_candidates(
            &auto_iface(),
            vec![AutoInterfaceDeviceCandidate {
                ifname: "eth0".to_string(),
                ipv6_addresses: vec!["fe80::1234".to_string()],
            }],
        )
        .expect("startup plan");
        let mut state = plan.discovery_state();
        let datagram = AutoDiscoveryDatagram {
            kind: AutoDiscoverySocketKind::Multicast,
            ifname: "eth0".to_string(),
            bind_addr: "[::]:48555".parse().expect("bind addr"),
            multicast_group_addr: Some(
                "[ff0e:0:77b9:4bfd:9488:364b:4bbe:119d]:48555".parse().expect("group addr"),
            ),
            source_addr: "[fe80::1234]:48555".parse().expect("source addr"),
            payload: rns_transport::iface::auto::peering_token(b"field-net", "fe80::1234").to_vec(),
        };

        let processed = plan
            .process_discovery_datagram(&mut state, datagram, core::time::Duration::from_secs(2))
            .expect("authenticated local echo");

        assert_eq!(processed.source_address, "fe80::1234");
        assert_eq!(
            processed.event,
            AutoDiscoveryEvent::LocalMulticastEcho { ifname: "eth0".to_string() }
        );
    }

    #[test]
    fn auto_process_discovery_datagram_authenticates_remote_peer() {
        let plan = build_startup_plan_from_candidates(
            &auto_iface(),
            vec![AutoInterfaceDeviceCandidate {
                ifname: "eth0".to_string(),
                ipv6_addresses: vec!["fe80::1234".to_string()],
            }],
        )
        .expect("startup plan");
        let mut state = plan.discovery_state();
        let datagram = AutoDiscoveryDatagram {
            kind: AutoDiscoverySocketKind::Unicast,
            ifname: "eth0".to_string(),
            bind_addr: "[fe80::1234]:48556".parse().expect("bind addr"),
            multicast_group_addr: None,
            source_addr: "[fe80::2222]:48556".parse().expect("source addr"),
            payload: rns_transport::iface::auto::peering_token(b"field-net", "fe80::2222").to_vec(),
        };

        let processed = plan
            .process_discovery_datagram(&mut state, datagram, core::time::Duration::from_secs(2))
            .expect("authenticated remote peer");

        assert_eq!(processed.source_address, "fe80::2222");
        assert_eq!(
            processed.event,
            AutoDiscoveryEvent::Peer(rns_transport::iface::auto::AutoPeerEvent::Added)
        );
        assert!(state.peer("fe80::2222").is_some());
    }

    #[test]
    fn auto_process_discovery_datagram_rejects_invalid_token() {
        let plan = build_startup_plan_from_candidates(
            &auto_iface(),
            vec![AutoInterfaceDeviceCandidate {
                ifname: "eth0".to_string(),
                ipv6_addresses: vec!["fe80::1234".to_string()],
            }],
        )
        .expect("startup plan");
        let mut state = plan.discovery_state();
        let datagram = AutoDiscoveryDatagram {
            kind: AutoDiscoverySocketKind::Unicast,
            ifname: "eth0".to_string(),
            bind_addr: "[fe80::1234]:48556".parse().expect("bind addr"),
            multicast_group_addr: None,
            source_addr: "[fe80::2222]:48556".parse().expect("source addr"),
            payload: vec![0; rns_transport::hash::HASH_SIZE],
        };

        let err = plan
            .process_discovery_datagram(&mut state, datagram, core::time::Duration::from_secs(2))
            .expect_err("invalid token should reject");

        assert_eq!(err, AutoDiscoveryRejectReason::InvalidToken);
        assert!(state.peer("fe80::2222").is_none());
    }
}
