use reticulum_daemon::announce_names::{
    encode_delivery_announce_app_data_with_capabilities,
    encode_propagation_node_app_data as encode_python_propagation_node_app_data,
    normalize_capabilities, normalize_display_name, PropagationNodeAnnounceConfig,
};

use reticulum_daemon::config::{DaemonConfig, InterfaceConfig};

use reticulum_daemon::identity_store::load_or_create_identity;

use crate::bridge_rnode_management::{
    DaemonRNodeManagementBinding, DaemonRNodeManagementBridge,
};

use rns_rpc::{
    AnnounceBridge, InterfaceRecord, MessagesStore, OutboundBridge, RemoteControlBridge,
    RpcDaemon, RpcRequest,
};

use rns_transport::destination::SingleInputDestination;

use rns_transport::hash::AddressHash;

use rns_transport::identity::Identity;

use rns_transport::transport::Transport;

use serde_json::{json, Map as JsonMap, Value as JsonValue};

use std::collections::{HashMap, HashSet};

use std::io::IsTerminal;

use std::net::SocketAddr;

use std::path::PathBuf;

use std::sync::{Arc, Mutex};

use tokio::net::TcpStream;

use tokio::sync::mpsc::channel;

use tokio::time::{timeout, Duration};

use transport_startup::{start_transport_and_interfaces, TransportStartupInput};

const INTERFACE_RUNTIME_STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
pub(super) struct RpcTlsConfig {
    pub(super) cert_chain_path: PathBuf,
    pub(super) private_key_path: PathBuf,
    pub(super) client_ca_path: Option<PathBuf>,
}

pub(super) struct BootstrapContext {
    pub(super) rpc_addr: Option<SocketAddr>,
    pub(super) rpc_unix: Option<PathBuf>,
    pub(super) daemon: Arc<RpcDaemon>,
    pub(super) rpc_tls: Option<RpcTlsConfig>,
}

const RECEIPT_EVENT_QUEUE_CAPACITY: usize = 1024;

#[derive(Clone)]
pub(super) struct PropagationControlContext {
    pub(super) enabled: bool,
    pub(super) local_identity_hash: [u8; 16],
    pub(super) propagation_destination_hash_hex: Option<String>,
    pub(super) control_destination_hash_hex: Option<String>,
    pub(super) delivery_destination:
        Option<Arc<tokio::sync::Mutex<rns_transport::destination::SingleInputDestination>>>,
    pub(super) allowed_control_identities: Vec<String>,
    pub(super) validated_peer_links: Arc<Mutex<HashSet<AddressHash>>>,
    pub(super) identified_peer_links: Arc<Mutex<HashMap<AddressHash, Identity>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct InterfaceStartupFailure {
    pub(super) label: String,
    pub(super) kind: String,
    pub(super) error: String,
}

pub(super) async fn bootstrap(args: Args) -> BootstrapContext {
    let rpc_addr: Option<SocketAddr> =
        args.rpc.as_ref().map(|addr| addr.parse().expect("invalid rpc address"));
    let rpc_unix = args.rpc_unix.clone();
    let rpc_tls = parse_tls_args(
        "--rpc-tls-cert",
        "--rpc-tls-key",
        "--rpc-tls-client-ca",
        args.rpc_tls_cert.clone(),
        args.rpc_tls_key.clone(),
        args.rpc_tls_client_ca.clone(),
    );
    let store = MessagesStore::open(&args.db).expect("open sqlite");

    let identity_path = args.identity.clone().unwrap_or_else(|| {
        let mut path = args.db.clone();
        path.set_extension("identity");
        path
    });
    let identity = load_or_create_identity(&identity_path).expect("load identity");
    let reticulum_storage_path =
        args.db.parent().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let mut local_identity_hash = [0u8; 16];
    local_identity_hash.copy_from_slice(identity.address_hash().as_slice());
    let daemon_config = args.config.as_ref().and_then(|path| match DaemonConfig::from_path(path) {
        Ok(config) => Some(config),
        Err(err) => {
            log::error!("[daemon] failed to load config {}: {}", path.display(), err);
            None
        }
    });
    let identity_hash = hex::encode(identity.address_hash().as_slice());
    let local_display_name = std::env::var("LXMF_DISPLAY_NAME")
        .ok()
        .and_then(|value| normalize_display_name(&value))
        .or_else(|| {
            daemon_config
                .as_ref()
                .and_then(|config| config.display_name.as_deref())
                .and_then(normalize_display_name)
        });
    let local_announce_capabilities = env_capabilities("LXMF_RCH_ANNOUNCE_CAPABILITIES")
        .or_else(|| {
            daemon_config
                .as_ref()
                .map(|config| normalize_capabilities(&config.announce_capabilities))
                .filter(|capabilities| !capabilities.is_empty())
        })
        .unwrap_or_default();
    let mut configured_interfaces = daemon_config
        .as_ref()
        .map(|config| {
            config.interfaces.iter().map(interface_record_from_config).collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let receipt_map: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
    let outbound_resource_map: OutboundResourceMap = Arc::new(Mutex::new(HashMap::new()));
    let (receipt_tx, receipt_rx) = channel(RECEIPT_EVENT_QUEUE_CAPACITY);
    let propagation_node_config = resolve_propagation_node_config(daemon_config.as_ref());
    let propagation_control_enabled = propagation_node_config.enabled;
    let configured_control_identities =
        propagation_node_config.allowed_control_identities.clone();
    let propagation_announce_config = propagation_node_config.announce_config;
    let propagation_announce_app_data =
        encode_propagation_node_app_data(local_display_name.as_deref(), propagation_announce_config);

    let startup = start_transport_and_interfaces(TransportStartupInput {
        args: &args,
        daemon_config: daemon_config.as_ref(),
        identity: &identity,
        reticulum_storage_path: reticulum_storage_path.as_path(),
        local_display_name: local_display_name.as_deref(),
        local_announce_capabilities: &local_announce_capabilities,
        propagation_announce_app_data: propagation_announce_app_data.clone(),
        configured_interfaces,
        receipt_map: receipt_map.clone(),
        receipt_tx: receipt_tx.clone(),
        propagation_control_enabled,
        propagation_announce_config: propagation_node_config.announce_config,
    })
    .await;

    let transport = startup.transport;
    let peer_crypto = startup.peer_crypto;
    let announce_destination = startup.announce_destination;
    let propagation_destination = startup.propagation_destination;
    let control_destination = startup.control_destination;
    let delivery_destination_hash_hex = startup.delivery_destination_hash_hex;
    let propagation_destination_hash_hex = startup.propagation_destination_hash_hex;
    let control_destination_hash_hex = startup.control_destination_hash_hex;
    let delivery_source_hash = startup.delivery_source_hash;
    configured_interfaces = startup.configured_interfaces;
    let startup_successes = startup.startup_successes;
    let startup_failures = startup.startup_failures;
    let seeded_tcp_interfaces = startup.seeded_tcp_interfaces;
    let auto_runtime_refreshes = startup.auto_runtime_refreshes;
    let pipe_runtime_refreshes = startup.pipe_runtime_refreshes;
    let udp_runtime_refreshes = startup.udp_runtime_refreshes;
    let i2p_runtime_refreshes = startup.i2p_runtime_refreshes;
    let tcp_runtime_refreshes = startup.tcp_runtime_refreshes;
    let weave_runtime_refreshes = startup.weave_runtime_refreshes;
    let rnode_multi_runtime_refreshes = startup.rnode_multi_runtime_refreshes;
    let lora_runtime_refreshes = startup.lora_runtime_refreshes;
    #[cfg(feature = "vrn76-kiss-ble")]
    let vrn76_runtime_refreshes = startup.vrn76_runtime_refreshes;
    let rnode_management_bindings = startup.rnode_management_bindings;
    let selected_tcp_server = startup.selected_tcp_server;

    if !startup_failures.is_empty() {
        log::warn!(
            "[daemon] interface startup degraded started={} failed={} strict={}",
            startup_successes,
            startup_failures.len(),
            args.strict_interface_startup
        );
        for failure in &startup_failures {
            log::warn!(
                "[daemon] interface startup failure name={} kind={} err={}",
                failure.label,
                failure.kind,
                failure.error
            );
        }
    }

    if let Err(policy_error) =
        enforce_startup_policy(args.strict_interface_startup, &startup_failures)
    {
        panic!("{policy_error}");
    }

    let transport_summary = if transport.is_some() {
        selected_tcp_server.bind_addr.clone().unwrap_or_else(|| "configured interfaces".to_string())
    } else {
        "disabled".to_string()
    };
    log::info!(
        "{}",
        pretty_boot_line(
            "startup",
            &format!(
                "reticulumd startup summary: rpc={} transport={} interfaces={} identity={}",
                rpc_addr.map(|addr| addr.to_string()).unwrap_or_else(|| "disabled".to_owned()),
                transport_summary,
                configured_interfaces.len(),
                identity_hash
            )
        )
    );

    let bridge: Option<Arc<TransportBridge>> =
        transport.as_ref().zip(announce_destination.as_ref()).map(|(transport, destination)| {
            let propagation_app_data =
                propagation_announce_app_data.clone();
            Arc::new(TransportBridge::new(
                transport.clone(),
                identity.clone(),
                delivery_source_hash,
                destination.clone(),
                local_display_name.as_ref().and_then(|display_name| {
                    encode_delivery_announce_app_data_with_capabilities(
                        display_name,
                        None,
                        &local_announce_capabilities,
                    )
                }),
                local_announce_capabilities.clone(),
                propagation_destination.clone(),
                propagation_app_data,
                control_destination.clone(),
                peer_crypto.clone(),
                receipt_map.clone(),
                outbound_resource_map.clone(),
                receipt_tx.clone(),
            ))
        });

    let outbound_bridge: Option<Arc<dyn OutboundBridge>> =
        bridge.as_ref().map(|bridge| bridge.clone() as Arc<dyn OutboundBridge>);
    let announce_bridge: Option<Arc<dyn AnnounceBridge>> =
        bridge.as_ref().map(|bridge| bridge.clone() as Arc<dyn AnnounceBridge>);

    let daemon = Arc::new(RpcDaemon::with_store_and_bridges(
        store,
        identity_hash,
        outbound_bridge,
        announce_bridge,
    ));
    configure_startup_rpc_token_auth(&args, daemon.as_ref());
    enforce_rpc_bind_security(rpc_addr.as_ref(), rpc_tls.as_ref(), daemon.as_ref());
    if let Some(transport) = transport.as_ref() {
        daemon.set_interface_mutation_bridge(Arc::new(TcpInterfaceMutationBridge::spawn(
            transport.iface_manager(),
            seeded_tcp_interfaces,
        )));
    }
    if !rnode_management_bindings.is_empty() {
        let bindings = rnode_management_bindings
            .into_iter()
            .map(|binding| DaemonRNodeManagementBinding {
                runtime_iface: binding.runtime_iface,
                name: binding.name,
                handle: binding.handle,
            })
            .collect();
        daemon.set_rnode_management_bridge(Arc::new(DaemonRNodeManagementBridge::new(bindings)));
    }
    if let Some(bridge) = bridge.as_ref() {
        bridge.set_daemon(daemon.clone());
        daemon.set_remote_control_bridge(bridge.clone() as Arc<dyn RemoteControlBridge>);
    }
    daemon.set_delivery_destination_hash(delivery_destination_hash_hex);
    daemon.set_propagation_destination_hash(propagation_destination_hash_hex.clone());
    daemon.replace_interfaces(configured_interfaces);
    spawn_auto_runtime_status_refresher(daemon.clone(), auto_runtime_refreshes);
    spawn_pipe_runtime_status_refresher(daemon.clone(), pipe_runtime_refreshes);
    spawn_udp_runtime_status_refresher(daemon.clone(), udp_runtime_refreshes);
    spawn_i2p_runtime_status_refresher(daemon.clone(), i2p_runtime_refreshes);
    spawn_tcp_runtime_status_refresher(daemon.clone(), tcp_runtime_refreshes);
    spawn_weave_runtime_status_refresher(daemon.clone(), weave_runtime_refreshes);
    spawn_rnode_multi_runtime_status_refresher(daemon.clone(), rnode_multi_runtime_refreshes);
    spawn_lora_runtime_status_refresher(daemon.clone(), lora_runtime_refreshes);
    #[cfg(feature = "vrn76-kiss-ble")]
    spawn_vrn76_runtime_status_refresher(daemon.clone(), vrn76_runtime_refreshes);
    daemon.set_propagation_state(transport.is_some(), None, 0);
    daemon.configure_propagation_node(
        propagation_node_config.enabled,
        propagation_node_config.peer_announce_at_start,
        propagation_node_config.peer_announce_interval_secs,
        propagation_node_config.node_announce_at_start,
        propagation_node_config.node_announce_interval_secs,
        propagation_node_config.announce_config.transfer_limit_kb,
        propagation_node_config.announce_config.sync_limit_kb,
        propagation_node_config.announce_config.stamp_cost,
        propagation_node_config.announce_config.stamp_cost_flexibility,
        propagation_node_config.announce_config.peering_cost,
        propagation_node_config.allowed_control_identities.clone(),
    );
    if propagation_node_config.enabled {
        if let Some(peer) = propagation_destination_hash_hex.as_deref() {
            let _ = daemon.handle_rpc(RpcRequest {
                id: 0,
                method: "set_outbound_propagation_node".to_string(),
                params: Some(json!({ "peer": peer })),
            });
        }
    }

    // Make the local delivery destination visible on startup when configured.
    if propagation_node_config.peer_announce_at_start {
        if let Some(bridge) = bridge.as_ref() {
            let _ = bridge.announce_now();
        }
    }
    if let Some(interval_secs) = propagation_node_config.peer_announce_interval_secs {
        if let Some(bridge) = bridge.as_ref() {
            spawn_bridge_announce_scheduler(bridge.clone(), interval_secs);
        }
    }

    if propagation_control_enabled && propagation_node_config.node_announce_at_start {
        if let Some(bridge) = bridge.as_ref() {
            let _ = bridge.announce_propagation_now();
        } else {
            if let Some((transport, destination)) =
                transport.as_ref().zip(propagation_destination.as_ref())
            {
                let propagation_app_data =
                    propagation_announce_app_data.clone();
                transport.send_announce(destination, propagation_app_data.as_deref()).await;
            }
            if let Some((transport, destination)) =
                transport.as_ref().zip(control_destination.as_ref())
            {
                transport.send_announce(destination, None).await;
            }
        }
    }
    if let Some(interval_secs) = propagation_node_config.node_announce_interval_secs {
        if propagation_control_enabled {
            if let Some(bridge) = bridge.as_ref() {
                spawn_bridge_propagation_announce_scheduler(bridge.clone(), interval_secs);
            } else {
                if let Some((transport, destination)) =
                    transport.as_ref().zip(propagation_destination.as_ref())
                {
                    let propagation_app_data =
                        propagation_announce_app_data.clone();
                    spawn_destination_announce_scheduler(
                        transport.clone(),
                        destination.clone(),
                        propagation_app_data,
                        interval_secs,
                    );
                }
                if let Some((transport, destination)) =
                    transport.as_ref().zip(control_destination.as_ref())
                {
                    spawn_destination_announce_scheduler(
                        transport.clone(),
                        destination.clone(),
                        None,
                        interval_secs,
                    );
                }
            }
        }
    }

    if transport.is_some() {
        spawn_receipt_worker(
            daemon.clone(),
            receipt_rx,
            receipt_map.clone(),
            outbound_resource_map.clone(),
        );
    }

    if args.announce_interval_secs > 0 {
        let _handle = daemon.clone().start_announce_scheduler_shared(args.announce_interval_secs);
    }

    if let Some(transport) = transport {
        spawn_inbound_worker(
            daemon.clone(),
            transport.clone(),
            PropagationControlContext {
                enabled: propagation_control_enabled,
                local_identity_hash,
                propagation_destination_hash_hex,
                control_destination_hash_hex,
                delivery_destination: announce_destination.clone(),
                allowed_control_identities: configured_control_identities,
                validated_peer_links: Arc::new(Mutex::new(HashSet::new())),
                identified_peer_links: Arc::new(Mutex::new(HashMap::new())),
            },
            receipt_tx.clone(),
            outbound_resource_map,
        );
        spawn_announce_worker(daemon.clone(), transport, peer_crypto, Some(reticulum_storage_path));
    }

    BootstrapContext { rpc_addr, rpc_unix, daemon, rpc_tls }
}

fn spawn_auto_runtime_status_refresher(
    daemon: Arc<RpcDaemon>,
    refreshes: Vec<transport_startup::AutoRuntimeRefresh>,
) {
    if refreshes.is_empty() {
        return;
    }
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(INTERFACE_RUNTIME_STATUS_REFRESH_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            refresh_auto_runtime_status_once(&daemon, &refreshes);
        }
    });
}

fn refresh_auto_runtime_status_once(
    daemon: &RpcDaemon,
    refreshes: &[transport_startup::AutoRuntimeRefresh],
) -> usize {
    refreshes
        .iter()
        .filter(|refresh| {
            daemon.update_interface_runtime_metadata_by_iface(
                refresh.runtime_iface.to_string().as_str(),
                "auto",
                "carrier_runtime",
                refresh.status.to_json(),
            )
        })
        .count()
}

fn spawn_pipe_runtime_status_refresher(
    daemon: Arc<RpcDaemon>,
    refreshes: Vec<transport_startup::PipeRuntimeRefresh>,
) {
    if refreshes.is_empty() {
        return;
    }
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(INTERFACE_RUNTIME_STATUS_REFRESH_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            refresh_pipe_runtime_status_once(&daemon, &refreshes);
        }
    });
}

fn refresh_pipe_runtime_status_once(
    daemon: &RpcDaemon,
    refreshes: &[transport_startup::PipeRuntimeRefresh],
) -> usize {
    refreshes
        .iter()
        .filter(|refresh| {
            daemon.update_interface_runtime_metadata_by_iface(
                refresh.runtime_iface.to_string().as_str(),
                "pipe",
                "status",
                refresh.status.to_json(),
            )
        })
        .count()
}

fn spawn_udp_runtime_status_refresher(
    daemon: Arc<RpcDaemon>,
    refreshes: Vec<transport_startup::UdpRuntimeRefresh>,
) {
    if refreshes.is_empty() {
        return;
    }
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(INTERFACE_RUNTIME_STATUS_REFRESH_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            refresh_udp_runtime_status_once(&daemon, &refreshes);
        }
    });
}

fn refresh_udp_runtime_status_once(
    daemon: &RpcDaemon,
    refreshes: &[transport_startup::UdpRuntimeRefresh],
) -> usize {
    refreshes
        .iter()
        .filter(|refresh| {
            daemon.update_interface_runtime_metadata_by_iface(
                refresh.runtime_iface.to_string().as_str(),
                "udp",
                "status",
                refresh.status.to_json(),
            )
        })
        .count()
}

fn spawn_i2p_runtime_status_refresher(
    daemon: Arc<RpcDaemon>,
    refreshes: Vec<transport_startup::I2pRuntimeRefresh>,
) {
    if refreshes.is_empty() {
        return;
    }
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(INTERFACE_RUNTIME_STATUS_REFRESH_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            refresh_i2p_runtime_status_once(&daemon, &refreshes);
        }
    });
}

fn refresh_i2p_runtime_status_once(
    daemon: &RpcDaemon,
    refreshes: &[transport_startup::I2pRuntimeRefresh],
) -> usize {
    refreshes
        .iter()
        .filter(|refresh| {
            daemon.update_interface_runtime_metadata_by_iface(
                refresh.runtime_iface.to_string().as_str(),
                "i2p",
                "tunnel_status",
                refresh.status.to_json(),
            )
        })
        .count()
}

fn spawn_tcp_runtime_status_refresher(
    daemon: Arc<RpcDaemon>,
    refreshes: Vec<transport_startup::TcpRuntimeRefresh>,
) {
    if refreshes.is_empty() {
        return;
    }
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(INTERFACE_RUNTIME_STATUS_REFRESH_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            refresh_tcp_runtime_status_once(&daemon, &refreshes);
        }
    });
}

fn refresh_tcp_runtime_status_once(
    daemon: &RpcDaemon,
    refreshes: &[transport_startup::TcpRuntimeRefresh],
) -> usize {
    refreshes
        .iter()
        .filter(|refresh| {
            daemon.update_interface_runtime_metadata_by_iface(
                refresh.runtime_iface.to_string().as_str(),
                "tcp",
                refresh.status.runtime_key(),
                refresh.status.to_json(),
            )
        })
        .count()
}

fn spawn_weave_runtime_status_refresher(
    daemon: Arc<RpcDaemon>,
    refreshes: Vec<transport_startup::WeaveRuntimeRefresh>,
) {
    if refreshes.is_empty() {
        return;
    }
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(INTERFACE_RUNTIME_STATUS_REFRESH_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            refresh_weave_runtime_status_once(&daemon, &refreshes);
        }
    });
}

fn refresh_weave_runtime_status_once(
    daemon: &RpcDaemon,
    refreshes: &[transport_startup::WeaveRuntimeRefresh],
) -> usize {
    refreshes
        .iter()
        .filter(|refresh| {
            daemon.update_interface_runtime_metadata_by_iface(
                refresh.runtime_iface.to_string().as_str(),
                "weave",
                "status",
                refresh.status.to_json(),
            )
        })
        .count()
}

fn spawn_rnode_multi_runtime_status_refresher(
    daemon: Arc<RpcDaemon>,
    refreshes: Vec<transport_startup::RNodeMultiRuntimeRefresh>,
) {
    if refreshes.is_empty() {
        return;
    }
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(INTERFACE_RUNTIME_STATUS_REFRESH_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            refresh_rnode_multi_runtime_status_once(&daemon, &refreshes);
        }
    });
}

fn refresh_rnode_multi_runtime_status_once(
    daemon: &RpcDaemon,
    refreshes: &[transport_startup::RNodeMultiRuntimeRefresh],
) -> usize {
    refreshes
        .iter()
        .filter(|refresh| {
            daemon.update_interface_runtime_metadata_by_iface(
                refresh.runtime_iface.to_string().as_str(),
                "rnode_multi",
                "radio_status",
                refresh.status.to_json(),
            )
        })
        .count()
}

fn spawn_lora_runtime_status_refresher(
    daemon: Arc<RpcDaemon>,
    refreshes: Vec<transport_startup::LoraRuntimeRefresh>,
) {
    if refreshes.is_empty() {
        return;
    }
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(INTERFACE_RUNTIME_STATUS_REFRESH_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            refresh_lora_runtime_status_once(&daemon, &refreshes);
        }
    });
}

fn refresh_lora_runtime_status_once(
    daemon: &RpcDaemon,
    refreshes: &[transport_startup::LoraRuntimeRefresh],
) -> usize {
    refreshes
        .iter()
        .filter(|refresh| {
            daemon.update_interface_runtime_metadata_by_iface(
                refresh.runtime_iface.to_string().as_str(),
                "lora",
                "rnode_status",
                refresh.status.to_json(),
            )
        })
        .count()
}

#[cfg(feature = "vrn76-kiss-ble")]
fn spawn_vrn76_runtime_status_refresher(
    daemon: Arc<RpcDaemon>,
    refreshes: Vec<transport_startup::Vrn76RuntimeRefresh>,
) {
    if refreshes.is_empty() {
        return;
    }
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(INTERFACE_RUNTIME_STATUS_REFRESH_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            refresh_vrn76_runtime_status_once(&daemon, &refreshes);
        }
    });
}

#[cfg(feature = "vrn76-kiss-ble")]
fn refresh_vrn76_runtime_status_once(
    daemon: &RpcDaemon,
    refreshes: &[transport_startup::Vrn76RuntimeRefresh],
) -> usize {
    refreshes
        .iter()
        .filter(|refresh| {
            daemon.update_interface_runtime_metadata_by_iface(
                refresh.runtime_iface.to_string().as_str(),
                "vrn76",
                "status",
                refresh.status.to_json(),
            )
        })
        .count()
}

fn pretty_console_logs_enabled() -> bool {
    matches!(
        std::env::var("LXMF_LOG_PRETTY").ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON")
    )
}

fn pretty_color_enabled() -> bool {
    if matches!(
        std::env::var("LXMF_LOG_COLOR").ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON" | "always" | "ALWAYS")
    ) {
        return true;
    }
    if matches!(
        std::env::var("LXMF_LOG_COLOR").ok().as_deref(),
        Some("0" | "false" | "FALSE" | "no" | "NO" | "off" | "OFF" | "never" | "NEVER")
    ) {
        return false;
    }
    pretty_console_logs_enabled() && std::io::stderr().is_terminal()
}

fn ansi(text: &str, code: &str) -> String {
    if pretty_color_enabled() {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

fn pretty_boot_line(tag: &str, body: &str) -> String {
    if !pretty_console_logs_enabled() {
        return body.to_string();
    }
    format!("{} {}", ansi(&format!("[{tag}]"), "1;35"), body)
}

fn pretty_daemon_line(body: &str) -> String {
    if !pretty_console_logs_enabled() {
        return format!("[daemon] {body}");
    }
    format!("{} {}", ansi("[daemon]", "1;34"), body)
}

fn pretty_warn_line(body: &str) -> String {
    if !pretty_console_logs_enabled() {
        return format!("[warn] {body}");
    }
    format!("{} {}", ansi("[warn]", "1;33"), body)
}

fn parse_tls_args(
    cert_flag: &str,
    key_flag: &str,
    client_ca_flag: &str,
    cert_chain_path: Option<PathBuf>,
    private_key_path: Option<PathBuf>,
    client_ca_path: Option<PathBuf>,
) -> Option<RpcTlsConfig> {
    match (cert_chain_path, private_key_path, client_ca_path) {
        (None, None, None) => None,
        (Some(cert_chain_path), Some(private_key_path), client_ca_path) => {
            Some(RpcTlsConfig { cert_chain_path, private_key_path, client_ca_path })
        }
        (None, None, Some(_)) => {
            panic!("{client_ca_flag} requires {cert_flag} and {key_flag}")
        }
        _ => panic!("{cert_flag} and {key_flag} must be provided together"),
    }
}

pub(super) fn enforce_rpc_bind_security(
    rpc_addr: Option<&SocketAddr>,
    rpc_tls: Option<&RpcTlsConfig>,
    daemon: &RpcDaemon,
) {
    let Some(addr) = rpc_addr else {
        return;
    };
    if is_local_rpc_bind(addr) {
        return;
    }
    if rpc_tls.and_then(|config| config.client_ca_path.as_ref()).is_some() {
        return;
    }
    if daemon.remote_rpc_auth_configured() {
        return;
    }
    panic!(
        "remote TCP RPC bind {} requires token auth or mTLS; bind to loopback, use --rpc-unix, configure persisted remote token auth, or provide --rpc-tls-client-ca",
        addr
    );
}

fn is_local_rpc_bind(addr: &SocketAddr) -> bool {
    let ip = addr.ip();
    ip.is_loopback() && !ip.is_unspecified()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_runtime_status_refresh_updates_matching_interface_record() {
        let daemon = RpcDaemon::test_instance();
        let runtime_iface = AddressHash::new([0x18; 16]);
        daemon.replace_interfaces(vec![InterfaceRecord {
            kind: "auto".to_string(),
            enabled: true,
            host: None,
            port: None,
            name: Some("auto-main".to_string()),
            settings: Some(json!({
                "_runtime": {
                    "iface": runtime_iface.to_string(),
                    "startup_status": "spawned",
                    "auto": {
                        "carrier_runtime": {
                            "carrier_changed": false
                        }
                    }
                }
            })),
        }]);
        let plan = rns_transport::iface::auto::AutoStartupPlan {
            discovery_listeners: Vec::new(),
            data_listeners: Vec::new(),
            peer_job_interval: core::time::Duration::ZERO,
            initial_peering_wait: core::time::Duration::ZERO,
        };
        let status = crate::interfaces::auto::AutoRuntimeStatusHandle::from_startup_plan(&plan);
        status.record_carrier_events(&[
            rns_transport::iface::auto::AutoMulticastCarrierEvent::CarrierLost {
                ifname: "eth0".to_string(),
            },
        ]);
        let refresh = transport_startup::AutoRuntimeRefresh { runtime_iface, status };

        assert_eq!(refresh_auto_runtime_status_once(&daemon, &[refresh]), 1);
        let result = daemon
            .handle_rpc(RpcRequest { id: 87, method: "daemon_status_ex".to_string(), params: None })
            .expect("daemon status")
            .result
            .expect("daemon status result");
        let carrier_runtime = &result["interfaces"][0]["settings"]["_runtime"]["auto"]
            ["carrier_runtime"];

        assert_eq!(carrier_runtime["online"].as_bool(), Some(true));
        assert_eq!(carrier_runtime["final_init_done"].as_bool(), Some(true));
        assert_eq!(carrier_runtime["carrier_changed"].as_bool(), Some(true));
        assert_eq!(carrier_runtime["carrier_event_count"].as_u64(), Some(1));
        assert_eq!(carrier_runtime["carrier_events"][0]["event"].as_str(), Some("carrier_lost"));
    }

    #[test]
    fn i2p_runtime_status_refresh_updates_matching_interface_record() {
        let daemon = RpcDaemon::test_instance();
        let runtime_iface = AddressHash::new([0x19; 16]);
        daemon.replace_interfaces(vec![InterfaceRecord {
            kind: "i2p".to_string(),
            enabled: true,
            host: Some("127.0.0.1".to_string()),
            port: Some(7656),
            name: Some("i2p-main".to_string()),
            settings: Some(json!({
                "_runtime": {
                    "iface": runtime_iface.to_string(),
                    "startup_status": "spawned",
                    "i2p": {
                        "tunnel_status": {
                            "accept_state": "stale"
                        }
                    }
                }
            })),
        }]);
        let manager = Arc::new(tokio::sync::Mutex::new(
            rns_transport::iface::InterfaceManager::new(8),
        ));
        let status = rns_transport::iface::i2p::I2pInterface::new("i2p-main", manager)
            .with_connectable(true)
            .with_peers(vec!["peer-a.b32.i2p".to_string(), "peer-b.b32.i2p".to_string()])
            .runtime_status_handle();
        status.mark_accept_listening();
        status.mark_outbound_connected("peer-a.b32.i2p", AddressHash::new([0x20; 16]));
        status.mark_outbound_reconnecting(
            "peer-b.b32.i2p",
            AddressHash::new([0x21; 16]),
            "simulated stream reset".to_string(),
        );
        for index in 0..18 {
            let iface = AddressHash::new([0x40 + index; 16]);
            status.mark_incoming_connected(format!("closed-{index}").as_str(), iface);
            status.mark_incoming_closed(iface);
        }
        let status_handle = status.clone();
        let refresh = transport_startup::I2pRuntimeRefresh { runtime_iface, status };

        assert_eq!(refresh_i2p_runtime_status_once(&daemon, std::slice::from_ref(&refresh)), 1);
        let result = daemon
            .handle_rpc(RpcRequest { id: 88, method: "daemon_status_ex".to_string(), params: None })
            .expect("daemon status")
            .result
            .expect("daemon status result");
        let tunnel_status = &result["interfaces"][0]["settings"]["_runtime"]["i2p"]
            ["tunnel_status"];

        assert_eq!(tunnel_status["accept_state"].as_str(), Some("listening"));
        assert_eq!(tunnel_status["connectable"].as_bool(), Some(true));
        assert_eq!(tunnel_status["configured_peer_count"].as_u64(), Some(2));
        let peers = tunnel_status["peers"].as_array().expect("i2p peer rows");
        let peer_row = |name: &str| {
            peers
                .iter()
                .find(|row| row["peer"].as_str() == Some(name))
                .unwrap_or_else(|| panic!("missing i2p peer row {name}"))
        };
        let peer_a = peer_row("peer-a.b32.i2p");
        assert_eq!(peer_a["state"].as_str(), Some("connected"));
        assert_eq!(peer_a["direction"].as_str(), Some("outbound"));
        assert_eq!(peer_a["iface"].as_str(), Some(AddressHash::new([0x20; 16]).to_string().as_str()));
        let peer_b = peer_row("peer-b.b32.i2p");
        assert_eq!(peer_b["state"].as_str(), Some("reconnecting"));
        assert_eq!(peer_b["reconnect_attempts"].as_u64(), Some(1));
        assert_eq!(peer_b["last_error"].as_str(), Some("simulated stream reset"));
        assert!(peers.iter().all(|row| row["peer"].as_str() != Some("closed-0")));
        assert!(peers.iter().all(|row| row["peer"].as_str() != Some("closed-1")));
        assert!(peers.iter().any(|row| row["peer"].as_str() == Some("closed-17")));
        let closed_incoming_count = peers
            .iter()
            .filter(|row| {
                row["direction"].as_str() == Some("incoming")
                    && row["state"].as_str() == Some("closed")
            })
            .count();
        assert_eq!(closed_incoming_count, 16);

        status_handle.mark_accept_closed();
        assert_eq!(refresh_i2p_runtime_status_once(&daemon, std::slice::from_ref(&refresh)), 1);
        let result = daemon
            .handle_rpc(RpcRequest { id: 89, method: "daemon_status_ex".to_string(), params: None })
            .expect("daemon status after accept close")
            .result
            .expect("daemon status result after accept close");
        let tunnel_status = &result["interfaces"][0]["settings"]["_runtime"]["i2p"]
            ["tunnel_status"];
        assert_eq!(tunnel_status["accept_state"].as_str(), Some("closed"));
        assert!(tunnel_status["last_accept_error"].is_null());
    }

    #[test]
    fn pipe_runtime_status_refresh_updates_matching_interface_record() {
        let daemon = RpcDaemon::test_instance();
        let runtime_iface = AddressHash::new([0x1a; 16]);
        daemon.replace_interfaces(vec![InterfaceRecord {
            kind: "pipe".to_string(),
            enabled: true,
            host: None,
            port: None,
            name: Some("pipe-main".to_string()),
            settings: Some(json!({
                "_runtime": {
                    "iface": runtime_iface.to_string(),
                    "startup_status": "spawned",
                    "pipe": {
                        "status": {
                            "process_state": "stale"
                        }
                    }
                }
            })),
        }]);
        let status = rns_transport::iface::pipe::PipeInterface::new("cat").runtime_status_handle();
        status.record_error_for_test("respawning", "spawn cat failed");
        let refresh = transport_startup::PipeRuntimeRefresh { runtime_iface, status };

        assert_eq!(refresh_pipe_runtime_status_once(&daemon, &[refresh]), 1);
        let result = daemon
            .handle_rpc(RpcRequest { id: 93, method: "daemon_status_ex".to_string(), params: None })
            .expect("daemon status")
            .result
            .expect("daemon status result");
        let status = &result["interfaces"][0]["settings"]["_runtime"]["pipe"]["status"];

        assert_eq!(status["command"].as_str(), Some("cat"));
        assert_eq!(status["process_state"].as_str(), Some("respawning"));
        assert_eq!(status["pipe_is_open"].as_bool(), Some(false));
        assert_eq!(status["respawn_attempts"].as_u64(), Some(1));
        assert_eq!(status["last_error"].as_str(), Some("spawn cat failed"));
    }

    #[test]
    fn udp_runtime_status_refresh_updates_matching_interface_record() {
        let daemon = RpcDaemon::test_instance();
        let runtime_iface = AddressHash::new([0x25; 16]);
        daemon.replace_interfaces(vec![InterfaceRecord {
            kind: "udp".to_string(),
            enabled: true,
            host: Some("127.0.0.1".to_string()),
            port: Some(4242),
            name: Some("udp-main".to_string()),
            settings: Some(json!({
                "_runtime": {
                    "iface": runtime_iface.to_string(),
                    "startup_status": "spawned",
                    "udp": {
                        "status": {
                            "link_state": "configured"
                        }
                    }
                }
            })),
        }]);
        let status =
            rns_transport::iface::udp::UdpInterface::new("127.0.0.1:4242", Some("192.0.2.1:4242"))
                .runtime_status_handle();
        status.update(|status| {
            status.link_state = "bound".to_string();
            status.iface = Some(runtime_iface.to_string());
            status.peer_routes = 3;
            status.packets_rx = 2;
            status.packets_tx = 1;
            status.bytes_rx = 120;
            status.bytes_tx = 64;
            status.decode_errors = 1;
            status.rx_queue_errors = 2;
            status.socket_errors = 3;
            status.tx_errors = 4;
            status.dropped_direct = 5;
            status.last_error = Some("simulated udp decode failure".to_string());
        });
        let refresh = transport_startup::UdpRuntimeRefresh { runtime_iface, status };

        assert_eq!(refresh_udp_runtime_status_once(&daemon, &[refresh]), 1);
        let result = daemon
            .handle_rpc(RpcRequest { id: 94, method: "daemon_status_ex".to_string(), params: None })
            .expect("daemon status")
            .result
            .expect("daemon status result");
        let status = &result["interfaces"][0]["settings"]["_runtime"]["udp"]["status"];

        assert_eq!(status["link_state"].as_str(), Some("bound"));
        assert_eq!(status["role"].as_str(), Some("peer"));
        assert_eq!(status["bind_addr"].as_str(), Some("127.0.0.1:4242"));
        assert_eq!(status["forward_addr"].as_str(), Some("192.0.2.1:4242"));
        assert_eq!(status["iface"].as_str(), Some(runtime_iface.to_string().as_str()));
        assert_eq!(status["peer_routes"].as_u64(), Some(3));
        assert_eq!(status["packets_rx"].as_u64(), Some(2));
        assert_eq!(status["packets_tx"].as_u64(), Some(1));
        assert_eq!(status["bytes_rx"].as_u64(), Some(120));
        assert_eq!(status["bytes_tx"].as_u64(), Some(64));
        assert_eq!(status["decode_errors"].as_u64(), Some(1));
        assert_eq!(status["rx_queue_errors"].as_u64(), Some(2));
        assert_eq!(status["socket_errors"].as_u64(), Some(3));
        assert_eq!(status["tx_errors"].as_u64(), Some(4));
        assert_eq!(status["dropped_direct"].as_u64(), Some(5));
        assert_eq!(status["last_error"].as_str(), Some("simulated udp decode failure"));
    }

    #[test]
    fn tcp_runtime_status_refresh_updates_matching_interface_record() {
        let daemon = RpcDaemon::test_instance();
        let runtime_iface = AddressHash::new([0x1b; 16]);
        daemon.replace_interfaces(vec![InterfaceRecord {
            kind: "backbone_client".to_string(),
            enabled: true,
            host: Some("127.0.0.1".to_string()),
            port: Some(4242),
            name: Some("backbone-main".to_string()),
            settings: Some(json!({
                "_runtime": {
                    "iface": runtime_iface.to_string(),
                    "startup_status": "spawned",
                    "tcp": {
                        "stream_status": {
                            "stream_state": "stale"
                        }
                    }
                }
            })),
        }]);
        let status = rns_transport::iface::tcp_client::TcpClient::new("127.0.0.1:4242")
            .with_backbone_liveness()
            .with_forced_bitrate(9_600)
            .runtime_status_handle();
        let refresh = transport_startup::TcpRuntimeRefresh {
            runtime_iface,
            status: transport_startup::TcpRuntimeStatusSource::Stream(status),
        };

        assert_eq!(refresh_tcp_runtime_status_once(&daemon, &[refresh]), 1);
        let result = daemon
            .handle_rpc(RpcRequest { id: 94, method: "daemon_status_ex".to_string(), params: None })
            .expect("daemon status")
            .result
            .expect("daemon status result");
        let status = &result["interfaces"][0]["settings"]["_runtime"]["tcp"]["stream_status"];

        assert_eq!(status["endpoint"].as_str(), Some("127.0.0.1:4242"));
        assert_eq!(status["stream_state"].as_str(), Some("configured"));
        assert_eq!(status["liveness_enabled"].as_bool(), Some(true));
        assert_eq!(status["forced_bitrate_bps"].as_u64(), Some(9_600));
    }

    #[test]
    fn tcp_listener_runtime_status_refresh_updates_matching_interface_record() {
        let daemon = RpcDaemon::test_instance();
        let runtime_iface = AddressHash::default();
        daemon.replace_interfaces(vec![InterfaceRecord {
            kind: "backbone".to_string(),
            enabled: true,
            host: Some("127.0.0.1".to_string()),
            port: Some(4242),
            name: Some("backbone-main".to_string()),
            settings: Some(json!({
                "_runtime": {
                    "iface": runtime_iface.to_string(),
                    "startup_status": "active",
                    "tcp": {
                        "listener_status": {
                            "listener_state": "configured"
                        }
                    }
                }
            })),
        }]);
        let manager =
            Arc::new(tokio::sync::Mutex::new(rns_transport::iface::InterfaceManager::new(8)));
        let status = rns_transport::iface::tcp_server::TcpServer::new(
            "127.0.0.1:4242",
            manager,
        )
        .with_backbone_client_liveness()
        .with_client_forced_bitrate(9_600)
        .runtime_status_handle();
        let refresh = transport_startup::TcpRuntimeRefresh {
            runtime_iface,
            status: transport_startup::TcpRuntimeStatusSource::Listener(status),
        };

        assert_eq!(refresh_tcp_runtime_status_once(&daemon, &[refresh]), 1);
        let result = daemon
            .handle_rpc(RpcRequest { id: 95, method: "daemon_status_ex".to_string(), params: None })
            .expect("daemon status")
            .result
            .expect("daemon status result");
        let status = &result["interfaces"][0]["settings"]["_runtime"]["tcp"]["listener_status"];

        assert_eq!(status["bind_addr"].as_str(), Some("127.0.0.1:4242"));
        assert_eq!(status["listener_state"].as_str(), Some("configured"));
        assert_eq!(status["client_liveness_enabled"].as_bool(), Some(true));
        assert_eq!(status["client_forced_bitrate_bps"].as_u64(), Some(9_600));
        assert_eq!(status["accepted_connections"].as_u64(), Some(0));
    }

    #[test]
    fn weave_runtime_status_refresh_updates_matching_interface_record() {
        let daemon = RpcDaemon::test_instance();
        let runtime_iface = AddressHash::new([0x20; 16]);
        daemon.replace_interfaces(vec![InterfaceRecord {
            kind: "weave".to_string(),
            enabled: true,
            host: None,
            port: None,
            name: Some("weave-main".to_string()),
            settings: Some(json!({
                "_runtime": {
                    "iface": runtime_iface.to_string(),
                    "startup_status": "spawned",
                    "weave": {
                        "status": {
                            "link_state": "stale"
                        }
                    }
                }
            })),
        }]);
        let manager = Arc::new(tokio::sync::Mutex::new(
            rns_transport::iface::InterfaceManager::new(8),
        ));
        let status = rns_transport::iface::weave::WeaveInterface::new("test", manager)
            .runtime_status_handle();
        let refresh = transport_startup::WeaveRuntimeRefresh { runtime_iface, status };

        assert_eq!(refresh_weave_runtime_status_once(&daemon, &[refresh]), 1);
        let result = daemon
            .handle_rpc(RpcRequest { id: 89, method: "daemon_status_ex".to_string(), params: None })
            .expect("daemon status")
            .result
            .expect("daemon status result");
        let status = &result["interfaces"][0]["settings"]["_runtime"]["weave"]["status"];

        assert_eq!(status["link_state"].as_str(), Some("configured"));
        assert_eq!(status["device"].as_str(), Some("test"));
    }

    #[test]
    fn rnode_multi_runtime_status_refresh_updates_matching_interface_record() {
        let daemon = RpcDaemon::test_instance();
        let runtime_iface = AddressHash::new([0x21; 16]);
        daemon.replace_interfaces(vec![InterfaceRecord {
            kind: "rnode_multi".to_string(),
            enabled: true,
            host: None,
            port: None,
            name: Some("rnode-multi".to_string()),
            settings: Some(json!({
                "_runtime": {
                    "iface": runtime_iface.to_string(),
                    "startup_status": "spawned",
                    "rnode_multi": {
                        "radio_status": {
                            "selected_vport": 99
                        }
                    }
                }
            })),
        }]);
        let manager = Arc::new(tokio::sync::Mutex::new(
            rns_transport::iface::InterfaceManager::new(8),
        ));
        let status =
            rns_transport::iface::rnode_multi::RNodeMultiInterface::new("test", manager)
                .with_subinterfaces(vec![
                    rns_transport::iface::rnode_multi::RNodeMultiSubInterfaceConfig {
                        name: "radio0".to_string(),
                        vport: 2,
                        config: rns_transport::iface::lora::LoraConfig::us915_default(),
                        outgoing: true,
                    },
                    rns_transport::iface::rnode_multi::RNodeMultiSubInterfaceConfig {
                        name: "radio1".to_string(),
                        vport: 3,
                        config: rns_transport::iface::lora::LoraConfig::us915_default(),
                        outgoing: true,
                    },
                ])
                .runtime_status_handle();
        let refresh = transport_startup::RNodeMultiRuntimeRefresh { runtime_iface, status };

        assert_eq!(refresh_rnode_multi_runtime_status_once(&daemon, &[refresh]), 1);
        let result = daemon
            .handle_rpc(RpcRequest { id: 90, method: "daemon_status_ex".to_string(), params: None })
            .expect("daemon status")
            .result
            .expect("daemon status result");
        let radio_status = &result["interfaces"][0]["settings"]["_runtime"]["rnode_multi"]
            ["radio_status"];

        assert_eq!(radio_status["selected_vport"].as_u64(), Some(2));
        assert_eq!(radio_status["stream_state"].as_str(), Some("configured"));
        assert!(radio_status["last_error"].is_null());
        assert_eq!(
            radio_status["vports"]
                .as_array()
                .expect("status vports")
                .iter()
                .filter_map(|value| value.as_u64())
                .collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert!(radio_status["subinterfaces"]["2"]["frequency_hz"].is_null());
    }

    #[test]
    fn lora_runtime_status_refresh_updates_matching_interface_record() {
        let daemon = RpcDaemon::test_instance();
        let runtime_iface = AddressHash::new([0x22; 16]);
        daemon.replace_interfaces(vec![InterfaceRecord {
            kind: "lora".to_string(),
            enabled: true,
            host: None,
            port: None,
            name: Some("rnode-main".to_string()),
            settings: Some(json!({
                "_runtime": {
                    "iface": runtime_iface.to_string(),
                    "startup_status": "spawned",
                    "lora": {
                        "rnode_status": {
                            "online": false
                        }
                    }
                }
            })),
        }]);
        let mut iface = rns_transport::iface::lora::LoraInterface::new(
            "COM9",
            115_200,
            rns_transport::iface::lora::LoraConfig::us915_default(),
        );
        iface
            .record_command_response(rns_transport::iface::lora::CMD_DETECT, &[
                rns_transport::iface::lora::DETECT_RESP,
            ])
            .expect("detect");
        iface
            .record_command_response(rns_transport::iface::lora::CMD_FW_VERSION, &[1, 52])
            .expect("firmware");
        iface
            .record_command_response(rns_transport::iface::lora::CMD_PLATFORM, &[
                rns_transport::iface::lora::PLATFORM_ESP32,
            ])
            .expect("platform");
        iface
            .record_command_response(rns_transport::iface::lora::CMD_MCU, &[0x01])
            .expect("mcu");
        let status = rns_transport::iface::lora::LoraRuntimeStatusHandle::new(Arc::new(
            Mutex::new(iface),
        ));
        let refresh = transport_startup::LoraRuntimeRefresh {
            runtime_iface,
            status: transport_startup::LoraRuntimeStatusSource::Lora(status),
        };

        assert_eq!(refresh_lora_runtime_status_once(&daemon, &[refresh]), 1);
        let result = daemon
            .handle_rpc(RpcRequest { id: 91, method: "daemon_status_ex".to_string(), params: None })
            .expect("daemon status")
            .result
            .expect("daemon status result");
        let status = &result["interfaces"][0]["settings"]["_runtime"]["lora"]["rnode_status"];

        assert_eq!(status["endpoint"].as_str(), Some("COM9"));
        assert_eq!(status["bearer"].as_str(), Some("serial"));
        assert_eq!(status["probe_status"]["detected"].as_bool(), Some(true));
        assert_eq!(status["probe_status"]["firmware_version"]["label"].as_str(), Some("1.52"));
    }

    #[cfg(feature = "vrn76-kiss-ble")]
    #[test]
    fn vrn76_runtime_status_refresh_updates_matching_interface_record() {
        let daemon = RpcDaemon::test_instance();
        let runtime_iface = AddressHash::new([0x24; 16]);
        daemon.replace_interfaces(vec![InterfaceRecord {
            kind: "vrn76_kiss_ble".to_string(),
            enabled: true,
            host: None,
            port: None,
            name: Some("vrn76-main".to_string()),
            settings: Some(json!({
                "_runtime": {
                    "iface": runtime_iface.to_string(),
                    "startup_status": "spawned",
                    "vrn76": {
                        "status": {
                            "connected": false
                        }
                    }
                }
            })),
        }]);
        let status = rns_transport::iface::vrn76_kiss_ble::Vrn76KissBleStatusHandle::new();
        status.update(rns_transport::iface::vrn76_kiss_ble::Vrn76KissBleStatus {
            connected: true,
            subscribed: true,
            interface_ready: true,
            startup_write_failures: 1,
            pending_payloads: 2,
            pending_writes: 3,
            pending_packets: 4,
        });
        let refresh = transport_startup::Vrn76RuntimeRefresh { runtime_iface, status };

        assert_eq!(refresh_vrn76_runtime_status_once(&daemon, &[refresh]), 1);
        let result = daemon
            .handle_rpc(RpcRequest { id: 96, method: "daemon_status_ex".to_string(), params: None })
            .expect("daemon status")
            .result
            .expect("daemon status result");
        let status = &result["interfaces"][0]["settings"]["_runtime"]["vrn76"]["status"];

        assert_eq!(status["connected"].as_bool(), Some(true));
        assert_eq!(status["subscribed"].as_bool(), Some(true));
        assert_eq!(status["interface_ready"].as_bool(), Some(true));
        assert_eq!(status["startup_write_failures"].as_u64(), Some(1));
        assert_eq!(status["pending_payloads"].as_u64(), Some(2));
        assert_eq!(status["pending_writes"].as_u64(), Some(3));
        assert_eq!(status["pending_packets"].as_u64(), Some(4));
    }

    #[cfg(feature = "rnode-ble")]
    #[test]
    fn rnode_ble_runtime_status_refresh_updates_matching_interface_record() {
        let daemon = RpcDaemon::test_instance();
        let runtime_iface = AddressHash::new([0x23; 16]);
        daemon.replace_interfaces(vec![InterfaceRecord {
            kind: "lora".to_string(),
            enabled: true,
            host: None,
            port: None,
            name: Some("rnode-ble".to_string()),
            settings: Some(json!({
                "_runtime": {
                    "iface": runtime_iface.to_string(),
                    "startup_status": "spawned",
                    "lora": {
                        "rnode_status": {
                            "online": false
                        }
                    }
                }
            })),
        }]);
        let status = std::sync::Arc::new(Mutex::new(serde_json::json!({
            "endpoint": "ble://RNode 1234",
            "bearer": "ble",
            "baud_rate": null,
            "probe_status": {
                "detected": true,
                "firmware_version": { "label": "1.52" },
                "platform": "ESP32",
                "mcu": "ESP32"
            },
            "radio_status": {
                "radio_state": 1
            },
            "online": true
        })));
        let refresh = transport_startup::LoraRuntimeRefresh {
            runtime_iface,
            status: transport_startup::LoraRuntimeStatusSource::RnodeBle(
                rns_transport::iface::rnode_ble::RnodeBleRuntimeStatusHandle::new(status),
            ),
        };

        assert_eq!(refresh_lora_runtime_status_once(&daemon, &[refresh]), 1);
        let result = daemon
            .handle_rpc(RpcRequest { id: 92, method: "daemon_status_ex".to_string(), params: None })
            .expect("daemon status")
            .result
            .expect("daemon status result");
        let status = &result["interfaces"][0]["settings"]["_runtime"]["lora"]["rnode_status"];

        assert_eq!(status["endpoint"].as_str(), Some("ble://RNode 1234"));
        assert_eq!(status["bearer"].as_str(), Some("ble"));
        assert!(status["baud_rate"].is_null());
        assert_eq!(status["online"].as_bool(), Some(true));
        assert_eq!(status["probe_status"]["detected"].as_bool(), Some(true));
    }
}
