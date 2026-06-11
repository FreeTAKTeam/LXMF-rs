use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct LxmdConfigFile {
    #[serde(default)]
    pub(crate) lxmd: LxmdConfigSection,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct LxmdConfigSection {
    pub(crate) profile: Option<String>,
    pub(crate) rpc: Option<String>,
    pub(crate) rnsconfig: Option<PathBuf>,
    pub(crate) propagation_node: Option<bool>,
    pub(crate) on_inbound: Option<String>,
    pub(crate) quiet: Option<bool>,
    pub(crate) service: Option<bool>,
    pub(crate) display_name: Option<String>,
    pub(crate) db: Option<PathBuf>,
    pub(crate) identity: Option<PathBuf>,
    pub(crate) transport: Option<String>,
    pub(crate) reticulumd: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct EffectiveArgs {
    pub(crate) profile: String,
    pub(crate) rpc: String,
    pub(crate) rnsconfig: Option<PathBuf>,
    pub(crate) propagation_node: bool,
    pub(crate) on_inbound: Option<String>,
    pub(crate) quiet: bool,
    pub(crate) service: bool,
    pub(crate) display_name: Option<String>,
    pub(crate) db: Option<PathBuf>,
    pub(crate) identity: Option<PathBuf>,
    pub(crate) transport: Option<String>,
    pub(crate) reticulumd: Option<PathBuf>,
    pub(crate) messages_dir: Option<PathBuf>,
    pub(crate) config_dir: Option<PathBuf>,
    pub(crate) timeout_secs: f64,
    pub(crate) status: bool,
    pub(crate) peers: bool,
    pub(crate) sync: Option<String>,
    pub(crate) unpeer: Option<String>,
    pub(crate) remote: Option<String>,
    pub(crate) query_identity: Option<PathBuf>,
    pub(crate) python_compat: PythonCompatConfig,
}

#[derive(Debug, Clone)]
pub(crate) struct LxmdPaths {
    pub(crate) config_dir: PathBuf,
    pub(crate) config_file: PathBuf,
    pub(crate) identity_file: PathBuf,
    pub(crate) storage_dir: PathBuf,
    pub(crate) messages_dir: PathBuf,
    pub(crate) generated_rnsconfig: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct SingleTomlConfigFile {
    #[serde(default)]
    pub(crate) node: SingleTomlNode,
    #[serde(default)]
    pub(crate) rpc: SingleTomlRpc,
    #[serde(default)]
    pub(crate) transport: SingleTomlTransport,
    #[serde(default)]
    pub(crate) storage: SingleTomlStorage,
    #[serde(default)]
    pub(crate) propagation: SingleTomlPropagation,
    #[serde(default)]
    pub(crate) lxmf: SingleTomlLxmf,
    #[serde(default)]
    pub(crate) interfaces: Vec<SingleTomlInterface>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct SingleTomlNode {
    pub(crate) display_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct SingleTomlRpc {
    pub(crate) listen: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct SingleTomlTransport {
    pub(crate) listen: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct SingleTomlStorage {
    pub(crate) db: Option<PathBuf>,
    pub(crate) identity: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct SingleTomlPropagation {
    pub(crate) enable: Option<bool>,
    pub(crate) announce_at_start: Option<bool>,
    pub(crate) announce_interval: Option<u64>,
    pub(crate) autopeer: Option<bool>,
    pub(crate) autopeer_maxdepth: Option<u32>,
    pub(crate) auth_required: Option<bool>,
    pub(crate) max_peers: Option<u32>,
    pub(crate) from_static_only: Option<bool>,
    pub(crate) retain_synced_on_node: Option<bool>,
    pub(crate) message_storage_limit_mb: Option<u64>,
    pub(crate) peering_cost: Option<u32>,
    pub(crate) remote_peering_cost_max: Option<u32>,
    pub(crate) static_peers: Option<Vec<String>>,
    pub(crate) control_allowed: Option<Vec<String>>,
    pub(crate) prioritised_destinations: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct SingleTomlLxmf {
    pub(crate) announce_at_start: Option<bool>,
    pub(crate) on_inbound: Option<String>,
    pub(crate) display_name: Option<String>,
    pub(crate) delivery_transfer_max_accepted_size: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct SingleTomlInterface {
    #[serde(rename = "type")]
    pub(crate) interface_type: String,
    #[serde(default = "default_true_bool")]
    pub(crate) enabled: bool,
    pub(crate) name: Option<String>,
    pub(crate) host: Option<String>,
    pub(crate) port: Option<u16>,
}

fn default_true_bool() -> bool {
    true
}

#[derive(Debug, Clone)]
pub(crate) struct PythonCompatConfig {
    pub(crate) auth_required: bool,
    pub(crate) autopeer: bool,
    pub(crate) autopeer_maxdepth: Option<u32>,
    pub(crate) allowed_identities: Vec<String>,
    pub(crate) ignored_destinations: Vec<String>,
    pub(crate) prioritised_destinations: Vec<String>,
    pub(crate) control_allowed: Vec<String>,
    pub(crate) static_peers: Vec<String>,
    pub(crate) node_name: Option<String>,
    pub(crate) message_storage_limit_mb: Option<u64>,
    pub(crate) propagation_message_max_kb: Option<f64>,
    pub(crate) propagation_sync_max_kb: Option<f64>,
    pub(crate) propagation_stamp_cost_target: Option<u32>,
    pub(crate) propagation_stamp_cost_flexibility: Option<u32>,
    pub(crate) peering_cost: Option<u32>,
    pub(crate) remote_peering_cost_max: Option<u32>,
    pub(crate) max_peers: Option<u32>,
    pub(crate) from_static_only: bool,
    pub(crate) retain_synced_on_node: bool,
    pub(crate) peer_announce_at_start: bool,
    pub(crate) node_announce_at_start: bool,
    pub(crate) peer_announce_interval_min: Option<u64>,
    pub(crate) node_announce_interval_min: Option<u64>,
    pub(crate) delivery_transfer_max_kb: Option<f64>,
}

impl Default for PythonCompatConfig {
    fn default() -> Self {
        Self {
            auth_required: false,
            autopeer: true,
            autopeer_maxdepth: Some(6),
            allowed_identities: Vec::new(),
            ignored_destinations: Vec::new(),
            prioritised_destinations: Vec::new(),
            control_allowed: Vec::new(),
            static_peers: Vec::new(),
            node_name: None,
            message_storage_limit_mb: None,
            propagation_message_max_kb: None,
            propagation_sync_max_kb: None,
            propagation_stamp_cost_target: None,
            propagation_stamp_cost_flexibility: None,
            peering_cost: None,
            remote_peering_cost_max: None,
            max_peers: None,
            from_static_only: false,
            retain_synced_on_node: false,
            peer_announce_at_start: false,
            node_announce_at_start: false,
            peer_announce_interval_min: None,
            node_announce_interval_min: None,
            delivery_transfer_max_kb: None,
        }
    }
}
