use super::announce_worker::spawn_announce_worker;
use super::bridge::TransportBridge;
use super::inbound_worker::spawn_inbound_worker;
use super::interface_hot_apply::TcpInterfaceMutationBridge;
use super::outbound_resources::OutboundResourceMap;
use super::receipt_worker::spawn_receipt_worker;
use super::Args;
#[path = "bootstrap_transport.rs"]
mod transport_startup;
use reticulum_daemon::announce_names::{
    encode_delivery_announce_app_data_with_capabilities,
    encode_propagation_node_app_data as encode_python_propagation_node_app_data,
    normalize_capabilities, normalize_display_name, PropagationNodeAnnounceConfig,
};
use reticulum_daemon::config::{DaemonConfig, InterfaceConfig};
use reticulum_daemon::identity_store::load_or_create_identity;
use rns_rpc::{
    AnnounceBridge, InterfaceRecord, MessagesStore, OutboundBridge, RemoteControlBridge, RpcDaemon,
};
use rns_transport::destination::SingleInputDestination;
use rns_transport::transport::Transport;
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::HashMap;
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
    let propagation_control_enabled = env_flag("LXMD_PROPAGATION_NODE");
    let configured_control_identities = parse_hex_list_env("LXMD_CONTROL_ALLOWED");
    let peer_announce_at_start = env_flag("LXMD_PEER_ANNOUNCE_AT_START");
    let node_announce_at_start = env_flag("LXMD_NODE_ANNOUNCE_AT_START");
    let peer_announce_interval_secs = env_u64("LXMD_PEER_ANNOUNCE_INTERVAL_SECS");
    let node_announce_interval_secs = env_u64("LXMD_NODE_ANNOUNCE_INTERVAL_SECS");

    let startup = start_transport_and_interfaces(TransportStartupInput {
        args: &args,
        daemon_config: daemon_config.as_ref(),
        identity: &identity,
        reticulum_storage_path: reticulum_storage_path.as_path(),
        local_display_name: local_display_name.as_deref(),
        local_announce_capabilities: &local_announce_capabilities,
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
                encode_propagation_node_app_data(local_display_name.as_deref());
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
    daemon.replace_interfaces(configured_interfaces);
    daemon.set_propagation_state(transport.is_some(), None, 0);

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
                    encode_propagation_node_app_data(local_display_name.as_deref());
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
                        encode_propagation_node_app_data(local_display_name.as_deref());
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

pub(super) fn configure_startup_rpc_token_auth(args: &Args, daemon: &RpcDaemon) {
    let token_args = [
        args.rpc_token_issuer.as_ref().map(|_| "--rpc-token-issuer"),
        args.rpc_token_audience.as_ref().map(|_| "--rpc-token-audience"),
        args.rpc_token_secret_env.as_ref().map(|_| "--rpc-token-secret-env"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    if token_args.is_empty() {
        return;
    }
    let issuer = args
        .rpc_token_issuer
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| panic!("--rpc-token-issuer is required for startup token auth"));
    let audience = args
        .rpc_token_audience
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| panic!("--rpc-token-audience is required for startup token auth"));
    let secret_env = args
        .rpc_token_secret_env
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| panic!("--rpc-token-secret-env is required for startup token auth"));
    let shared_secret = std::env::var(secret_env)
        .unwrap_or_else(|_| panic!("startup token auth secret env var {secret_env} is not set"));
    if shared_secret.trim().is_empty() {
        panic!("startup token auth secret env var {secret_env} is empty");
    }

    daemon
        .configure_remote_token_auth_for_startup(
            issuer,
            audience,
            shared_secret,
            args.rpc_token_jti_ttl_ms,
            args.rpc_token_clock_skew_ms,
        )
        .unwrap_or_else(|err| panic!("invalid startup token auth configuration: {}", err.message));
}

fn interface_record_from_config(iface: &InterfaceConfig) -> InterfaceRecord {
    InterfaceRecord {
        kind: iface.kind.clone(),
        enabled: iface.enabled(),
        host: iface.host.clone(),
        port: iface.port,
        name: iface.name.clone(),
        settings: iface.settings_json(),
    }
}

#[derive(Debug, Default)]
pub(super) struct TcpServerSelection {
    pub(super) bind_addr: Option<String>,
    pub(super) selected_index: Option<usize>,
}

pub(super) fn select_tcp_server_bind(
    args: &Args,
    daemon_config: Option<&DaemonConfig>,
) -> Result<TcpServerSelection, String> {
    if let Some(addr) = args.transport.as_ref() {
        return Ok(TcpServerSelection { bind_addr: Some(addr.clone()), selected_index: None });
    }

    let Some(config) = daemon_config else {
        return Ok(TcpServerSelection::default());
    };

    let mut matches = Vec::new();
    for (index, iface) in config.interfaces.iter().enumerate() {
        if !iface.enabled() || iface.kind != "tcp_server" {
            continue;
        }
        let Some(port) = iface.port else {
            continue;
        };
        let host = iface
            .host
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("0.0.0.0");
        matches.push((index, format!("{}:{}", host, port)));
    }

    if matches.len() > 1 {
        return Err(format!(
            "multiple enabled tcp_server interfaces configured without --transport override: {}",
            matches.iter().map(|(_, endpoint)| endpoint.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }

    Ok(matches
        .into_iter()
        .next()
        .map(|(selected_index, bind_addr)| TcpServerSelection {
            bind_addr: Some(bind_addr),
            selected_index: Some(selected_index),
        })
        .unwrap_or_default())
}

pub(super) fn mark_interface_startup_status(
    record: &mut InterfaceRecord,
    status: &str,
    startup_error: Option<&str>,
    runtime_iface: Option<&str>,
) {
    with_interface_runtime_metadata(record, |runtime| {
        runtime.insert("startup_status".to_string(), JsonValue::String(status.to_string()));
        if let Some(startup_error) = startup_error {
            runtime
                .insert("startup_error".to_string(), JsonValue::String(startup_error.to_string()));
        } else {
            runtime.remove("startup_error");
        }
        if let Some(runtime_iface) = runtime_iface {
            runtime.insert("iface".to_string(), JsonValue::String(runtime_iface.to_string()));
        } else {
            runtime.remove("iface");
        }
    });
}

fn env_flag(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .map(|value| {
            matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

fn parse_hex_list_env(key: &str) -> Vec<String> {
    std::env::var(key)
        .ok()
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn env_capabilities(key: &str) -> Option<Vec<String>> {
    std::env::var(key)
        .ok()
        .map(|value| {
            let values = value
                .split([',', ';', ' ', '\t', '\r', '\n'])
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>();
            normalize_capabilities(&values)
        })
        .filter(|capabilities| !capabilities.is_empty())
}

fn env_u64(key: &str) -> Option<u64> {
    std::env::var(key)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
}

fn spawn_destination_announce_scheduler(
    transport: Arc<Transport>,
    destination: Arc<tokio::sync::Mutex<SingleInputDestination>>,
    app_data: Option<Vec<u8>>,
    interval_secs: u64,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
        loop {
            interval.tick().await;
            transport.send_announce(&destination, app_data.as_deref()).await;
        }
    });
}

fn spawn_bridge_announce_scheduler(bridge: Arc<TransportBridge>, interval_secs: u64) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
        loop {
            interval.tick().await;
            let _ = bridge.announce_now();
        }
    });
}

fn spawn_bridge_propagation_announce_scheduler(bridge: Arc<TransportBridge>, interval_secs: u64) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
        loop {
            interval.tick().await;
            let _ = bridge.announce_propagation_now();
        }
    });
}

fn encode_propagation_node_app_data(display_name: Option<&str>) -> Option<Vec<u8>> {
    encode_python_propagation_node_app_data(
        display_name,
        PropagationNodeAnnounceConfig {
            timebase: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
            ..PropagationNodeAnnounceConfig::default()
        },
    )
}

pub(super) fn mark_interface_runtime_managed(record: &mut InterfaceRecord, managed_by: &str) {
    with_interface_runtime_metadata(record, |runtime| {
        runtime.insert("managed_by".to_string(), JsonValue::String(managed_by.to_string()));
    });
}

pub(super) fn with_interface_runtime_metadata(
    record: &mut InterfaceRecord,
    update: impl FnOnce(&mut JsonMap<String, JsonValue>),
) {
    let mut settings = match record.settings.take() {
        Some(JsonValue::Object(existing)) => existing,
        Some(other) => {
            let mut wrapped = JsonMap::new();
            wrapped.insert("configured_settings".to_string(), other);
            wrapped
        }
        None => JsonMap::new(),
    };

    let runtime_value =
        settings.entry("_runtime".to_string()).or_insert_with(|| JsonValue::Object(JsonMap::new()));
    let runtime = match runtime_value {
        JsonValue::Object(existing) => existing,
        other => {
            *other = JsonValue::Object(JsonMap::new());
            match other {
                JsonValue::Object(existing) => existing,
                _ => unreachable!("runtime metadata must be an object"),
            }
        }
    };
    update(runtime);
    record.settings = Some(JsonValue::Object(settings));
}

pub(super) fn mark_interface_runtime_fields(
    record: &mut InterfaceRecord,
    runtime_status: &str,
    reconnect_attempts: u64,
) {
    let mut settings = match record.settings.take() {
        Some(JsonValue::Object(existing)) => existing,
        Some(other) => {
            let mut wrapped = JsonMap::new();
            wrapped.insert("configured_settings".to_string(), other);
            wrapped
        }
        None => JsonMap::new(),
    };

    let mut runtime = match settings.remove("_runtime") {
        Some(JsonValue::Object(existing)) => existing,
        _ => JsonMap::new(),
    };

    runtime.insert("runtime_status".to_string(), JsonValue::String(runtime_status.to_string()));
    runtime.insert("reconnect_attempts".to_string(), JsonValue::Number(reconnect_attempts.into()));
    settings.insert("_runtime".to_string(), JsonValue::Object(runtime));
    record.settings = Some(JsonValue::Object(settings));
}

pub(super) fn enforce_startup_policy(
    strict_interface_startup: bool,
    startup_failures: &[InterfaceStartupFailure],
) -> Result<(), String> {
    if !strict_interface_startup || startup_failures.is_empty() {
        return Ok(());
    }

    let details = startup_failures
        .iter()
        .map(|failure| format!("{} ({}): {}", failure.label, failure.kind, failure.error))
        .collect::<Vec<_>>()
        .join("; ");
    Err(format!(
        "strict interface startup policy rejected {} interface(s): {}",
        startup_failures.len(),
        details
    ))
}

async fn strict_tcp_client_preflight(endpoint: &str) -> Result<(), String> {
    let connect = timeout(Duration::from_secs(2), TcpStream::connect(endpoint))
        .await
        .map_err(|_| format!("tcp_client preflight connect timed out endpoint={endpoint}"))?;
    connect
        .map(|_| ())
        .map_err(|err| format!("tcp_client preflight connect failed endpoint={endpoint} err={err}"))
}
