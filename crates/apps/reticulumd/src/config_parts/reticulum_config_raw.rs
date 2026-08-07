#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
struct ReticulumConfigRaw {
    #[serde(default)]
    enable_transport: Option<bool>,
    #[serde(default)]
    share_instance: Option<bool>,
    #[serde(default)]
    shared_instance_type: Option<String>,
    #[serde(default)]
    shared_instance_port: Option<u16>,
    #[serde(default)]
    instance_name: Option<String>,
    #[serde(default)]
    force_shared_instance_bitrate: Option<u64>,
    #[serde(default)]
    instance_control_port: Option<u16>,
    #[serde(default)]
    rpc_key: Option<String>,
    #[serde(default)]
    panic_on_interface_error: Option<bool>,
    #[serde(default)]
    link_mtu_discovery: Option<bool>,
    #[serde(default)]
    static_transport_identity: Option<bool>,
    #[serde(default)]
    local_hops_delta: Option<bool>,
    #[serde(default)]
    default_gravity: Option<i64>,
    #[serde(default)]
    enable_remote_management: Option<bool>,
    #[serde(default)]
    respond_to_probes: Option<bool>,
    #[serde(default)]
    use_implicit_proof: Option<bool>,
    #[serde(default)]
    discover_interfaces: Option<bool>,
    #[serde(default)]
    required_discovery_value: Option<i64>,
    #[serde(default)]
    publish_blackhole: Option<bool>,
    #[serde(default)]
    blackhole_sources: Vec<String>,
    #[serde(default)]
    interface_discovery_sources: Vec<String>,
    #[serde(default)]
    autoconnect_discovered_interfaces: Option<i64>,
    #[serde(default)]
    autoconnect_interface_mode: Option<String>,
    #[serde(default)]
    autoconnect_interface_gravity: Option<i64>,
    #[serde(default)]
    autoconnect_announces_to_internal: Option<bool>,
    #[serde(default)]
    blackhole_update_interval: Option<f64>,
}

impl ReticulumConfigRaw {
    fn global_shared_instance_interface(&self) -> InterfaceConfig {
        let shared_instance_type = self
            .shared_instance_type
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .filter(|value| matches!(value.as_str(), "tcp" | "unix"))
            .unwrap_or_else(default_global_shared_instance_type);
        let instance_name = self
            .instance_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let is_tcp = shared_instance_type == "tcp";
        InterfaceConfig {
            kind: "local".to_string(),
            enabled: Some(true),
            name: Some("shared-instance".to_string()),
            synthetic_shared_instance: true,
            shared_instance_type: Some(shared_instance_type),
            host: is_tcp.then(|| "127.0.0.1".to_string()),
            port: is_tcp.then_some(self.shared_instance_port.unwrap_or(37_428)),
            instance_name,
            force_shared_instance_bitrate: self.force_shared_instance_bitrate,
            ..InterfaceConfig::default()
        }
    }
}
