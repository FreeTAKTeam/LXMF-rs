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

    pub fn peers_by_ifname(&self, ifname: &str) -> Vec<AutoPeer> {
        self.peers
            .values()
            .filter(|peer| peer.ifname == ifname)
            .cloned()
            .collect()
    }

    pub fn remove_by_ifname(&mut self, ifname: &str) -> Vec<AutoPeer> {
        let removed = self
            .peers
            .iter()
            .filter_map(|(address, peer)| (peer.ifname == ifname).then_some(address.clone()))
            .collect::<Vec<_>>();
        removed.into_iter().filter_map(|address| self.peers.remove(&address)).collect()
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
pub enum AutoAdoptedInterfaceChange {
    Added {
        adopted: AutoInterfaceAdoptedDevice,
        discovery_listener: AutoDiscoveryListenerBinding,
        data_listener: AutoDataListenerBinding,
    },
    Removed {
        adopted: AutoInterfaceAdoptedDevice,
        discovery_listener: AutoDiscoveryListenerBinding,
        data_listener: AutoDataListenerBinding,
        removed_peers: Vec<AutoPeer>,
    },
    LinkLocalChanged(AutoLinkLocalAddressUpdate),
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
