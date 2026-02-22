use super::announce_worker::spawn_announce_worker;
use super::bridge::{PeerCrypto, TransportBridge};
use super::inbound_worker::spawn_inbound_worker;
use super::interfaces::{ble, common::interface_label, lora, serial};
use super::receipt_worker::spawn_receipt_worker;
use super::Args;
use reticulum_daemon::announce_names::{
    encode_delivery_display_name_app_data, normalize_display_name,
};
use reticulum_daemon::config::{DaemonConfig, InterfaceConfig};
use reticulum_daemon::identity_store::load_or_create_identity;
use reticulum_daemon::receipt_bridge::ReceiptBridge;
use rns_rpc::{AnnounceBridge, InterfaceRecord, MessagesStore, OutboundBridge, RpcDaemon};
use rns_transport::destination::{DestinationName, SingleInputDestination};
use rns_transport::iface::tcp_client::TcpClient;
use rns_transport::iface::tcp_server::TcpServer;
use rns_transport::transport::{Transport, TransportConfig};
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use tokio::net::TcpStream;
use tokio::sync::mpsc::unbounded_channel;
use tokio::time::{timeout, Duration};

#[derive(Clone, Debug)]
pub(super) struct RpcTlsConfig {
    pub(super) cert_chain_path: PathBuf,
    pub(super) private_key_path: PathBuf,
    pub(super) client_ca_path: Option<PathBuf>,
}

pub(super) struct BootstrapContext {
    pub(super) rpc_addr: SocketAddr,
    pub(super) daemon: Rc<RpcDaemon>,
    pub(super) rpc_tls: Option<RpcTlsConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct InterfaceStartupFailure {
    pub(super) label: String,
    pub(super) kind: String,
    pub(super) error: String,
}

pub(super) async fn bootstrap(args: Args) -> BootstrapContext {
    let rpc_addr: SocketAddr = args.rpc.parse().expect("invalid rpc address");
    let rpc_tls =
        match (args.rpc_tls_cert.clone(), args.rpc_tls_key.clone(), args.rpc_tls_client_ca.clone())
        {
            (None, None, None) => None,
            (Some(cert_chain_path), Some(private_key_path), client_ca_path) => {
                Some(RpcTlsConfig { cert_chain_path, private_key_path, client_ca_path })
            }
            (None, None, Some(_)) => {
                panic!("--rpc-tls-client-ca requires --rpc-tls-cert and --rpc-tls-key")
            }
            _ => panic!("--rpc-tls-cert and --rpc-tls-key must be provided together"),
        };
    let store = MessagesStore::open(&args.db).expect("open sqlite");

    let identity_path = args.identity.clone().unwrap_or_else(|| {
        let mut path = args.db.clone();
        path.set_extension("identity");
        path
    });
    let identity = load_or_create_identity(&identity_path).expect("load identity");
    let identity_hash = hex::encode(identity.address_hash().as_slice());
    let local_display_name =
        std::env::var("LXMF_DISPLAY_NAME").ok().and_then(|value| normalize_display_name(&value));
    let daemon_config = args.config.as_ref().and_then(|path| match DaemonConfig::from_path(path) {
        Ok(config) => Some(config),
        Err(err) => {
            eprintln!("[daemon] failed to load config {}: {}", path.display(), err);
            None
        }
    });
    let mut configured_interfaces = daemon_config
        .as_ref()
        .map(|config| {
            config.interfaces.iter().map(interface_record_from_config).collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut startup_successes = 0usize;
    let mut startup_failures: Vec<InterfaceStartupFailure> = Vec::new();

    if let Some(config) = daemon_config.as_ref() {
        for (index, iface) in config.interfaces.iter().enumerate() {
            if !iface.enabled() {
                mark_interface_startup_status(
                    &mut configured_interfaces[index],
                    "disabled",
                    None,
                    None,
                );
            }
        }
    }

    let mut transport: Option<Arc<Transport>> = None;
    let peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>> = Arc::new(Mutex::new(HashMap::new()));
    let mut announce_destination: Option<Arc<tokio::sync::Mutex<SingleInputDestination>>> = None;
    let mut delivery_destination_hash_hex: Option<String> = None;
    let mut delivery_source_hash = [0u8; 16];
    let receipt_map: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
    let (receipt_tx, receipt_rx) = unbounded_channel();

    if let Some(addr) = args.transport.clone() {
        let transport_identity =
            rns_transport::identity_bridge::to_transport_private_identity(&identity);
        let config = TransportConfig::new("daemon", &transport_identity, true);
        let mut transport_instance = Transport::new(config);
        transport_instance
            .set_receipt_handler(Box::new(ReceiptBridge::new(
                receipt_map.clone(),
                receipt_tx.clone(),
            )))
            .await;
        let iface_manager = transport_instance.iface_manager();
        let server_iface = iface_manager
            .lock()
            .await
            .spawn(TcpServer::new(addr.clone(), iface_manager.clone()), TcpServer::spawn);
        eprintln!("[daemon] tcp_server enabled iface={} bind={}", server_iface, addr);
        startup_successes += 1;
        if let Some(config) = daemon_config.as_ref() {
            for (index, iface) in config.interfaces.iter().enumerate() {
                if !iface.enabled() {
                    continue;
                }
                let label = interface_label(iface, index);
                match iface.kind.as_str() {
                    "tcp_client" => {
                        if let (Some(host), Some(port)) = (iface.host.as_ref(), iface.port) {
                            let endpoint = format!("{}:{}", host, port);
                            if args.strict_interface_startup {
                                if let Err(err) =
                                    strict_tcp_client_preflight(endpoint.as_str()).await
                                {
                                    eprintln!(
                                        "[daemon] tcp_client startup rejected name={} err={}",
                                        label, err
                                    );
                                    mark_interface_startup_status(
                                        &mut configured_interfaces[index],
                                        "failed",
                                        Some(err.as_str()),
                                        None,
                                    );
                                    startup_failures.push(InterfaceStartupFailure {
                                        label,
                                        kind: iface.kind.clone(),
                                        error: err,
                                    });
                                    continue;
                                }
                            }
                            let client_iface = iface_manager
                                .lock()
                                .await
                                .spawn(TcpClient::new(endpoint), TcpClient::spawn);
                            eprintln!(
                                "[daemon] tcp_client enabled iface={} name={} host={} port={}",
                                client_iface, label, host, port
                            );
                            let runtime_iface = client_iface.to_string();
                            mark_interface_startup_status(
                                &mut configured_interfaces[index],
                                "spawned",
                                None,
                                Some(runtime_iface.as_str()),
                            );
                            startup_successes += 1;
                        } else {
                            let err = "tcp_client requires host and port for startup".to_string();
                            eprintln!(
                                "[daemon] tcp_client startup rejected name={} err={}",
                                label, err
                            );
                            mark_interface_startup_status(
                                &mut configured_interfaces[index],
                                "failed",
                                Some(err.as_str()),
                                None,
                            );
                            startup_failures.push(InterfaceStartupFailure {
                                label,
                                kind: iface.kind.clone(),
                                error: err,
                            });
                        }
                    }
                    "serial" => match serial::build_adapter(iface) {
                        Ok(adapter) => {
                            if args.strict_interface_startup {
                                if let Err(err) = adapter.preflight_open() {
                                    eprintln!(
                                        "[daemon] serial startup rejected name={} err={}",
                                        label, err
                                    );
                                    mark_interface_startup_status(
                                        &mut configured_interfaces[index],
                                        "failed",
                                        Some(err.as_str()),
                                        None,
                                    );
                                    startup_failures.push(InterfaceStartupFailure {
                                        label,
                                        kind: iface.kind.clone(),
                                        error: err,
                                    });
                                    continue;
                                }
                            }
                            let serial_iface =
                                iface_manager.lock().await.spawn(adapter, |context| async move {
                                    rns_transport::iface::serial::SerialInterface::spawn(context)
                                        .await
                                });
                            eprintln!(
                                "[daemon] serial enabled iface={} name={} device={} baud_rate={}",
                                serial_iface,
                                label,
                                iface.device.as_deref().unwrap_or("<unset>"),
                                iface.baud_rate.unwrap_or_default()
                            );
                            let runtime_iface = serial_iface.to_string();
                            mark_interface_startup_status(
                                &mut configured_interfaces[index],
                                "spawned",
                                None,
                                Some(runtime_iface.as_str()),
                            );
                            startup_successes += 1;
                        }
                        Err(err) => {
                            eprintln!(
                                "[daemon] serial startup rejected name={} err={}",
                                label, err
                            );
                            mark_interface_startup_status(
                                &mut configured_interfaces[index],
                                "failed",
                                Some(err.as_str()),
                                None,
                            );
                            startup_failures.push(InterfaceStartupFailure {
                                label,
                                kind: iface.kind.clone(),
                                error: err,
                            });
                        }
                    },
                    "ble_gatt" => match ble::startup(iface) {
                        Ok(()) => {
                            mark_interface_startup_status(
                                &mut configured_interfaces[index],
                                "active",
                                None,
                                None,
                            );
                            startup_successes += 1;
                        }
                        Err(err) => {
                            eprintln!(
                                "[daemon] ble_gatt startup rejected name={} err={}",
                                label, err
                            );
                            mark_interface_startup_status(
                                &mut configured_interfaces[index],
                                "failed",
                                Some(err.as_str()),
                                None,
                            );
                            startup_failures.push(InterfaceStartupFailure {
                                label,
                                kind: iface.kind.clone(),
                                error: err,
                            });
                        }
                    },
                    "lora" => match lora::startup(iface) {
                        Ok(()) => {
                            mark_interface_startup_status(
                                &mut configured_interfaces[index],
                                "active",
                                None,
                                None,
                            );
                            startup_successes += 1;
                        }
                        Err(err) => {
                            eprintln!("[daemon] lora startup rejected name={} err={}", label, err);
                            mark_interface_startup_status(
                                &mut configured_interfaces[index],
                                "failed",
                                Some(err.as_str()),
                                None,
                            );
                            startup_failures.push(InterfaceStartupFailure {
                                label,
                                kind: iface.kind.clone(),
                                error: err,
                            });
                        }
                    },
                    _ => {
                        let err = format!("unsupported interface kind '{}'", iface.kind);
                        eprintln!("[daemon] interface startup rejected name={} err={}", label, err);
                        mark_interface_startup_status(
                            &mut configured_interfaces[index],
                            "failed",
                            Some(err.as_str()),
                            None,
                        );
                        startup_failures.push(InterfaceStartupFailure {
                            label,
                            kind: iface.kind.clone(),
                            error: err,
                        });
                    }
                }
            }
        }
        eprintln!("[daemon] transport enabled");
        if let Some((host, port)) = addr.rsplit_once(':') {
            let mut server_record = InterfaceRecord {
                kind: "tcp_server".into(),
                enabled: true,
                host: Some(host.to_string()),
                port: port.parse::<u16>().ok(),
                name: Some("daemon-transport".into()),
                settings: None,
            };
            let runtime_iface = server_iface.to_string();
            mark_interface_startup_status(
                &mut server_record,
                "active",
                None,
                Some(runtime_iface.as_str()),
            );
            configured_interfaces.push(server_record);
        }

        let destination = transport_instance
            .add_destination(transport_identity.clone(), DestinationName::new("lxmf", "delivery"))
            .await;
        {
            let dest = destination.lock().await;
            delivery_source_hash.copy_from_slice(dest.desc.address_hash.as_slice());
            delivery_destination_hash_hex = Some(hex::encode(dest.desc.address_hash.as_slice()));
            println!(
                "[daemon] delivery destination hash={}",
                hex::encode(dest.desc.address_hash.as_slice())
            );
        }
        announce_destination = Some(destination);
        transport = Some(Arc::new(transport_instance));
    } else if let Some(config) = daemon_config.as_ref() {
        for (index, iface) in config.interfaces.iter().enumerate() {
            if !iface.enabled() {
                continue;
            }
            let label = interface_label(iface, index);
            let err =
                "transport is disabled; start reticulumd with --transport to activate interfaces"
                    .to_string();
            mark_interface_startup_status(
                &mut configured_interfaces[index],
                "inactive_transport_disabled",
                Some(err.as_str()),
                None,
            );
            startup_failures.push(InterfaceStartupFailure {
                label,
                kind: iface.kind.clone(),
                error: err,
            });
        }
    }

    if !startup_failures.is_empty() {
        eprintln!(
            "[daemon] interface startup degraded started={} failed={} strict={}",
            startup_successes,
            startup_failures.len(),
            args.strict_interface_startup
        );
        for failure in &startup_failures {
            eprintln!(
                "[daemon] interface startup failure name={} kind={} err={}",
                failure.label, failure.kind, failure.error
            );
        }
    }

    if let Err(policy_error) =
        enforce_startup_policy(args.strict_interface_startup, &startup_failures)
    {
        panic!("{policy_error}");
    }

    let bridge: Option<Arc<TransportBridge>> =
        transport.as_ref().zip(announce_destination.as_ref()).map(|(transport, destination)| {
            Arc::new(TransportBridge::new(
                transport.clone(),
                identity.clone(),
                delivery_source_hash,
                destination.clone(),
                local_display_name
                    .as_ref()
                    .and_then(|display_name| encode_delivery_display_name_app_data(display_name)),
                peer_crypto.clone(),
                receipt_map.clone(),
                receipt_tx.clone(),
            ))
        });

    let outbound_bridge: Option<Arc<dyn OutboundBridge>> =
        bridge.as_ref().map(|bridge| bridge.clone() as Arc<dyn OutboundBridge>);
    let announce_bridge: Option<Arc<dyn AnnounceBridge>> =
        bridge.as_ref().map(|bridge| bridge.clone() as Arc<dyn AnnounceBridge>);

    let daemon = Rc::new(RpcDaemon::with_store_and_bridges(
        store,
        identity_hash,
        outbound_bridge,
        announce_bridge,
    ));
    daemon.set_delivery_destination_hash(delivery_destination_hash_hex);
    daemon.replace_interfaces(configured_interfaces);
    daemon.set_propagation_state(transport.is_some(), None, 0);

    // Make the local delivery destination visible on startup.
    if let Some(bridge) = bridge.as_ref() {
        let _ = bridge.announce_now();
    }

    if transport.is_some() {
        spawn_receipt_worker(daemon.clone(), receipt_rx);
    }

    if args.announce_interval_secs > 0 {
        let _handle = daemon.clone().start_announce_scheduler(args.announce_interval_secs);
    }

    if let Some(transport) = transport {
        spawn_inbound_worker(daemon.clone(), transport.clone());
        spawn_announce_worker(daemon.clone(), transport, peer_crypto);
    }

    BootstrapContext { rpc_addr, daemon, rpc_tls }
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

pub(super) fn mark_interface_startup_status(
    record: &mut InterfaceRecord,
    status: &str,
    startup_error: Option<&str>,
    runtime_iface: Option<&str>,
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

    let mut runtime = JsonMap::new();
    runtime.insert("startup_status".to_string(), JsonValue::String(status.to_string()));
    if let Some(startup_error) = startup_error {
        runtime.insert("startup_error".to_string(), JsonValue::String(startup_error.to_string()));
    }
    if let Some(runtime_iface) = runtime_iface {
        runtime.insert("iface".to_string(), JsonValue::String(runtime_iface.to_string()));
    }

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
