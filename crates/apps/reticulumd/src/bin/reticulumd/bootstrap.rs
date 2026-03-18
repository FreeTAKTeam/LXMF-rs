use super::announce_worker::spawn_announce_worker;
use super::bridge::{PeerCrypto, TransportBridge};
use super::inbound_worker::spawn_inbound_worker;
use super::interface_hot_apply::{legacy_tcp_interface_key, LegacyTcpInterfaceMutationBridge};
use super::interfaces::{ble, common::interface_label, lora, serial, udp};
use super::receipt_worker::spawn_receipt_worker;
use super::Args;
use reticulum_daemon::announce_names::{
    encode_delivery_display_name_app_data, normalize_display_name,
};
use reticulum_daemon::config::{DaemonConfig, InterfaceConfig};
use reticulum_daemon::identity_store::load_or_create_identity;
use reticulum_daemon::receipt_bridge::ReceiptBridge;
use rns_rpc::{
    AnnounceBridge, InterfaceRecord, MessagesStore, OutboundBridge, RemoteControlBridge, RpcDaemon,
};
use rns_transport::destination::{DestinationName, SingleInputDestination};
use rns_transport::iface::tcp_client::TcpClient;
use rns_transport::iface::tcp_server::TcpServer;
use rns_transport::iface::udp::UdpInterface;
use rns_transport::transport::{Transport, TransportConfig};
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::HashMap;
use std::io::IsTerminal;
use std::net::SocketAddr;
use std::path::PathBuf;
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
    pub(super) grpc_addr: Option<SocketAddr>,
    pub(super) daemon: Arc<RpcDaemon>,
    pub(super) rpc_tls: Option<RpcTlsConfig>,
    pub(super) grpc_tls: Option<RpcTlsConfig>,
}

#[derive(Clone, Debug)]
pub(super) struct PropagationControlContext {
    pub(super) enabled: bool,
    pub(super) propagation_destination_hash_hex: Option<String>,
    pub(super) control_destination_hash_hex: Option<String>,
    pub(super) allowed_control_identities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct InterfaceStartupFailure {
    pub(super) label: String,
    pub(super) kind: String,
    pub(super) error: String,
}

pub(super) async fn bootstrap(args: Args) -> BootstrapContext {
    let rpc_addr: SocketAddr = args.rpc.parse().expect("invalid rpc address");
    let grpc_addr: Option<SocketAddr> =
        args.grpc.as_ref().map(|addr| addr.parse().expect("invalid grpc address"));
    let rpc_tls = parse_tls_args(
        "--rpc-tls-cert",
        "--rpc-tls-key",
        "--rpc-tls-client-ca",
        args.rpc_tls_cert.clone(),
        args.rpc_tls_key.clone(),
        args.rpc_tls_client_ca.clone(),
    );
    let grpc_tls = parse_tls_args(
        "--grpc-tls-cert",
        "--grpc-tls-key",
        "--grpc-tls-client-ca",
        args.grpc_tls_cert.clone(),
        args.grpc_tls_key.clone(),
        args.grpc_tls_client_ca.clone(),
    );
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
    let mut propagation_destination: Option<Arc<tokio::sync::Mutex<SingleInputDestination>>> = None;
    let mut control_destination: Option<Arc<tokio::sync::Mutex<SingleInputDestination>>> = None;
    let mut delivery_destination_hash_hex: Option<String> = None;
    let mut propagation_destination_hash_hex: Option<String> = None;
    let mut control_destination_hash_hex: Option<String> = None;
    let mut delivery_source_hash = [0u8; 16];
    let receipt_map: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
    let outbound_resource_map: Arc<Mutex<HashMap<String, String>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let (receipt_tx, receipt_rx) = unbounded_channel();
    let mut hot_apply_seeded_tcp: Vec<(String, InterfaceRecord, rns_transport::hash::AddressHash)> =
        Vec::new();
    let propagation_control_enabled = env_flag("LXMD_PROPAGATION_NODE");
    let configured_control_identities = parse_hex_list_env("LXMD_CONTROL_ALLOWED");
    let peer_announce_at_start = env_flag("LXMD_PEER_ANNOUNCE_AT_START");
    let node_announce_at_start = env_flag("LXMD_NODE_ANNOUNCE_AT_START");
    let peer_announce_interval_secs = env_u64("LXMD_PEER_ANNOUNCE_INTERVAL_SECS");
    let node_announce_interval_secs = env_u64("LXMD_NODE_ANNOUNCE_INTERVAL_SECS");

    let selected_tcp_server = match select_tcp_server_bind(&args, daemon_config.as_ref()) {
        Ok(selection) => selection,
        Err(err) => {
            panic!("{err}");
        }
    };
    let transport_required = selected_tcp_server.bind_addr.is_some();

    if transport_required {
        if let Some(addr) = selected_tcp_server.bind_addr.as_ref() {
            println!(
                "{}",
                pretty_boot_line(
                    "transport",
                    &format!("reticulumd transport listening on reticulum://{}", addr)
                )
            );
        }
        println!("{}", pretty_daemon_line("transport enabled"));
        let transport_identity =
            rns_transport::identity_bridge::to_transport_private_identity(&identity);
        let mut config = TransportConfig::new("daemon", &transport_identity, true);
        // Central tcp_server topologies depend on the daemon relaying announces and path
        // responses between connected peers, which is gated by transport retransmit mode.
        config.set_retransmit(true);
        let mut transport_instance = Transport::new(config);
        transport_instance
            .set_receipt_handler(Box::new(ReceiptBridge::new(
                receipt_map.clone(),
                receipt_tx.clone(),
            )))
            .await;
        let iface_manager = transport_instance.iface_manager();
        let mut server_iface = None;
        if let Some(addr) = selected_tcp_server.bind_addr.as_ref() {
            let active_iface = iface_manager
                .lock()
                .await
                .spawn(TcpServer::new(addr.clone(), iface_manager.clone()), TcpServer::spawn);
            eprintln!("[daemon] tcp_server enabled iface={} bind={}", active_iface, addr);
            startup_successes += 1;
            server_iface = Some(active_iface);
        }
        if let Some(config) = daemon_config.as_ref() {
            for (index, iface) in config.interfaces.iter().enumerate() {
                if !iface.enabled() {
                    continue;
                }
                let label = interface_label(iface, index);
                match iface.kind.as_str() {
                    "tcp_server" => {
                        let selected_for_startup =
                            selected_tcp_server.selected_index == Some(index);
                        if !selected_for_startup {
                            mark_interface_startup_status(
                                &mut configured_interfaces[index],
                                "shadowed_by_transport_override",
                                Some(
                                    "tcp_server ignored because --transport selected the active bind endpoint",
                                ),
                                None,
                            );
                            let endpoint = iface
                                .port
                                .map(|port| {
                                    let host = iface
                                        .host
                                        .as_deref()
                                        .map(str::trim)
                                        .filter(|value| !value.is_empty())
                                        .unwrap_or("0.0.0.0");
                                    format!("{}:{}", host, port)
                                })
                                .unwrap_or_else(|| "<missing-port>".to_string());
                            eprintln!(
                                "[daemon] tcp_server startup skipped name={} endpoint={} selected={}",
                                label,
                                endpoint,
                                selected_tcp_server.bind_addr.as_deref().unwrap_or("<none>")
                            );
                            continue;
                        }

                        if iface.port.is_none() {
                            let err = "tcp_server requires port for startup".to_string();
                            eprintln!(
                                "[daemon] tcp_server startup rejected name={} err={}",
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
                        mark_interface_startup_status(
                            &mut configured_interfaces[index],
                            "active",
                            None,
                            server_iface.as_ref().map(ToString::to_string).as_deref(),
                        );
                    }
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
                            if let Some(key) =
                                legacy_tcp_interface_key(&configured_interfaces[index])
                            {
                                hot_apply_seeded_tcp.push((
                                    key,
                                    configured_interfaces[index].clone(),
                                    client_iface,
                                ));
                            }
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
                    "udp" => match udp::bind_and_forward_addr(iface) {
                        Ok((bind_addr, forward_addr)) => {
                            if args.strict_interface_startup {
                                if let Err(err) = udp::strict_preflight(bind_addr.as_str()).await {
                                    eprintln!(
                                        "[daemon] udp startup rejected name={} err={}",
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
                            let udp_iface = iface_manager.lock().await.spawn(
                                UdpInterface::new(bind_addr.clone(), forward_addr.clone()),
                                UdpInterface::spawn,
                            );
                            eprintln!(
                                "[daemon] udp enabled iface={} name={} bind={} forward={}",
                                udp_iface,
                                label,
                                bind_addr,
                                forward_addr.as_deref().unwrap_or("<none>")
                            );
                            let runtime_iface = udp_iface.to_string();
                            mark_interface_startup_status(
                                &mut configured_interfaces[index],
                                "spawned",
                                None,
                                Some(runtime_iface.as_str()),
                            );
                            startup_successes += 1;
                        }
                        Err(err) => {
                            eprintln!("[daemon] udp startup rejected name={} err={}", label, err);
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
                    "ble_gatt" => match ble::spawn(iface_manager.clone(), iface).await {
                        Ok(ble_iface) => {
                            eprintln!(
                                "[daemon] ble_gatt enabled iface={} name={} peripheral_id={}",
                                ble_iface,
                                label,
                                iface.peripheral_id.as_deref().unwrap_or("<unset>")
                            );
                            let runtime_iface = ble_iface.to_string();
                            mark_interface_startup_status(
                                &mut configured_interfaces[index],
                                "spawned",
                                None,
                                Some(runtime_iface.as_str()),
                            );
                            mark_interface_runtime_fields(
                                &mut configured_interfaces[index],
                                "running",
                                0,
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
                            mark_interface_runtime_fields(
                                &mut configured_interfaces[index],
                                "degraded",
                                0,
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
                                "validated_startup_only",
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
        if selected_tcp_server.selected_index.is_none() {
            if let (Some(addr), Some(active_iface)) =
                (selected_tcp_server.bind_addr.as_ref(), server_iface.as_ref())
            {
                let (host, port) = addr.rsplit_once(':').unwrap_or(("0.0.0.0", "0"));
                let mut server_record = InterfaceRecord {
                    kind: "tcp_server".into(),
                    enabled: true,
                    host: Some(host.to_string()),
                    port: port.parse::<u16>().ok(),
                    name: Some("daemon-transport".into()),
                    settings: None,
                };
                let runtime_iface = active_iface.to_string();
                mark_interface_startup_status(
                    &mut server_record,
                    "active",
                    None,
                    Some(runtime_iface.as_str()),
                );
                mark_interface_runtime_managed(&mut server_record, "daemon_transport");
                configured_interfaces.push(server_record);
            }
        }

        let destination = transport_instance
            .add_destination(transport_identity.clone(), DestinationName::new("lxmf", "delivery"))
            .await;
        {
            let dest = destination.lock().await;
            delivery_source_hash.copy_from_slice(dest.desc.address_hash.as_slice());
            delivery_destination_hash_hex = Some(hex::encode(dest.desc.address_hash.as_slice()));
            println!(
                "{}",
                pretty_daemon_line(&format!(
                    "delivery destination hash={}",
                    hex::encode(dest.desc.address_hash.as_slice())
                ))
            );
        }
        announce_destination = Some(destination);
        if propagation_control_enabled {
            let propagation = transport_instance
                .add_destination(
                    transport_identity.clone(),
                    DestinationName::new("lxmf", "propagation"),
                )
                .await;
            {
                let dest = propagation.lock().await;
                propagation_destination_hash_hex =
                    Some(hex::encode(dest.desc.address_hash.as_slice()));
                println!(
                    "{}",
                    pretty_daemon_line(&format!(
                        "propagation destination hash={}",
                        hex::encode(dest.desc.address_hash.as_slice())
                    ))
                );
            }
            propagation_destination = Some(propagation);

            let control = transport_instance
                .add_destination(
                    transport_identity.clone(),
                    DestinationName::new("lxmf", "propagation.control"),
                )
                .await;
            {
                let dest = control.lock().await;
                control_destination_hash_hex = Some(hex::encode(dest.desc.address_hash.as_slice()));
                println!(
                    "{}",
                    pretty_daemon_line(&format!(
                        "control destination hash={}",
                        hex::encode(dest.desc.address_hash.as_slice())
                    ))
                );
            }
            control_destination = Some(control);
        }
        transport = Some(Arc::new(transport_instance));
    } else if let Some(config) = daemon_config.as_ref() {
        eprintln!(
            "{}",
            pretty_warn_line(
                "transport disabled; configured interfaces will remain inactive until you start reticulumd with --transport HOST:PORT"
            )
        );
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

    let grpc_summary =
        grpc_addr.map(|addr| addr.to_string()).unwrap_or_else(|| "disabled".to_string());
    let transport_summary =
        selected_tcp_server.bind_addr.clone().unwrap_or_else(|| "disabled".to_string());
    println!(
        "{}",
        pretty_boot_line(
            "startup",
            &format!(
                "reticulumd startup summary: rpc={} grpc={} transport={} interfaces={} identity={}",
                rpc_addr,
                grpc_summary,
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
                local_display_name
                    .as_ref()
                    .and_then(|display_name| encode_delivery_display_name_app_data(display_name)),
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
    if let Some(transport) = transport.as_ref() {
        daemon.set_interface_mutation_bridge(Arc::new(LegacyTcpInterfaceMutationBridge::spawn(
            transport.iface_manager(),
            hot_apply_seeded_tcp,
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
    if let Some((transport, destination, interval_secs)) =
        transport.as_ref().zip(announce_destination.as_ref()).zip(peer_announce_interval_secs).map(
            |((transport, destination), interval_secs)| (transport, destination, interval_secs),
        )
    {
        spawn_destination_announce_scheduler(
            transport.clone(),
            destination.clone(),
            None,
            interval_secs,
        );
    }

    if propagation_control_enabled && node_announce_at_start {
        if let Some((transport, destination)) =
            transport.as_ref().zip(propagation_destination.as_ref())
        {
            let propagation_app_data =
                encode_propagation_node_app_data(local_display_name.as_deref());
            transport.send_announce(destination, propagation_app_data.as_deref()).await;
        }
        if let Some((transport, destination)) = transport.as_ref().zip(control_destination.as_ref())
        {
            transport.send_announce(destination, None).await;
        }
    }
    if let Some(interval_secs) = node_announce_interval_secs {
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
        if let Some((transport, destination)) = transport.as_ref().zip(control_destination.as_ref())
        {
            spawn_destination_announce_scheduler(
                transport.clone(),
                destination.clone(),
                None,
                interval_secs,
            );
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
                propagation_destination_hash_hex,
                control_destination_hash_hex,
                allowed_control_identities: configured_control_identities,
            },
            receipt_tx.clone(),
            outbound_resource_map,
        );
        spawn_announce_worker(daemon.clone(), transport, peer_crypto);
    }

    BootstrapContext { rpc_addr, grpc_addr, daemon, rpc_tls, grpc_tls }
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

fn encode_propagation_node_app_data(display_name: Option<&str>) -> Option<Vec<u8>> {
    let mut metadata = Vec::new();
    if let Some(name) = display_name {
        metadata.push((rmpv::Value::from(1_i64), rmpv::Value::Binary(name.as_bytes().to_vec())));
    }
    let announce_data = rmpv::Value::Array(vec![
        rmpv::Value::Boolean(false),
        rmpv::Value::from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
        ),
        rmpv::Value::Boolean(true),
        rmpv::Value::from(256_i64),
        rmpv::Value::from(10240_i64),
        rmpv::Value::Array(vec![
            rmpv::Value::from(16_i64),
            rmpv::Value::from(3_i64),
            rmpv::Value::from(18_i64),
        ]),
        rmpv::Value::Map(metadata),
    ]);
    rmp_serde::to_vec(&announce_data).ok()
}

pub(super) fn mark_interface_runtime_managed(record: &mut InterfaceRecord, managed_by: &str) {
    with_interface_runtime_metadata(record, |runtime| {
        runtime.insert("managed_by".to_string(), JsonValue::String(managed_by.to_string()));
    });
}

fn with_interface_runtime_metadata(
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
