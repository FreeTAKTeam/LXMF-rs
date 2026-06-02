use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::hash::Hash;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoInterfaceConfig {
    pub group_id: String,
    pub discovery_scope: AutoDiscoveryScope,
    pub multicast_address_type: MulticastAddressType,
    pub discovery_port: u16,
    pub data_port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoPeeringPacketKind {
    Multicast,
    ReverseUnicast,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoPeeringPacket {
    pub kind: AutoPeeringPacketKind,
    pub ifname: String,
    pub source_link_local_address: String,
    pub destination_address: String,
    pub destination_port: u16,
    pub token: [u8; crate::hash::HASH_SIZE],
}

impl AutoPeeringPacket {
    pub fn payload(&self) -> &[u8; crate::hash::HASH_SIZE] {
        &self.token
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoPeerDataTarget {
    pub ifname: String,
    pub peer_address: String,
    pub destination_address: String,
    pub destination_port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoDataListenerBinding {
    pub ifname: String,
    pub link_local_address: String,
    pub bind_address: String,
    pub bind_port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoDiscoveryListenerBinding {
    pub ifname: String,
    pub link_local_address: String,
    pub unicast_bind_address: String,
    pub unicast_bind_port: u16,
    pub multicast_group_address: String,
    pub multicast_bind_address: String,
    pub multicast_bind_port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoStartupPlan {
    pub discovery_listeners: Vec<AutoDiscoveryListenerBinding>,
    pub data_listeners: Vec<AutoDataListenerBinding>,
    pub peer_job_interval: core::time::Duration,
    pub initial_peering_wait: core::time::Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoRuntimeEvent {
    FinalInitCompleted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoRuntimeState {
    pub online: bool,
    pub final_init_done: bool,
    pub carrier_changed: bool,
    startup_started_at: core::time::Duration,
    initial_peering_wait: core::time::Duration,
}

impl AutoRuntimeState {
    pub fn from_startup_plan(
        plan: &AutoStartupPlan,
        startup_started_at: core::time::Duration,
    ) -> Self {
        Self {
            online: false,
            final_init_done: false,
            carrier_changed: false,
            startup_started_at,
            initial_peering_wait: plan.initial_peering_wait,
        }
    }

    pub fn advance(&mut self, now: core::time::Duration) -> Option<AutoRuntimeEvent> {
        if self.final_init_done || now < self.startup_started_at + self.initial_peering_wait {
            return None;
        }
        self.online = true;
        self.final_init_done = true;
        Some(AutoRuntimeEvent::FinalInitCompleted)
    }

    pub fn can_process_discovery_packets(&self) -> bool {
        self.final_init_done
    }

    pub fn can_process_spawned_peer_inbound(&self) -> bool {
        self.online
    }

    pub fn record_carrier_events(&mut self, events: &[AutoMulticastCarrierEvent]) -> bool {
        if events.is_empty() {
            return false;
        }
        self.carrier_changed = true;
        true
    }

    pub fn record_link_local_update(
        &mut self,
        update: Option<&AutoLinkLocalAddressUpdate>,
    ) -> bool {
        if update.is_none() {
            return false;
        }
        self.carrier_changed = true;
        true
    }

    pub fn clear_carrier_changed(&mut self) {
        self.carrier_changed = false;
    }

    pub fn detach(&mut self) {
        self.online = false;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoInterfaceTiming {
    pub peering_timeout: core::time::Duration,
    pub announce_interval: core::time::Duration,
    pub peer_job_interval: core::time::Duration,
    pub multicast_echo_timeout: core::time::Duration,
    pub reverse_peering_interval: core::time::Duration,
    pub initial_peering_wait: core::time::Duration,
    pub multi_interface_dedupe_ttl: core::time::Duration,
    pub multi_interface_dedupe_len: usize,
}

impl AutoInterfaceTiming {
    pub fn for_platform(platform: AutoInterfacePlatform) -> Self {
        let announce_interval = core::time::Duration::from_millis(1_600);
        let peering_timeout = match platform {
            AutoInterfacePlatform::Android => core::time::Duration::from_millis(27_500),
            AutoInterfacePlatform::Other
            | AutoInterfacePlatform::Darwin
            | AutoInterfacePlatform::Windows => core::time::Duration::from_secs(22),
        };
        Self {
            peering_timeout,
            announce_interval,
            peer_job_interval: core::time::Duration::from_secs(4),
            multicast_echo_timeout: core::time::Duration::from_millis(6_500),
            reverse_peering_interval: core::time::Duration::from_millis(5_200),
            initial_peering_wait: core::time::Duration::from_millis(1_920),
            multi_interface_dedupe_ttl: core::time::Duration::from_millis(750),
            multi_interface_dedupe_len: 48,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoInterfacePlatform {
    Other,
    Darwin,
    Windows,
    Android,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutoInterfaceDeviceFilter {
    pub allowed: Vec<String>,
    pub ignored: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoInterfaceDeviceCandidate {
    pub ifname: String,
    pub ipv6_addresses: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoInterfaceAdoptedDevice {
    pub ifname: String,
    pub link_local_address: String,
}

impl AutoInterfaceDeviceFilter {
    pub fn should_adopt(&self, ifname: &str, platform: AutoInterfacePlatform) -> bool {
        match platform {
            AutoInterfacePlatform::Darwin => {
                if ifname == "lo0" {
                    return false;
                }
                if matches!(ifname, "awdl0" | "llw0" | "en5") && !self.is_allowed(ifname) {
                    return false;
                }
            }
            AutoInterfacePlatform::Android => {
                if matches!(
                    ifname,
                    "dummy0"
                        | "lo"
                        | "tun0"
                        | "rmnet0"
                        | "rmnet1"
                        | "rmnet2"
                        | "rmnet3"
                        | "rmnet4"
                        | "rmnet5"
                        | "rmnet6"
                        | "rmnet7"
                ) && !self.is_allowed(ifname)
                {
                    return false;
                }
            }
            AutoInterfacePlatform::Other | AutoInterfacePlatform::Windows => {}
        }
        if self.is_ignored(ifname) {
            return false;
        }
        if ifname == "lo0" {
            return false;
        }
        self.allowed.is_empty() || self.is_allowed(ifname)
    }

    pub fn adopt_devices(
        &self,
        candidates: &[AutoInterfaceDeviceCandidate],
        platform: AutoInterfacePlatform,
    ) -> Vec<AutoInterfaceAdoptedDevice> {
        candidates
            .iter()
            .filter(|candidate| self.should_adopt(&candidate.ifname, platform))
            .filter_map(|candidate| {
                let link_local_address = candidate
                    .ipv6_addresses
                    .iter()
                    .rev()
                    .find(|address| address.starts_with("fe80:"))
                    .map(|address| descope_link_local(address))?;
                Some(AutoInterfaceAdoptedDevice {
                    ifname: candidate.ifname.clone(),
                    link_local_address,
                })
            })
            .collect()
    }

    fn is_allowed(&self, ifname: &str) -> bool {
        self.allowed.iter().any(|allowed| allowed == ifname)
    }

    fn is_ignored(&self, ifname: &str) -> bool {
        self.ignored.iter().any(|ignored| ignored == ifname)
    }
}

impl Default for AutoInterfaceConfig {
    fn default() -> Self {
        Self {
            group_id: "reticulum".to_string(),
            discovery_scope: AutoDiscoveryScope::Link,
            multicast_address_type: MulticastAddressType::Temporary,
            discovery_port: 29_716,
            data_port: 42_671,
        }
    }
}

impl AutoInterfaceConfig {
    pub fn multicast_discovery_address(&self) -> String {
        multicast_discovery_address(
            self.group_id.as_bytes(),
            self.discovery_scope,
            self.multicast_address_type,
        )
    }

    pub fn unicast_discovery_port(&self) -> u16 {
        self.discovery_port + 1
    }

    pub fn multicast_peering_packet(
        &self,
        adopted: &AutoInterfaceAdoptedDevice,
    ) -> AutoPeeringPacket {
        let source_link_local_address = descope_link_local(&adopted.link_local_address);
        AutoPeeringPacket {
            kind: AutoPeeringPacketKind::Multicast,
            ifname: adopted.ifname.clone(),
            destination_address: self.multicast_discovery_address(),
            destination_port: self.discovery_port,
            token: peering_token(self.group_id.as_bytes(), &source_link_local_address),
            source_link_local_address,
        }
    }

    pub fn reverse_peering_packet(
        &self,
        adopted: &AutoInterfaceAdoptedDevice,
        peer_address: &str,
    ) -> AutoPeeringPacket {
        let source_link_local_address = descope_link_local(&adopted.link_local_address);
        let peer_address = descope_link_local(peer_address);
        AutoPeeringPacket {
            kind: AutoPeeringPacketKind::ReverseUnicast,
            ifname: adopted.ifname.clone(),
            destination_address: format!("{peer_address}%{}", adopted.ifname),
            destination_port: self.unicast_discovery_port(),
            token: peering_token(self.group_id.as_bytes(), &source_link_local_address),
            source_link_local_address,
        }
    }

    pub fn peer_data_target(&self, peer: &AutoPeer) -> AutoPeerDataTarget {
        let peer_address = descope_link_local(&peer.address);
        AutoPeerDataTarget {
            ifname: peer.ifname.clone(),
            destination_address: format!("{peer_address}%{}", peer.ifname),
            destination_port: self.data_port,
            peer_address,
        }
    }

    pub fn data_listener_binding(
        &self,
        adopted: &AutoInterfaceAdoptedDevice,
    ) -> AutoDataListenerBinding {
        let link_local_address = descope_link_local(&adopted.link_local_address);
        AutoDataListenerBinding {
            ifname: adopted.ifname.clone(),
            bind_address: format!("{link_local_address}%{}", adopted.ifname),
            bind_port: self.data_port,
            link_local_address,
        }
    }

    pub fn data_listener_bindings(
        &self,
        adopted_devices: &[AutoInterfaceAdoptedDevice],
    ) -> Vec<AutoDataListenerBinding> {
        adopted_devices.iter().map(|adopted| self.data_listener_binding(adopted)).collect()
    }

    pub fn discovery_listener_binding(
        &self,
        adopted: &AutoInterfaceAdoptedDevice,
        platform: AutoInterfacePlatform,
    ) -> AutoDiscoveryListenerBinding {
        let link_local_address = descope_link_local(&adopted.link_local_address);
        let multicast_group_address = self.multicast_discovery_address();
        let (unicast_bind_address, multicast_bind_address) = match platform {
            AutoInterfacePlatform::Windows => (String::new(), String::new()),
            AutoInterfacePlatform::Other
            | AutoInterfacePlatform::Darwin
            | AutoInterfacePlatform::Android => {
                let multicast_bind_address = match self.discovery_scope {
                    AutoDiscoveryScope::Link => {
                        format!("{multicast_group_address}%{}", adopted.ifname)
                    }
                    AutoDiscoveryScope::Admin
                    | AutoDiscoveryScope::Site
                    | AutoDiscoveryScope::Organisation
                    | AutoDiscoveryScope::Global => multicast_group_address.clone(),
                };
                (format!("{link_local_address}%{}", adopted.ifname), multicast_bind_address)
            }
        };

        AutoDiscoveryListenerBinding {
            ifname: adopted.ifname.clone(),
            link_local_address,
            unicast_bind_address,
            unicast_bind_port: self.unicast_discovery_port(),
            multicast_group_address,
            multicast_bind_address,
            multicast_bind_port: self.discovery_port,
        }
    }

    pub fn startup_plan(
        &self,
        adopted_devices: &[AutoInterfaceAdoptedDevice],
        platform: AutoInterfacePlatform,
        timing: AutoInterfaceTiming,
    ) -> AutoStartupPlan {
        AutoStartupPlan {
            discovery_listeners: adopted_devices
                .iter()
                .map(|adopted| self.discovery_listener_binding(adopted, platform))
                .collect(),
            data_listeners: self.data_listener_bindings(adopted_devices),
            peer_job_interval: timing.peer_job_interval,
            initial_peering_wait: timing.initial_peering_wait,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoDiscoveryScope {
    Link,
    Admin,
    Site,
    Organisation,
    Global,
}

impl AutoDiscoveryScope {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "link" => Some(Self::Link),
            "admin" => Some(Self::Admin),
            "site" => Some(Self::Site),
            "organisation" | "organization" => Some(Self::Organisation),
            "global" => Some(Self::Global),
            _ => None,
        }
    }

    fn code(self) -> char {
        match self {
            Self::Link => '2',
            Self::Admin => '4',
            Self::Site => '5',
            Self::Organisation => '8',
            Self::Global => 'e',
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MulticastAddressType {
    Permanent,
    Temporary,
}

impl MulticastAddressType {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "permanent" => Some(Self::Permanent),
            "temporary" => Some(Self::Temporary),
            _ => None,
        }
    }

    fn code(self) -> char {
        match self {
            Self::Permanent => '0',
            Self::Temporary => '1',
        }
    }
}

pub fn multicast_discovery_address(
    group_id: &[u8],
    discovery_scope: AutoDiscoveryScope,
    multicast_address_type: MulticastAddressType,
) -> String {
    let group_hash = Hash::new_from_slice(group_id);
    let g = group_hash.as_slice();
    let mut address = format!("ff{}{}:0", multicast_address_type.code(), discovery_scope.code());
    for i in (2..14).step_by(2) {
        let segment = u16::from_be_bytes([g[i], g[i + 1]]);
        address.push(':');
        address.push_str(&format!("{segment:x}"));
    }
    address
}

pub fn peering_token(group_id: &[u8], link_local_address: &str) -> [u8; 32] {
    let address = descope_link_local(link_local_address);
    let mut seed = Vec::with_capacity(group_id.len() + address.len());
    seed.extend_from_slice(group_id);
    seed.extend_from_slice(address.as_bytes());
    Hash::new_from_slice(&seed).to_bytes()
}

pub fn verify_peering_token(token: &[u8], group_id: &[u8], source_address: &str) -> bool {
    token.get(..crate::hash::HASH_SIZE) == Some(peering_token(group_id, source_address).as_slice())
}

pub fn descope_link_local(address: &str) -> String {
    let without_zone = address.split_once('%').map_or(address, |(addr, _)| addr);
    if !without_zone.starts_with("fe80:") || without_zone.starts_with("fe80::") {
        return without_zone.to_string();
    }
    if let Some(rest) = without_zone.strip_prefix("fe80:").and_then(|rest| rest.split_once("::")) {
        return format!("fe80::{}", rest.1);
    }
    without_zone.to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoPeer {
    pub address: String,
    pub ifname: String,
    pub last_heard_at: core::time::Duration,
    pub last_outbound_at: core::time::Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoPeerEvent {
    Added,
    Refreshed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoPeerInboundDecision {
    Accepted { peer: AutoPeer },
    Duplicate,
    UnknownPeer,
}

#[derive(Debug, Clone)]
pub struct AutoPeerTable {
    peers: BTreeMap<String, AutoPeer>,
    peering_timeout: core::time::Duration,
    reverse_peering_interval: core::time::Duration,
}

impl AutoPeerTable {
    pub fn new(
        peering_timeout: core::time::Duration,
        reverse_peering_interval: core::time::Duration,
    ) -> Self {
        Self { peers: BTreeMap::new(), peering_timeout, reverse_peering_interval }
    }

    pub fn observe_peer(
        &mut self,
        address: &str,
        ifname: &str,
        now: core::time::Duration,
    ) -> AutoPeerEvent {
        let address = descope_link_local(address);
        if let Some(peer) = self.peers.get_mut(&address) {
            peer.last_heard_at = now;
            return AutoPeerEvent::Refreshed;
        }

        self.peers.insert(
            address.clone(),
            AutoPeer {
                address,
                ifname: ifname.to_string(),
                last_heard_at: now,
                last_outbound_at: now,
            },
        );
        AutoPeerEvent::Added
    }

    pub fn peer(&self, address: &str) -> Option<&AutoPeer> {
        self.peers.get(&descope_link_local(address))
    }

    pub fn refresh_peer(&mut self, address: &str, now: core::time::Duration) -> Option<AutoPeer> {
        let peer = self.peers.get_mut(&descope_link_local(address))?;
        peer.last_heard_at = now;
        Some(peer.clone())
    }

    pub fn len(&self) -> usize {
        self.peers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    pub fn expire_stale(&mut self, now: core::time::Duration) -> Vec<AutoPeer> {
        let stale = self
            .peers
            .iter()
            .filter_map(|(address, peer)| {
                (now > peer.last_heard_at + self.peering_timeout).then_some(address.clone())
            })
            .collect::<Vec<_>>();
        stale.into_iter().filter_map(|address| self.peers.remove(&address)).collect()
    }

    pub fn stale_peers(&self, now: core::time::Duration) -> Vec<AutoPeer> {
        self.peers
            .values()
            .filter(|peer| now > peer.last_heard_at + self.peering_timeout)
            .cloned()
            .collect()
    }

    pub fn reverse_announces_due(&self, now: core::time::Duration) -> Vec<AutoPeer> {
        self.peers
            .values()
            .filter(|peer| now > peer.last_outbound_at + self.reverse_peering_interval)
            .cloned()
            .collect()
    }

    pub fn mark_reverse_announced(&mut self, address: &str, now: core::time::Duration) -> bool {
        let Some(peer) = self.peers.get_mut(&descope_link_local(address)) else {
            return false;
        };
        peer.last_outbound_at = now;
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoDiscoveryEvent {
    LocalMulticastEcho { ifname: String },
    Peer(AutoPeerEvent),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoDiscoveryRejectReason {
    InvalidToken,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoMulticastCarrierEvent {
    CarrierLost { ifname: String },
    CarrierRecovered { ifname: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoLinkLocalAddressUpdate {
    pub ifname: String,
    pub old_link_local_address: String,
    pub new_link_local_address: String,
    pub listener_binding: AutoDataListenerBinding,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoPeerJobPlan {
    pub expired_peers: Vec<AutoPeer>,
    pub reverse_peering_packets: Vec<AutoPeeringPacket>,
    pub missing_initial_echo_interfaces: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoPeerJobRun {
    pub expired_peers: Vec<AutoPeer>,
    pub reverse_peering_packets: Vec<AutoPeeringPacket>,
    pub missing_initial_echo_interfaces: Vec<String>,
    pub carrier_events: Vec<AutoMulticastCarrierEvent>,
}

#[derive(Debug, Clone)]
pub struct AutoDiscoveryState {
    adopted_devices: BTreeMap<String, String>,
    peers: AutoPeerTable,
    multicast_echoes: BTreeMap<String, core::time::Duration>,
    initial_echoes: BTreeMap<String, core::time::Duration>,
    last_multicast_announces: BTreeMap<String, core::time::Duration>,
    timed_out_interfaces: BTreeMap<String, bool>,
}

impl AutoDiscoveryState {
    pub fn new(
        adopted_devices: Vec<AutoInterfaceAdoptedDevice>,
        peering_timeout: core::time::Duration,
        reverse_peering_interval: core::time::Duration,
    ) -> Self {
        Self {
            adopted_devices: adopted_devices
                .into_iter()
                .map(|device| (device.ifname, descope_link_local(&device.link_local_address)))
                .collect(),
            peers: AutoPeerTable::new(peering_timeout, reverse_peering_interval),
            multicast_echoes: BTreeMap::new(),
            initial_echoes: BTreeMap::new(),
            last_multicast_announces: BTreeMap::new(),
            timed_out_interfaces: BTreeMap::new(),
        }
    }

    pub fn from_timing(
        adopted_devices: Vec<AutoInterfaceAdoptedDevice>,
        timing: AutoInterfaceTiming,
    ) -> Self {
        Self::new(adopted_devices, timing.peering_timeout, timing.reverse_peering_interval)
    }

    pub fn observe_discovery_packet(
        &mut self,
        source_address: &str,
        ifname: &str,
        now: core::time::Duration,
    ) -> AutoDiscoveryEvent {
        let source_address = descope_link_local(source_address);
        if let Some((echo_ifname, _)) = self
            .adopted_devices
            .iter()
            .find(|(_, link_local_address)| **link_local_address == source_address)
        {
            self.multicast_echoes.insert(echo_ifname.clone(), now);
            self.initial_echoes.entry(echo_ifname.clone()).or_insert(now);
            return AutoDiscoveryEvent::LocalMulticastEcho { ifname: echo_ifname.clone() };
        }

        AutoDiscoveryEvent::Peer(self.peers.observe_peer(&source_address, ifname, now))
    }

    pub fn observe_authenticated_discovery_packet(
        &mut self,
        packet: &[u8],
        group_id: &[u8],
        source_address: &str,
        ifname: &str,
        now: core::time::Duration,
    ) -> Result<AutoDiscoveryEvent, AutoDiscoveryRejectReason> {
        if !verify_peering_token(packet, group_id, source_address) {
            return Err(AutoDiscoveryRejectReason::InvalidToken);
        }
        Ok(self.observe_discovery_packet(source_address, ifname, now))
    }

    pub fn peer(&self, address: &str) -> Option<&AutoPeer> {
        self.peers.peer(address)
    }

    pub fn handle_spawned_peer_inbound(
        &mut self,
        dedupe: &mut AutoInboundPacketDeduplicator,
        peer_address: &str,
        packet: &[u8],
        now: core::time::Duration,
    ) -> AutoPeerInboundDecision {
        if self.peer(peer_address).is_none() {
            return AutoPeerInboundDecision::UnknownPeer;
        }
        if !dedupe.should_accept(packet, now) {
            return AutoPeerInboundDecision::Duplicate;
        }
        let peer = self
            .peers
            .refresh_peer(peer_address, now)
            .expect("known peer should refresh after dedupe accept");
        AutoPeerInboundDecision::Accepted { peer }
    }

    pub fn update_adopted_link_local_address(
        &mut self,
        config: &AutoInterfaceConfig,
        ifname: &str,
        link_local_address: &str,
    ) -> Option<AutoLinkLocalAddressUpdate> {
        let new_link_local_address = descope_link_local(link_local_address);
        let old_link_local_address = self.adopted_devices.get(ifname)?.clone();
        if old_link_local_address == new_link_local_address {
            return None;
        }

        self.adopted_devices.insert(ifname.to_string(), new_link_local_address.clone());
        let adopted = AutoInterfaceAdoptedDevice {
            ifname: ifname.to_string(),
            link_local_address: new_link_local_address.clone(),
        };

        Some(AutoLinkLocalAddressUpdate {
            ifname: ifname.to_string(),
            old_link_local_address,
            new_link_local_address,
            listener_binding: config.data_listener_binding(&adopted),
        })
    }

    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    pub fn last_multicast_echo(&self, ifname: &str) -> Option<core::time::Duration> {
        self.multicast_echoes.get(ifname).copied()
    }

    pub fn initial_multicast_echo(&self, ifname: &str) -> Option<core::time::Duration> {
        self.initial_echoes.get(ifname).copied()
    }

    pub fn missing_initial_multicast_echoes(&self) -> Vec<String> {
        self.adopted_devices
            .keys()
            .filter(|ifname| !self.initial_echoes.contains_key(*ifname))
            .cloned()
            .collect()
    }

    pub fn peer_job_plan(
        &self,
        config: &AutoInterfaceConfig,
        adopted_devices: &[AutoInterfaceAdoptedDevice],
        now: core::time::Duration,
    ) -> AutoPeerJobPlan {
        let expired_peers = self.peers.stale_peers(now);
        let reverse_peering_packets = self
            .peers
            .reverse_announces_due(now)
            .into_iter()
            .filter(|peer| !expired_peers.iter().any(|expired| expired.address == peer.address))
            .filter_map(|peer| {
                let adopted =
                    adopted_devices.iter().find(|adopted| adopted.ifname == peer.ifname)?;
                Some(config.reverse_peering_packet(adopted, &peer.address))
            })
            .collect();

        AutoPeerJobPlan {
            expired_peers,
            reverse_peering_packets,
            missing_initial_echo_interfaces: self.missing_initial_multicast_echoes(),
        }
    }

    pub fn run_peer_job(
        &mut self,
        config: &AutoInterfaceConfig,
        adopted_devices: &[AutoInterfaceAdoptedDevice],
        now: core::time::Duration,
        multicast_echo_timeout: core::time::Duration,
    ) -> AutoPeerJobRun {
        let expired_peers = self.expire_stale_peers(now);
        let reverse_peering_packets = self
            .peers
            .reverse_announces_due(now)
            .into_iter()
            .filter_map(|peer| {
                let adopted =
                    adopted_devices.iter().find(|adopted| adopted.ifname == peer.ifname)?;
                let packet = config.reverse_peering_packet(adopted, &peer.address);
                self.peers.mark_reverse_announced(&peer.address, now);
                Some(packet)
            })
            .collect();
        let missing_initial_echo_interfaces = self.missing_initial_multicast_echoes();
        let carrier_events = self.update_multicast_echo_timeouts(now, multicast_echo_timeout);

        AutoPeerJobRun {
            expired_peers,
            reverse_peering_packets,
            missing_initial_echo_interfaces,
            carrier_events,
        }
    }

    pub fn run_multicast_announce_job(
        &mut self,
        config: &AutoInterfaceConfig,
        adopted_devices: &[AutoInterfaceAdoptedDevice],
        now: core::time::Duration,
        announce_interval: core::time::Duration,
    ) -> Vec<AutoPeeringPacket> {
        let mut packets = Vec::new();
        for adopted in adopted_devices {
            let due = match self.last_multicast_announces.get(&adopted.ifname) {
                Some(last_announce) => now >= *last_announce + announce_interval,
                None => true,
            };
            if due {
                packets.push(config.multicast_peering_packet(adopted));
                self.last_multicast_announces.insert(adopted.ifname.clone(), now);
            }
        }
        packets
    }

    pub fn update_multicast_echo_timeouts(
        &mut self,
        now: core::time::Duration,
        multicast_echo_timeout: core::time::Duration,
    ) -> Vec<AutoMulticastCarrierEvent> {
        let mut events = Vec::new();
        for ifname in self.adopted_devices.keys() {
            let last_echo = self
                .multicast_echoes
                .get(ifname)
                .copied()
                .unwrap_or_else(|| core::time::Duration::from_secs(0));
            let timed_out = now > last_echo + multicast_echo_timeout;
            match (timed_out, self.timed_out_interfaces.get(ifname).copied()) {
                (true, Some(false)) => {
                    events.push(AutoMulticastCarrierEvent::CarrierLost { ifname: ifname.clone() });
                }
                (false, Some(true)) => {
                    events.push(AutoMulticastCarrierEvent::CarrierRecovered {
                        ifname: ifname.clone(),
                    });
                }
                _ => {}
            }
            self.timed_out_interfaces.insert(ifname.clone(), timed_out);
        }
        events
    }

    pub fn multicast_echo_timed_out(&self, ifname: &str) -> Option<bool> {
        self.timed_out_interfaces.get(ifname).copied()
    }

    pub fn expire_stale_peers(&mut self, now: core::time::Duration) -> Vec<AutoPeer> {
        self.peers.expire_stale(now)
    }

    pub fn reverse_announces_due(&self, now: core::time::Duration) -> Vec<AutoPeer> {
        self.peers.reverse_announces_due(now)
    }

    pub fn mark_reverse_announced(&mut self, address: &str, now: core::time::Duration) -> bool {
        self.peers.mark_reverse_announced(address, now)
    }
}

#[derive(Debug, Clone)]
pub struct AutoInboundPacketDeduplicator {
    entries: VecDeque<(Hash, core::time::Duration)>,
    capacity: usize,
    ttl: core::time::Duration,
}

impl AutoInboundPacketDeduplicator {
    pub fn new(capacity: usize, ttl: core::time::Duration) -> Self {
        Self { entries: VecDeque::with_capacity(capacity), capacity, ttl }
    }

    pub fn from_timing(timing: AutoInterfaceTiming) -> Self {
        Self::new(timing.multi_interface_dedupe_len, timing.multi_interface_dedupe_ttl)
    }

    pub fn should_accept(&mut self, packet: &[u8], now: core::time::Duration) -> bool {
        let packet_hash = Hash::new_from_slice(packet);
        if self.entries.iter().any(|(entry_hash, entry_time)| {
            *entry_hash == packet_hash && now < *entry_time + self.ttl
        }) {
            return false;
        }

        if self.capacity == 0 {
            return true;
        }
        if self.entries.len() == self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back((packet_hash, now));
        true
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        descope_link_local, peering_token, verify_peering_token, AutoDataListenerBinding,
        AutoDiscoveryEvent, AutoDiscoveryRejectReason, AutoDiscoveryScope, AutoDiscoveryState,
        AutoInboundPacketDeduplicator, AutoInterfaceAdoptedDevice, AutoInterfaceConfig,
        AutoInterfaceDeviceCandidate, AutoInterfaceDeviceFilter, AutoInterfacePlatform,
        AutoInterfaceTiming, AutoLinkLocalAddressUpdate, AutoMulticastCarrierEvent, AutoPeer,
        AutoPeerEvent, AutoPeerInboundDecision, AutoPeerTable, AutoPeeringPacketKind,
        AutoRuntimeEvent, AutoRuntimeState, MulticastAddressType,
    };

    #[test]
    fn default_multicast_discovery_address_matches_python_auto_interface() {
        let config = AutoInterfaceConfig::default();

        assert_eq!(config.multicast_discovery_address(), "ff12:0:d70b:fb1c:16e4:5e39:485e:31e1");
    }

    #[test]
    fn custom_multicast_discovery_address_matches_python_auto_interface() {
        let config = AutoInterfaceConfig {
            group_id: "field-net".to_string(),
            discovery_scope: AutoDiscoveryScope::Global,
            multicast_address_type: MulticastAddressType::Permanent,
            discovery_port: 48_555,
            data_port: 49_555,
        };

        assert_eq!(config.multicast_discovery_address(), "ff0e:0:77b9:4bfd:9488:364b:4bbe:119d");
    }

    #[test]
    fn multicast_peering_packet_matches_python_peer_announce() {
        let config = AutoInterfaceConfig::default();
        let adopted = AutoInterfaceAdoptedDevice {
            ifname: "eth0".to_string(),
            link_local_address: "fe80::1111%eth0".to_string(),
        };

        let packet = config.multicast_peering_packet(&adopted);

        assert_eq!(packet.kind, AutoPeeringPacketKind::Multicast);
        assert_eq!(packet.ifname, "eth0");
        assert_eq!(packet.source_link_local_address, "fe80::1111");
        assert_eq!(packet.destination_address, "ff12:0:d70b:fb1c:16e4:5e39:485e:31e1");
        assert_eq!(packet.destination_port, 29_716);
        assert_eq!(packet.token, peering_token(b"reticulum", "fe80::1111"));
    }

    #[test]
    fn multicast_announce_job_sends_immediately_like_python_announce_handler() {
        let config = AutoInterfaceConfig::default();
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);
        let adopted_devices = vec![
            AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1111%eth0".to_string(),
            },
            AutoInterfaceAdoptedDevice {
                ifname: "wlan0".to_string(),
                link_local_address: "fe80::2222%wlan0".to_string(),
            },
        ];
        let mut state = AutoDiscoveryState::from_timing(adopted_devices.clone(), timing);

        let packets = state.run_multicast_announce_job(
            &config,
            &adopted_devices,
            core::time::Duration::ZERO,
            timing.announce_interval,
        );

        assert_eq!(packets.len(), 2);
        assert_eq!(packets[0].kind, AutoPeeringPacketKind::Multicast);
        assert_eq!(packets[0].ifname, "eth0");
        assert_eq!(packets[0].destination_port, 29_716);
        assert_eq!(packets[1].ifname, "wlan0");
    }

    #[test]
    fn multicast_announce_job_respects_python_announce_interval() {
        let config = AutoInterfaceConfig::default();
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);
        let adopted_devices = vec![AutoInterfaceAdoptedDevice {
            ifname: "eth0".to_string(),
            link_local_address: "fe80::1111".to_string(),
        }];
        let mut state = AutoDiscoveryState::from_timing(adopted_devices.clone(), timing);

        assert_eq!(
            state
                .run_multicast_announce_job(
                    &config,
                    &adopted_devices,
                    core::time::Duration::ZERO,
                    timing.announce_interval,
                )
                .len(),
            1
        );
        assert!(state
            .run_multicast_announce_job(
                &config,
                &adopted_devices,
                core::time::Duration::from_millis(1_599),
                timing.announce_interval,
            )
            .is_empty());

        let packets = state.run_multicast_announce_job(
            &config,
            &adopted_devices,
            core::time::Duration::from_millis(1_600),
            timing.announce_interval,
        );

        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].destination_address, "ff12:0:d70b:fb1c:16e4:5e39:485e:31e1");
    }

    #[test]
    fn reverse_peering_packet_matches_python_reverse_announce() {
        let config = AutoInterfaceConfig::default();
        let adopted = AutoInterfaceAdoptedDevice {
            ifname: "eth0".to_string(),
            link_local_address: "fe80::1111%eth0".to_string(),
        };

        let packet = config.reverse_peering_packet(&adopted, "fe80::2222");

        assert_eq!(packet.kind, AutoPeeringPacketKind::ReverseUnicast);
        assert_eq!(packet.ifname, "eth0");
        assert_eq!(packet.source_link_local_address, "fe80::1111");
        assert_eq!(packet.destination_address, "fe80::2222%eth0");
        assert_eq!(packet.destination_port, 29_717);
        assert_eq!(packet.token, peering_token(b"reticulum", "fe80::1111"));
    }

    #[test]
    fn peer_data_target_matches_python_spawned_peer_delivery() {
        let config = AutoInterfaceConfig::default();
        let peer = AutoPeer {
            address: "fe80::2222%ignored".to_string(),
            ifname: "eth0".to_string(),
            last_heard_at: core::time::Duration::from_secs(1),
            last_outbound_at: core::time::Duration::from_secs(1),
        };

        let target = config.peer_data_target(&peer);

        assert_eq!(target.ifname, "eth0");
        assert_eq!(target.peer_address, "fe80::2222");
        assert_eq!(target.destination_address, "fe80::2222%eth0");
        assert_eq!(target.destination_port, 42_671);
    }

    #[test]
    fn peer_data_target_uses_configured_data_port() {
        let config = AutoInterfaceConfig { data_port: 49_555, ..AutoInterfaceConfig::default() };
        let peer = AutoPeer {
            address: "fe80::3333".to_string(),
            ifname: "wlan0".to_string(),
            last_heard_at: core::time::Duration::from_secs(1),
            last_outbound_at: core::time::Duration::from_secs(1),
        };

        let target = config.peer_data_target(&peer);

        assert_eq!(target.destination_address, "fe80::3333%wlan0");
        assert_eq!(target.destination_port, 49_555);
    }

    #[test]
    fn data_listener_binding_matches_python_final_init_udp_server_target() {
        let config = AutoInterfaceConfig::default();
        let adopted = AutoInterfaceAdoptedDevice {
            ifname: "eth0".to_string(),
            link_local_address: "fe80::1111%ignored".to_string(),
        };

        let binding = config.data_listener_binding(&adopted);

        assert_eq!(binding.ifname, "eth0");
        assert_eq!(binding.link_local_address, "fe80::1111");
        assert_eq!(binding.bind_address, "fe80::1111%eth0");
        assert_eq!(binding.bind_port, 42_671);
    }

    #[test]
    fn data_listener_bindings_use_configured_data_port_and_preserve_adopted_order() {
        let config = AutoInterfaceConfig { data_port: 49_555, ..AutoInterfaceConfig::default() };
        let adopted = vec![
            AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1111".to_string(),
            },
            AutoInterfaceAdoptedDevice {
                ifname: "wlan0".to_string(),
                link_local_address: "fe80::2222%wlan0".to_string(),
            },
        ];

        let bindings = config.data_listener_bindings(&adopted);

        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].bind_address, "fe80::1111%eth0");
        assert_eq!(bindings[0].bind_port, 49_555);
        assert_eq!(bindings[1].bind_address, "fe80::2222%wlan0");
        assert_eq!(bindings[1].bind_port, 49_555);
    }

    #[test]
    fn discovery_listener_binding_matches_python_non_windows_startup_targets() {
        let config = AutoInterfaceConfig::default();
        let adopted = AutoInterfaceAdoptedDevice {
            ifname: "eth0".to_string(),
            link_local_address: "fe80::1111%ignored".to_string(),
        };

        let binding = config.discovery_listener_binding(&adopted, AutoInterfacePlatform::Other);

        assert_eq!(binding.ifname, "eth0");
        assert_eq!(binding.link_local_address, "fe80::1111");
        assert_eq!(binding.unicast_bind_address, "fe80::1111%eth0");
        assert_eq!(binding.unicast_bind_port, 29_717);
        assert_eq!(binding.multicast_group_address, "ff12:0:d70b:fb1c:16e4:5e39:485e:31e1");
        assert_eq!(binding.multicast_bind_address, "ff12:0:d70b:fb1c:16e4:5e39:485e:31e1%eth0");
        assert_eq!(binding.multicast_bind_port, 29_716);
    }

    #[test]
    fn discovery_listener_binding_matches_python_global_scope_and_windows_bind_targets() {
        let global_config = AutoInterfaceConfig {
            discovery_scope: AutoDiscoveryScope::Global,
            ..AutoInterfaceConfig::default()
        };
        let adopted = AutoInterfaceAdoptedDevice {
            ifname: "eth0".to_string(),
            link_local_address: "fe80::1111".to_string(),
        };

        let global =
            global_config.discovery_listener_binding(&adopted, AutoInterfacePlatform::Other);
        assert_eq!(global.multicast_bind_address, "ff1e:0:d70b:fb1c:16e4:5e39:485e:31e1");

        let windows = AutoInterfaceConfig::default()
            .discovery_listener_binding(&adopted, AutoInterfacePlatform::Windows);
        assert_eq!(windows.unicast_bind_address, "");
        assert_eq!(windows.unicast_bind_port, 29_717);
        assert_eq!(windows.multicast_bind_address, "");
        assert_eq!(windows.multicast_bind_port, 29_716);
        assert_eq!(windows.multicast_group_address, "ff12:0:d70b:fb1c:16e4:5e39:485e:31e1");
    }

    #[test]
    fn startup_plan_aggregates_python_final_init_runtime_targets() {
        let config = AutoInterfaceConfig::default();
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);
        let adopted = vec![
            AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1111%eth0".to_string(),
            },
            AutoInterfaceAdoptedDevice {
                ifname: "wlan0".to_string(),
                link_local_address: "fe80::2222".to_string(),
            },
        ];

        let plan = config.startup_plan(&adopted, AutoInterfacePlatform::Other, timing);

        assert_eq!(plan.initial_peering_wait, core::time::Duration::from_millis(1_920));
        assert_eq!(plan.peer_job_interval, core::time::Duration::from_secs(4));
        assert_eq!(plan.discovery_listeners.len(), 2);
        assert_eq!(plan.discovery_listeners[0].ifname, "eth0");
        assert_eq!(plan.discovery_listeners[1].unicast_bind_address, "fe80::2222%wlan0");
        assert_eq!(plan.data_listeners.len(), 2);
        assert_eq!(plan.data_listeners[0].bind_address, "fe80::1111%eth0");
        assert_eq!(plan.data_listeners[1].bind_address, "fe80::2222%wlan0");
    }

    #[test]
    fn startup_plan_carries_windows_discovery_bindings_but_normal_data_listeners() {
        let config = AutoInterfaceConfig::default();
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Windows);
        let adopted = vec![AutoInterfaceAdoptedDevice {
            ifname: "eth0".to_string(),
            link_local_address: "fe80::1111".to_string(),
        }];

        let plan = config.startup_plan(&adopted, AutoInterfacePlatform::Windows, timing);

        assert_eq!(plan.discovery_listeners[0].unicast_bind_address, "");
        assert_eq!(plan.discovery_listeners[0].multicast_bind_address, "");
        assert_eq!(plan.data_listeners[0].bind_address, "fe80::1111%eth0");
        assert_eq!(plan.initial_peering_wait, core::time::Duration::from_millis(1_920));
    }

    #[test]
    fn runtime_state_gates_discovery_until_python_final_init_wait_completes() {
        let config = AutoInterfaceConfig::default();
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);
        let plan = config.startup_plan(&[], AutoInterfacePlatform::Other, timing);
        let mut runtime = AutoRuntimeState::from_startup_plan(&plan, core::time::Duration::ZERO);

        assert!(!runtime.online);
        assert!(!runtime.final_init_done);
        assert!(!runtime.can_process_discovery_packets());
        assert_eq!(runtime.advance(core::time::Duration::from_millis(1_919)), None);
        assert!(!runtime.can_process_discovery_packets());

        assert_eq!(
            runtime.advance(core::time::Duration::from_millis(1_920)),
            Some(AutoRuntimeEvent::FinalInitCompleted)
        );
        assert!(runtime.online);
        assert!(runtime.final_init_done);
        assert!(runtime.can_process_discovery_packets());
        assert_eq!(runtime.advance(core::time::Duration::from_millis(3_000)), None);
    }

    #[test]
    fn runtime_state_gates_spawned_peer_inbound_on_online_state_like_python() {
        let config = AutoInterfaceConfig::default();
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);
        let plan = config.startup_plan(&[], AutoInterfacePlatform::Other, timing);
        let mut runtime = AutoRuntimeState::from_startup_plan(&plan, core::time::Duration::ZERO);

        assert!(!runtime.can_process_spawned_peer_inbound());
        runtime.advance(core::time::Duration::from_millis(1_920));
        assert!(runtime.can_process_spawned_peer_inbound());

        runtime.detach();

        assert!(!runtime.online);
        assert!(runtime.final_init_done);
        assert!(!runtime.can_process_spawned_peer_inbound());
        assert!(runtime.can_process_discovery_packets());
    }

    #[test]
    fn runtime_state_records_multicast_carrier_transitions_like_python() {
        let config = AutoInterfaceConfig::default();
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);
        let plan = config.startup_plan(&[], AutoInterfacePlatform::Other, timing);
        let mut runtime = AutoRuntimeState::from_startup_plan(&plan, core::time::Duration::ZERO);

        assert!(!runtime.carrier_changed);
        assert!(!runtime.record_carrier_events(&[]));
        assert!(!runtime.carrier_changed);

        assert!(runtime.record_carrier_events(&[AutoMulticastCarrierEvent::CarrierLost {
            ifname: "eth0".to_string(),
        }]));
        assert!(runtime.carrier_changed);

        runtime.clear_carrier_changed();

        assert!(!runtime.carrier_changed);
    }

    #[test]
    fn runtime_state_records_link_local_replacement_as_carrier_change_like_python() {
        let config = AutoInterfaceConfig::default();
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);
        let plan = config.startup_plan(&[], AutoInterfacePlatform::Other, timing);
        let mut runtime = AutoRuntimeState::from_startup_plan(&plan, core::time::Duration::ZERO);
        let update = AutoLinkLocalAddressUpdate {
            ifname: "eth0".to_string(),
            old_link_local_address: "fe80::1111".to_string(),
            new_link_local_address: "fe80::2222".to_string(),
            listener_binding: AutoDataListenerBinding {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::2222".to_string(),
                bind_address: "fe80::2222%eth0".to_string(),
                bind_port: config.data_port,
            },
        };

        assert!(!runtime.carrier_changed);
        assert!(runtime.record_link_local_update(Some(&update)));
        assert!(runtime.carrier_changed);

        runtime.clear_carrier_changed();

        assert!(!runtime.record_link_local_update(None));
        assert!(!runtime.carrier_changed);
    }

    #[test]
    fn link_local_update_replaces_adopted_address_and_plans_listener_restart_like_python() {
        let config = AutoInterfaceConfig::default();
        let mut state = AutoDiscoveryState::from_timing(
            vec![AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1111".to_string(),
            }],
            AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other),
        );

        let update = state.update_adopted_link_local_address(&config, "eth0", "fe80::2222%eth0");

        assert_eq!(
            update,
            Some(AutoLinkLocalAddressUpdate {
                ifname: "eth0".to_string(),
                old_link_local_address: "fe80::1111".to_string(),
                new_link_local_address: "fe80::2222".to_string(),
                listener_binding: AutoDataListenerBinding {
                    ifname: "eth0".to_string(),
                    link_local_address: "fe80::2222".to_string(),
                    bind_address: "fe80::2222%eth0".to_string(),
                    bind_port: 42_671,
                },
            })
        );
        assert_eq!(
            state.observe_discovery_packet(
                "fe80::2222",
                "eth0",
                core::time::Duration::from_secs(3),
            ),
            AutoDiscoveryEvent::LocalMulticastEcho { ifname: "eth0".to_string() }
        );
        assert_eq!(
            state.observe_discovery_packet(
                "fe80::1111",
                "eth0",
                core::time::Duration::from_secs(4),
            ),
            AutoDiscoveryEvent::Peer(AutoPeerEvent::Added)
        );
    }

    #[test]
    fn link_local_update_is_noop_for_same_or_unknown_interface() {
        let config = AutoInterfaceConfig::default();
        let mut state = AutoDiscoveryState::from_timing(
            vec![AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1111".to_string(),
            }],
            AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other),
        );

        assert_eq!(
            state.update_adopted_link_local_address(&config, "eth0", "fe80::1111%eth0"),
            None
        );
        assert_eq!(state.update_adopted_link_local_address(&config, "wlan0", "fe80::2222"), None);
    }

    #[test]
    fn peer_job_plan_matches_python_reverse_announce_and_initial_echo_checks() {
        let config = AutoInterfaceConfig::default();
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);
        let adopted_devices = vec![
            AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1111".to_string(),
            },
            AutoInterfaceAdoptedDevice {
                ifname: "wlan0".to_string(),
                link_local_address: "fe80::3333".to_string(),
            },
        ];
        let mut state = AutoDiscoveryState::from_timing(adopted_devices.clone(), timing);

        state.observe_discovery_packet("fe80::1111%eth0", "eth0", core::time::Duration::ZERO);
        state.observe_discovery_packet("fe80::2222", "eth0", core::time::Duration::ZERO);

        let plan = state.peer_job_plan(
            &config,
            &adopted_devices,
            core::time::Duration::from_millis(5_201),
        );

        assert!(plan.expired_peers.is_empty());
        assert_eq!(plan.missing_initial_echo_interfaces, vec!["wlan0"]);
        assert_eq!(plan.reverse_peering_packets.len(), 1);
        assert_eq!(plan.reverse_peering_packets[0].kind, AutoPeeringPacketKind::ReverseUnicast);
        assert_eq!(plan.reverse_peering_packets[0].destination_address, "fe80::2222%eth0");
        assert_eq!(plan.reverse_peering_packets[0].destination_port, 29_717);
    }

    #[test]
    fn peer_job_plan_expires_peers_before_reverse_announces_like_python() {
        let config = AutoInterfaceConfig::default();
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);
        let adopted_devices = vec![AutoInterfaceAdoptedDevice {
            ifname: "eth0".to_string(),
            link_local_address: "fe80::1111".to_string(),
        }];
        let mut state = AutoDiscoveryState::from_timing(adopted_devices.clone(), timing);

        state.observe_discovery_packet("fe80::2222", "eth0", core::time::Duration::ZERO);

        let plan = state.peer_job_plan(
            &config,
            &adopted_devices,
            core::time::Duration::from_millis(22_001),
        );

        assert_eq!(plan.expired_peers.len(), 1);
        assert_eq!(plan.expired_peers[0].address, "fe80::2222");
        assert!(plan.reverse_peering_packets.is_empty());
    }

    #[test]
    fn run_peer_job_marks_reverse_announced_and_updates_carrier_state_like_python() {
        let config = AutoInterfaceConfig::default();
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);
        let adopted_devices = vec![AutoInterfaceAdoptedDevice {
            ifname: "eth0".to_string(),
            link_local_address: "fe80::1111".to_string(),
        }];
        let mut state = AutoDiscoveryState::from_timing(adopted_devices.clone(), timing);

        state.observe_discovery_packet(
            "fe80::1111%eth0",
            "eth0",
            core::time::Duration::from_millis(1_000),
        );
        state.observe_discovery_packet(
            "fe80::2222%eth0",
            "eth0",
            core::time::Duration::from_millis(1_000),
        );
        let initial_run = state.run_peer_job(
            &config,
            &adopted_devices,
            core::time::Duration::from_millis(1_000),
            timing.multicast_echo_timeout,
        );
        assert!(initial_run.carrier_events.is_empty());
        assert_eq!(state.multicast_echo_timed_out("eth0"), Some(false));

        let run = state.run_peer_job(
            &config,
            &adopted_devices,
            core::time::Duration::from_millis(7_501),
            timing.multicast_echo_timeout,
        );

        assert!(run.expired_peers.is_empty());
        assert_eq!(run.reverse_peering_packets.len(), 1);
        assert_eq!(
            run.carrier_events,
            vec![AutoMulticastCarrierEvent::CarrierLost { ifname: "eth0".to_string() }]
        );
        assert!(state.reverse_announces_due(core::time::Duration::from_millis(12_701)).is_empty());
        assert_eq!(state.multicast_echo_timed_out("eth0"), Some(true));
    }

    #[test]
    fn run_peer_job_expires_stale_peers_before_marking_reverse_announces() {
        let config = AutoInterfaceConfig::default();
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);
        let adopted_devices = vec![AutoInterfaceAdoptedDevice {
            ifname: "eth0".to_string(),
            link_local_address: "fe80::1111".to_string(),
        }];
        let mut state = AutoDiscoveryState::from_timing(adopted_devices.clone(), timing);

        state.observe_discovery_packet("fe80::2222", "eth0", core::time::Duration::ZERO);

        let run = state.run_peer_job(
            &config,
            &adopted_devices,
            core::time::Duration::from_millis(22_001),
            timing.multicast_echo_timeout,
        );

        assert_eq!(run.expired_peers.len(), 1);
        assert_eq!(run.expired_peers[0].address, "fe80::2222");
        assert!(run.reverse_peering_packets.is_empty());
        assert!(state.peer("fe80::2222").is_none());
    }

    #[test]
    fn peering_token_matches_python_auto_interface() {
        let token = peering_token(b"reticulum", "fe80::1234:abcd");

        assert_eq!(
            hex::encode(token),
            "2158465c9c7ece3cc433c698231ebd4304b7f278e352c769426ade2b0ebecff0"
        );
        assert!(verify_peering_token(&token, b"reticulum", "fe80::1234:abcd"));
        assert!(!verify_peering_token(&token, b"reticulum", "fe80::beef"));
    }

    #[test]
    fn peering_token_verification_matches_python_payload_slicing() {
        let token = peering_token(b"reticulum", "fe80::1234:abcd");
        let mut payload = token.to_vec();
        payload.extend_from_slice(b"ignored suffix");

        assert!(verify_peering_token(&payload, b"reticulum", "fe80::1234:abcd"));
        assert!(!verify_peering_token(&payload[..31], b"reticulum", "fe80::1234:abcd"));
    }

    #[test]
    fn descopes_link_local_addresses_like_python_auto_interface() {
        assert_eq!(descope_link_local("fe80::1234%eth0"), "fe80::1234");
        assert_eq!(descope_link_local("fe80:abcd::1234"), "fe80::1234");
        assert_eq!(descope_link_local("2001:db8::1"), "2001:db8::1");
    }

    #[test]
    fn timing_defaults_match_python_auto_interface() {
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);

        assert_eq!(timing.peering_timeout, core::time::Duration::from_secs(22));
        assert_eq!(timing.announce_interval, core::time::Duration::from_millis(1_600));
        assert_eq!(timing.peer_job_interval, core::time::Duration::from_secs(4));
        assert_eq!(timing.multicast_echo_timeout, core::time::Duration::from_millis(6_500));
        assert_eq!(timing.reverse_peering_interval, core::time::Duration::from_millis(5_200));
        assert_eq!(timing.initial_peering_wait, core::time::Duration::from_millis(1_920));
        assert_eq!(timing.multi_interface_dedupe_ttl, core::time::Duration::from_millis(750));
        assert_eq!(timing.multi_interface_dedupe_len, 48);
    }

    #[test]
    fn timing_applies_python_android_peering_multiplier() {
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Android);

        assert_eq!(timing.peering_timeout, core::time::Duration::from_millis(27_500));
        assert_eq!(timing.reverse_peering_interval, core::time::Duration::from_millis(5_200));
    }

    #[test]
    fn discovery_state_from_timing_uses_python_peer_intervals() {
        let mut state = AutoDiscoveryState::from_timing(
            vec![AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1111".to_string(),
            }],
            AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Android),
        );
        state.observe_discovery_packet(
            "fe80::2222%eth0",
            "eth0",
            core::time::Duration::from_secs(0),
        );

        assert!(state.reverse_announces_due(core::time::Duration::from_millis(5_200)).is_empty());
        let due = state.reverse_announces_due(core::time::Duration::from_millis(5_201));
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].address, "fe80::2222");

        assert!(state.expire_stale_peers(core::time::Duration::from_millis(27_500)).is_empty());
        let expired = state.expire_stale_peers(core::time::Duration::from_millis(27_501));
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].address, "fe80::2222");
    }

    #[test]
    fn inbound_deduplicator_from_timing_uses_python_window() {
        let mut dedupe = AutoInboundPacketDeduplicator::from_timing(
            AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other),
        );

        assert!(dedupe.should_accept(b"packet", core::time::Duration::from_millis(1_000)));
        assert!(!dedupe.should_accept(b"packet", core::time::Duration::from_millis(1_749)));
        assert!(dedupe.should_accept(b"packet", core::time::Duration::from_millis(1_751)));
    }

    #[test]
    fn spawned_peer_inbound_accepts_known_peer_and_refreshes_it_like_python() {
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);
        let mut state = AutoDiscoveryState::from_timing(Vec::new(), timing);
        let mut dedupe = AutoInboundPacketDeduplicator::from_timing(timing);
        state.observe_discovery_packet(
            "fe80::2222%eth0",
            "eth0",
            core::time::Duration::from_secs(1),
        );

        let decision = state.handle_spawned_peer_inbound(
            &mut dedupe,
            "fe80::2222%eth0",
            b"packet",
            core::time::Duration::from_secs(2),
        );

        assert_eq!(
            decision,
            AutoPeerInboundDecision::Accepted {
                peer: AutoPeer {
                    address: "fe80::2222".to_string(),
                    ifname: "eth0".to_string(),
                    last_heard_at: core::time::Duration::from_secs(2),
                    last_outbound_at: core::time::Duration::from_secs(1),
                }
            }
        );
        assert_eq!(
            state.peer("fe80::2222").expect("peer").last_heard_at,
            core::time::Duration::from_secs(2)
        );
        assert_eq!(dedupe.len(), 1);
    }

    #[test]
    fn spawned_peer_inbound_suppresses_duplicate_without_refreshing_peer() {
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);
        let mut state = AutoDiscoveryState::from_timing(Vec::new(), timing);
        let mut dedupe = AutoInboundPacketDeduplicator::from_timing(timing);
        state.observe_discovery_packet("fe80::2222", "eth0", core::time::Duration::from_secs(1));

        assert!(matches!(
            state.handle_spawned_peer_inbound(
                &mut dedupe,
                "fe80::2222",
                b"packet",
                core::time::Duration::from_millis(2_000),
            ),
            AutoPeerInboundDecision::Accepted { .. }
        ));
        let duplicate = state.handle_spawned_peer_inbound(
            &mut dedupe,
            "fe80::2222",
            b"packet",
            core::time::Duration::from_millis(2_500),
        );

        assert_eq!(duplicate, AutoPeerInboundDecision::Duplicate);
        assert_eq!(
            state.peer("fe80::2222").expect("peer").last_heard_at,
            core::time::Duration::from_millis(2_000)
        );
    }

    #[test]
    fn spawned_peer_inbound_rejects_unknown_peer_without_touching_dedupe() {
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);
        let mut state = AutoDiscoveryState::from_timing(Vec::new(), timing);
        let mut dedupe = AutoInboundPacketDeduplicator::from_timing(timing);

        let decision = state.handle_spawned_peer_inbound(
            &mut dedupe,
            "fe80::4444",
            b"packet",
            core::time::Duration::from_secs(2),
        );

        assert_eq!(decision, AutoPeerInboundDecision::UnknownPeer);
        assert!(state.peer("fe80::4444").is_none());
        assert_eq!(dedupe.len(), 0);
    }

    #[test]
    fn peer_table_adds_new_peer_and_refreshes_existing_like_python() {
        let mut peers = AutoPeerTable::new(
            core::time::Duration::from_secs(22),
            core::time::Duration::from_millis(5_200),
        );

        assert_eq!(
            peers.observe_peer("fe80::1", "eth0", core::time::Duration::from_secs(10)),
            AutoPeerEvent::Added
        );
        assert_eq!(peers.len(), 1);

        assert_eq!(
            peers.observe_peer("fe80::1", "wlan0", core::time::Duration::from_secs(12)),
            AutoPeerEvent::Refreshed
        );
        let peer = peers.peer("fe80::1").expect("peer");
        assert_eq!(peer.ifname, "eth0");
        assert_eq!(peer.last_heard_at, core::time::Duration::from_secs(12));
    }

    #[test]
    fn peer_table_expires_stale_peers_after_python_timeout() {
        let mut peers = AutoPeerTable::new(
            core::time::Duration::from_secs(22),
            core::time::Duration::from_millis(5_200),
        );
        peers.observe_peer("fe80::1", "eth0", core::time::Duration::from_secs(0));

        assert!(peers.expire_stale(core::time::Duration::from_secs(22)).is_empty());
        let expired = peers.expire_stale(core::time::Duration::from_secs(23));

        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].address, "fe80::1");
        assert_eq!(peers.len(), 0);
    }

    #[test]
    fn peer_table_tracks_reverse_announce_due_times() {
        let mut peers = AutoPeerTable::new(
            core::time::Duration::from_secs(22),
            core::time::Duration::from_millis(5_200),
        );
        peers.observe_peer("fe80::1", "eth0", core::time::Duration::from_secs(10));

        assert!(peers.reverse_announces_due(core::time::Duration::from_millis(15_200)).is_empty());
        let due = peers.reverse_announces_due(core::time::Duration::from_millis(15_201));
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].address, "fe80::1");

        peers.mark_reverse_announced("fe80::1", core::time::Duration::from_millis(15_201));
        assert!(peers.reverse_announces_due(core::time::Duration::from_millis(20_401)).is_empty());
    }

    #[test]
    fn device_filter_matches_python_allow_and_ignore_order() {
        let filter = AutoInterfaceDeviceFilter {
            allowed: vec!["awdl0".to_string()],
            ignored: vec!["eth0".to_string()],
        };

        assert!(filter.should_adopt("awdl0", AutoInterfacePlatform::Darwin));
        assert!(!filter.should_adopt("eth0", AutoInterfacePlatform::Other));
        assert!(!filter.should_adopt("en0", AutoInterfacePlatform::Darwin));
        assert!(!filter.should_adopt("lo0", AutoInterfacePlatform::Darwin));
    }

    #[test]
    fn device_filter_matches_python_platform_defaults() {
        let filter = AutoInterfaceDeviceFilter::default();

        assert!(!filter.should_adopt("awdl0", AutoInterfacePlatform::Darwin));
        assert!(!filter.should_adopt("llw0", AutoInterfacePlatform::Darwin));
        assert!(!filter.should_adopt("en5", AutoInterfacePlatform::Darwin));
        assert!(!filter.should_adopt("lo0", AutoInterfacePlatform::Other));
        assert!(!filter.should_adopt("rmnet0", AutoInterfacePlatform::Android));
        assert!(filter.should_adopt("eth0", AutoInterfacePlatform::Other));
    }

    #[test]
    fn adopted_devices_select_python_link_local_addresses() {
        let filter = AutoInterfaceDeviceFilter {
            allowed: vec!["eth0".to_string(), "wlan0".to_string(), "eth1".to_string()],
            ignored: vec![],
        };
        let candidates = vec![
            AutoInterfaceDeviceCandidate {
                ifname: "eth0".to_string(),
                ipv6_addresses: vec![
                    "2001:db8::1".to_string(),
                    "fe80::1111%eth0".to_string(),
                    "fe80:abcd::2222".to_string(),
                ],
            },
            AutoInterfaceDeviceCandidate {
                ifname: "wlan0".to_string(),
                ipv6_addresses: vec!["2001:db8::2".to_string()],
            },
            AutoInterfaceDeviceCandidate {
                ifname: "eth1".to_string(),
                ipv6_addresses: vec!["fe80::3333%eth1".to_string()],
            },
        ];

        let adopted = filter.adopt_devices(&candidates, AutoInterfacePlatform::Other);

        assert_eq!(adopted.len(), 2);
        assert_eq!(adopted[0].ifname, "eth0");
        assert_eq!(adopted[0].link_local_address, "fe80::2222");
        assert_eq!(adopted[1].ifname, "eth1");
        assert_eq!(adopted[1].link_local_address, "fe80::3333");
    }

    #[test]
    fn adopted_devices_apply_platform_filter_before_link_local_selection() {
        let filter = AutoInterfaceDeviceFilter::default();
        let candidates = vec![
            AutoInterfaceDeviceCandidate {
                ifname: "awdl0".to_string(),
                ipv6_addresses: vec!["fe80::1111%awdl0".to_string()],
            },
            AutoInterfaceDeviceCandidate {
                ifname: "en0".to_string(),
                ipv6_addresses: vec!["fe80::2222%en0".to_string()],
            },
        ];

        let adopted = filter.adopt_devices(&candidates, AutoInterfacePlatform::Darwin);

        assert_eq!(adopted.len(), 1);
        assert_eq!(adopted[0].ifname, "en0");
        assert_eq!(adopted[0].link_local_address, "fe80::2222");
    }

    #[test]
    fn discovery_state_records_local_multicast_echo_without_peer() {
        let mut state = AutoDiscoveryState::new(
            vec![AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1111".to_string(),
            }],
            core::time::Duration::from_secs(22),
            core::time::Duration::from_millis(5_200),
        );

        let event = state.observe_discovery_packet(
            "fe80::1111%eth0",
            "eth0",
            core::time::Duration::from_secs(7),
        );

        assert_eq!(event, AutoDiscoveryEvent::LocalMulticastEcho { ifname: "eth0".to_string() });
        assert_eq!(state.peer_count(), 0);
        assert_eq!(state.last_multicast_echo("eth0"), Some(core::time::Duration::from_secs(7)));
        assert_eq!(state.initial_multicast_echo("eth0"), Some(core::time::Duration::from_secs(7)));
    }

    #[test]
    fn discovery_state_observes_remote_peer_when_not_local_echo() {
        let mut state = AutoDiscoveryState::new(
            vec![AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1111".to_string(),
            }],
            core::time::Duration::from_secs(22),
            core::time::Duration::from_millis(5_200),
        );

        let event = state.observe_discovery_packet(
            "fe80::2222%eth0",
            "eth0",
            core::time::Duration::from_secs(7),
        );

        assert_eq!(event, AutoDiscoveryEvent::Peer(AutoPeerEvent::Added));
        assert_eq!(state.peer_count(), 1);
        assert!(state.peer("fe80::2222").is_some());
        assert_eq!(state.last_multicast_echo("eth0"), None);
    }

    #[test]
    fn discovery_state_rejects_unauthenticated_discovery_packet() {
        let mut state = AutoDiscoveryState::new(
            vec![AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1111".to_string(),
            }],
            core::time::Duration::from_secs(22),
            core::time::Duration::from_millis(5_200),
        );

        let err = state
            .observe_authenticated_discovery_packet(
                &[0xAA; 32],
                b"reticulum",
                "fe80::2222%eth0",
                "eth0",
                core::time::Duration::from_secs(7),
            )
            .expect_err("bad discovery token must be rejected");

        assert_eq!(err, AutoDiscoveryRejectReason::InvalidToken);
        assert_eq!(state.peer_count(), 0);
        assert_eq!(state.last_multicast_echo("eth0"), None);
    }

    #[test]
    fn discovery_state_accepts_authenticated_remote_peer_packet() {
        let mut state = AutoDiscoveryState::new(
            vec![AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1111".to_string(),
            }],
            core::time::Duration::from_secs(22),
            core::time::Duration::from_millis(5_200),
        );
        let token = peering_token(b"reticulum", "fe80::2222%eth0");

        let event = state
            .observe_authenticated_discovery_packet(
                &token,
                b"reticulum",
                "fe80::2222%eth0",
                "eth0",
                core::time::Duration::from_secs(7),
            )
            .expect("valid discovery token");

        assert_eq!(event, AutoDiscoveryEvent::Peer(AutoPeerEvent::Added));
        assert!(state.peer("fe80::2222").is_some());
    }

    #[test]
    fn discovery_state_accepts_authenticated_packet_with_suffix_like_python() {
        let mut state = AutoDiscoveryState::new(
            vec![AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1111".to_string(),
            }],
            core::time::Duration::from_secs(22),
            core::time::Duration::from_millis(5_200),
        );
        let mut packet = peering_token(b"reticulum", "fe80::2222%eth0").to_vec();
        packet.extend_from_slice(b"ignored suffix");

        let event = state
            .observe_authenticated_discovery_packet(
                &packet,
                b"reticulum",
                "fe80::2222%eth0",
                "eth0",
                core::time::Duration::from_secs(7),
            )
            .expect("valid token prefix");

        assert_eq!(event, AutoDiscoveryEvent::Peer(AutoPeerEvent::Added));
    }

    #[test]
    fn discovery_state_tracks_python_multicast_echo_timeout_boundary() {
        let mut state = AutoDiscoveryState::new(
            vec![AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1111".to_string(),
            }],
            core::time::Duration::from_secs(22),
            core::time::Duration::from_millis(5_200),
        );
        state.observe_discovery_packet(
            "fe80::1111%eth0",
            "eth0",
            core::time::Duration::from_secs(10),
        );

        let events = state.update_multicast_echo_timeouts(
            core::time::Duration::from_millis(16_500),
            core::time::Duration::from_millis(6_500),
        );
        assert!(events.is_empty());
        assert_eq!(state.multicast_echo_timed_out("eth0"), Some(false));

        let events = state.update_multicast_echo_timeouts(
            core::time::Duration::from_millis(16_501),
            core::time::Duration::from_millis(6_500),
        );
        assert_eq!(
            events,
            vec![AutoMulticastCarrierEvent::CarrierLost { ifname: "eth0".to_string() }]
        );
        assert_eq!(state.multicast_echo_timed_out("eth0"), Some(true));
    }

    #[test]
    fn discovery_state_recovers_carrier_after_local_echo_returns() {
        let mut state = AutoDiscoveryState::new(
            vec![AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1111".to_string(),
            }],
            core::time::Duration::from_secs(22),
            core::time::Duration::from_millis(5_200),
        );

        assert!(state
            .update_multicast_echo_timeouts(
                core::time::Duration::from_millis(6_501),
                core::time::Duration::from_millis(6_500),
            )
            .is_empty());
        assert_eq!(state.multicast_echo_timed_out("eth0"), Some(true));

        state.observe_discovery_packet(
            "fe80::1111%eth0",
            "eth0",
            core::time::Duration::from_millis(7_000),
        );
        let events = state.update_multicast_echo_timeouts(
            core::time::Duration::from_millis(7_000),
            core::time::Duration::from_millis(6_500),
        );

        assert_eq!(
            events,
            vec![AutoMulticastCarrierEvent::CarrierRecovered { ifname: "eth0".to_string() }]
        );
        assert_eq!(state.multicast_echo_timed_out("eth0"), Some(false));
    }

    #[test]
    fn inbound_deduplicator_matches_python_multi_interface_ttl() {
        let mut dedupe =
            AutoInboundPacketDeduplicator::new(48, core::time::Duration::from_millis(750));

        assert!(dedupe.should_accept(b"packet", core::time::Duration::from_millis(1_000)));
        assert!(!dedupe.should_accept(b"packet", core::time::Duration::from_millis(1_749)));
        assert!(dedupe.should_accept(b"packet", core::time::Duration::from_millis(1_751)));
    }

    #[test]
    fn inbound_deduplicator_retains_python_window_length() {
        let mut dedupe =
            AutoInboundPacketDeduplicator::new(48, core::time::Duration::from_millis(750));
        for i in 0..48 {
            assert!(dedupe.should_accept(&[i], core::time::Duration::from_secs(1)));
        }

        assert!(!dedupe.should_accept(&[0], core::time::Duration::from_millis(1_100)));
        assert!(dedupe.should_accept(&[48], core::time::Duration::from_millis(1_100)));
        assert!(dedupe.should_accept(&[0], core::time::Duration::from_millis(1_200)));
    }
}
