use reticulum_daemon::announce_names::{
    encode_delivery_announce_app_data_with_capabilities,
    encode_propagation_node_app_data as encode_python_propagation_node_app_data,
    normalize_capabilities, normalize_display_name, PropagationNodeAnnounceConfig,
};

use reticulum_daemon::config::{DaemonConfig, InterfaceConfig, PropagationNodeConfig};

use reticulum_daemon::identity_store::load_or_create_identity;

use rns_rpc::{
    AnnounceBridge, InterfaceRecord, MessagesStore, OutboundBridge, RemoteControlBridge, RpcDaemon,
    RpcRequest,
};

use rns_transport::destination::SingleInputDestination;

use rns_transport::hash::AddressHash;

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
    let propagation_node_config = daemon_config.as_ref().and_then(|config| {
        config.propagation_node.as_ref()
    });
    let propagation_control_enabled =
        env_flag("LXMD_PROPAGATION_NODE")
            || propagation_node_config.and_then(|config| config.enabled).unwrap_or(false);
    let configured_control_identities = parse_hex_list_env("LXMD_CONTROL_ALLOWED");
    let peer_announce_at_start = env_flag("LXMD_PEER_ANNOUNCE_AT_START")
        || propagation_node_config
            .and_then(|config| config.peer_announce_at_start)
            .unwrap_or(false);
    let node_announce_at_start = env_flag("LXMD_NODE_ANNOUNCE_AT_START")
        || propagation_node_config
            .and_then(|config| config.node_announce_at_start)
            .unwrap_or(false);
    let peer_announce_interval_secs = env_u64("LXMD_PEER_ANNOUNCE_INTERVAL_SECS")
        .or_else(|| propagation_node_config.and_then(|config| config.peer_announce_interval_secs));
    let node_announce_interval_secs = env_u64("LXMD_NODE_ANNOUNCE_INTERVAL_SECS")
        .or_else(|| propagation_node_config.and_then(|config| config.node_announce_interval_secs));
    let propagation_announce_config = propagation_announce_config_from_config(
        propagation_control_enabled,
        propagation_node_config,
    );
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
    if let Some(bridge) = bridge.as_ref() {
        bridge.set_daemon(daemon.clone());
        daemon.set_remote_control_bridge(bridge.clone() as Arc<dyn RemoteControlBridge>);
    }
    daemon.set_delivery_destination_hash(delivery_destination_hash_hex);
    daemon.set_propagation_destination_hash(propagation_destination_hash_hex.clone());
    daemon.replace_interfaces(configured_interfaces);
    if propagation_control_enabled {
        apply_startup_propagation_config(daemon.as_ref(), propagation_node_config);
    } else {
        daemon.set_propagation_state(false, None, 0);
    }

    // Make the local delivery destination visible on startup when configured.
    if peer_announce_at_start {
        if let Some(bridge) = bridge.as_ref() {
            let _ = bridge.announce_now();
        }
    }
    if let Some(interval_secs) = peer_announce_interval_secs {
        if let Some(bridge) = bridge.as_ref() {
            spawn_bridge_announce_scheduler(bridge.clone(), interval_secs);
        }
    }

    if propagation_control_enabled && node_announce_at_start {
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
    if let Some(interval_secs) = node_announce_interval_secs {
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
            },
            receipt_tx.clone(),
            outbound_resource_map,
        );
        spawn_announce_worker(daemon.clone(), transport, peer_crypto, Some(reticulum_storage_path));
    }

    BootstrapContext { rpc_addr, rpc_unix, daemon, rpc_tls }
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
