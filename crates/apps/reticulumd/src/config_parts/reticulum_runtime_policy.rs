#[derive(Debug, Clone, PartialEq)]
pub struct ReticulumRuntimePolicy {
    pub link_mtu_discovery: bool,
    pub static_transport_identity: bool,
    pub network_identity_path: Option<std::path::PathBuf>,
    pub local_hops_delta: bool,
    pub default_gravity: i64,
    pub remote_management_enabled: bool,
    pub respond_to_probes: bool,
    pub use_implicit_proof: bool,
    pub discover_interfaces: bool,
    pub required_discovery_value: Option<u32>,
    pub publish_blackhole: bool,
    pub blackhole_sources: Vec<String>,
    pub interface_discovery_sources: Vec<String>,
    pub max_autoconnected_interfaces: u32,
    pub autoconnect_interface_mode: Option<String>,
    pub autoconnect_interface_gravity: Option<i64>,
    pub autoconnect_announces_to_internal: Option<bool>,
    pub blackhole_update_interval_secs: f64,
    pub inbound_queue_limits: rns_transport::transport::InboundQueueLimits,
}

impl Default for ReticulumRuntimePolicy {
    fn default() -> Self {
        Self {
            link_mtu_discovery: true,
            static_transport_identity: false,
            network_identity_path: None,
            local_hops_delta: false,
            default_gravity: 0,
            remote_management_enabled: false,
            respond_to_probes: false,
            use_implicit_proof: true,
            discover_interfaces: false,
            required_discovery_value: None,
            publish_blackhole: false,
            blackhole_sources: Vec::new(),
            interface_discovery_sources: Vec::new(),
            max_autoconnected_interfaces: 0,
            autoconnect_interface_mode: None,
            autoconnect_interface_gravity: None,
            autoconnect_announces_to_internal: None,
            blackhole_update_interval_secs: 60.0 * 60.0,
            inbound_queue_limits: rns_transport::transport::InboundQueueLimits::default(),
        }
    }
}

impl ReticulumRuntimePolicy {
    pub fn from_toml(input: &str) -> Result<Self, toml::de::Error> {
        #[derive(serde::Deserialize)]
        struct Document {
            #[serde(default)]
            reticulum: Option<ReticulumConfigRaw>,
        }

        let document: Document = toml::from_str(input)?;
        document
            .reticulum
            .as_ref()
            .map(ReticulumConfigRaw::runtime_policy)
            .transpose()
            .map_err(<toml::de::Error as serde::de::Error>::custom)
            .map(Option::unwrap_or_default)
    }

    pub fn autoconnect_interface_mode(&self) -> Option<&str> {
        self.autoconnect_interface_mode.as_deref()
    }

    pub const fn autoconnect_interface_gravity(&self) -> Option<i64> {
        self.autoconnect_interface_gravity
    }

    pub const fn autoconnect_announces_to_internal(&self) -> Option<bool> {
        self.autoconnect_announces_to_internal
    }

    pub const fn blackhole_update_interval(&self) -> f64 {
        self.blackhole_update_interval_secs
    }

    pub const fn static_transport_identity(&self) -> bool {
        self.static_transport_identity
    }

    pub const fn local_hops_delta(&self) -> bool {
        self.local_hops_delta
    }

    pub const fn inbound_queue_limits(
        &self,
    ) -> rns_transport::transport::InboundQueueLimits {
        self.inbound_queue_limits
    }
}

impl ReticulumConfigRaw {
    fn runtime_policy(&self) -> Result<ReticulumRuntimePolicy, String> {
        Ok(ReticulumRuntimePolicy {
            link_mtu_discovery: self.link_mtu_discovery.unwrap_or(true),
            static_transport_identity: self.static_transport_identity.unwrap_or(false),
            network_identity_path: self.network_identity.clone(),
            local_hops_delta: self.local_hops_delta.unwrap_or(false),
            default_gravity: self.default_gravity.unwrap_or(0),
            remote_management_enabled: self.enable_remote_management.unwrap_or(false),
            respond_to_probes: self.respond_to_probes.unwrap_or(false),
            use_implicit_proof: self.use_implicit_proof.unwrap_or(true),
            discover_interfaces: self.discover_interfaces.unwrap_or(false),
            required_discovery_value: self
                .required_discovery_value
                .filter(|value| *value > 0)
                .map(|value| value as u32),
            publish_blackhole: self.publish_blackhole.unwrap_or(false),
            blackhole_sources: validate_identity_hash_list(
                "blackhole source",
                &self.blackhole_sources,
            )?,
            interface_discovery_sources: validate_identity_hash_list(
                "interface discovery source",
                &self.interface_discovery_sources,
            )?,
            max_autoconnected_interfaces: self
                .autoconnect_discovered_interfaces
                .filter(|value| *value > 0)
                .map(|value| value as u32)
                .unwrap_or(0),
            autoconnect_interface_mode: self
                .autoconnect_interface_mode
                .as_deref()
                .map(str::trim)
                .filter(|value| {
                    matches!(
                        value.to_ascii_lowercase().as_str(),
                        "full"
                            | "access_point"
                            | "accesspoint"
                            | "ap"
                            | "pointtopoint"
                            | "ptp"
                            | "roaming"
                            | "boundary"
                            | "gateway"
                            | "gw"
                            | "internal"
                    )
                })
                .map(str::to_ascii_lowercase),
            autoconnect_interface_gravity: self.autoconnect_interface_gravity,
            autoconnect_announces_to_internal: self.autoconnect_announces_to_internal,
            blackhole_update_interval_secs: self
                .blackhole_update_interval
                .unwrap_or(60.0)
                .max(2.0)
                * 60.0,
            inbound_queue_limits: rns_transport::transport::InboundQueueLimits {
                data: self.qlen_in_data.filter(|value| *value > 0).unwrap_or(
                    rns_transport::transport::DEFAULT_DATA_QUEUE_LENGTH,
                ),
                announce: self.qlen_in_announce.filter(|value| *value > 0).unwrap_or(
                    rns_transport::transport::DEFAULT_ANNOUNCE_QUEUE_LENGTH,
                ),
                path_request: self.qlen_in_pr.filter(|value| *value > 0).unwrap_or(
                    rns_transport::transport::DEFAULT_PATH_REQUEST_QUEUE_LENGTH,
                ),
                ingress_limited: self.qlen_in_il.filter(|value| *value > 0).unwrap_or(
                    rns_transport::transport::DEFAULT_INGRESS_LIMITED_QUEUE_LENGTH,
                ),
            },
        })
    }
}

fn validate_identity_hash_list(label: &str, values: &[String]) -> Result<Vec<String>, String> {
    let mut validated = Vec::new();
    for value in values {
        let normalized = value.trim().to_ascii_lowercase();
        if normalized.len() != 32 || hex::decode(&normalized).is_err() {
            return Err(format!(
                "{label} {value} is invalid, must be 32 hexadecimal characters (16 bytes)"
            ));
        }
        if !validated.contains(&normalized) {
            validated.push(normalized);
        }
    }
    Ok(validated)
}
