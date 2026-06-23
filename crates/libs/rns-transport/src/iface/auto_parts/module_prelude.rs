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
    pub fn parse(value: &str) -> Result<Option<Self>, &'static str> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" => Ok(None),
            "link" => Ok(Some(Self::Link)),
            "admin" => Ok(Some(Self::Admin)),
            "site" => Ok(Some(Self::Site)),
            "organisation" | "organization" => Ok(Some(Self::Organisation)),
            "global" => Ok(Some(Self::Global)),
            _ => Err("unknown auto discovery scope"),
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
