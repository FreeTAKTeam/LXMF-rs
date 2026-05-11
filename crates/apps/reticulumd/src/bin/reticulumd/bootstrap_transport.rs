use super::{
    encode_propagation_node_app_data, mark_interface_runtime_managed,
    mark_interface_startup_status, pretty_boot_line, pretty_daemon_line, pretty_warn_line,
    select_tcp_server_bind, InterfaceStartupFailure, TcpServerSelection,
};
#[path = "bootstrap_interface_startup.rs"]
mod interface_startup;
#[path = "bootstrap_transport_destinations.rs"]
mod transport_destinations;
use crate::bridge::PeerCrypto;
use crate::interface_worker_mode::{self, InterfaceWorkerBridgeHandle};
use crate::interfaces::common::interface_label;
use crate::Args;
use reticulum_daemon::config::DaemonConfig;
use reticulum_daemon::receipt_bridge::ReceiptBridge;
use rns_core::identity::PrivateIdentity;
use rns_rpc::InterfaceRecord;
use rns_transport::destination::SingleInputDestination;
use rns_transport::hash::AddressHash;
use rns_transport::iface::tcp_server::TcpServer;
use rns_transport::iface::{IfaceRole, InterfaceMode};
use rns_transport::transport::{worker_boundary::WorkerBackend, Transport, TransportConfig};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::time::Duration;
use tokio_util::sync::CancellationToken;

pub(super) struct TransportStartupArtifacts {
    pub(super) selected_tcp_server: TcpServerSelection,
    pub(super) transport: Option<Arc<Transport>>,
    pub(super) peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>>,
    pub(super) announce_destination: Option<Arc<tokio::sync::Mutex<SingleInputDestination>>>,
    pub(super) propagation_destination: Option<Arc<tokio::sync::Mutex<SingleInputDestination>>>,
    pub(super) control_destination: Option<Arc<tokio::sync::Mutex<SingleInputDestination>>>,
    pub(super) delivery_destination_hash_hex: Option<String>,
    pub(super) propagation_destination_hash_hex: Option<String>,
    pub(super) control_destination_hash_hex: Option<String>,
    pub(super) delivery_source_hash: [u8; 16],
    pub(super) configured_interfaces: Vec<InterfaceRecord>,
    pub(super) startup_successes: usize,
    pub(super) startup_failures: Vec<InterfaceStartupFailure>,
    pub(super) seeded_tcp_interfaces: Vec<(String, InterfaceRecord, AddressHash)>,
    pub(super) interface_worker_bridges: Vec<InterfaceWorkerBridgeHandle>,
}

pub(super) struct TransportStartupInput<'a> {
    pub(super) args: &'a Args,
    pub(super) daemon_config: Option<&'a DaemonConfig>,
    pub(super) identity: &'a PrivateIdentity,
    pub(super) reticulum_storage_path: &'a std::path::Path,
    pub(super) local_display_name: Option<&'a str>,
    pub(super) configured_interfaces: Vec<InterfaceRecord>,
    pub(super) receipt_map: Arc<Mutex<HashMap<String, String>>>,
    pub(super) receipt_tx:
        tokio::sync::mpsc::Sender<reticulum_daemon::receipt_bridge::ReceiptEvent>,
    pub(super) propagation_control_enabled: bool,
    pub(super) worker_process_backend: Option<Arc<dyn WorkerBackend>>,
}

pub(super) async fn start_transport_and_interfaces(
    input: TransportStartupInput<'_>,
) -> TransportStartupArtifacts {
    let TransportStartupInput {
        args,
        daemon_config,
        identity,
        reticulum_storage_path,
        local_display_name,
        mut configured_interfaces,
        receipt_map,
        receipt_tx,
        propagation_control_enabled,
        worker_process_backend,
    } = input;

    for record in &mut configured_interfaces {
        if !record.enabled {
            mark_interface_startup_status(record, "disabled", None, None);
        }
    }

    let selected_tcp_server = match select_tcp_server_bind(args, daemon_config) {
        Ok(selection) => selection,
        Err(err) => panic!("{err}"),
    };
    let has_enabled_configured_interface =
        daemon_config.is_some_and(|config| config.interfaces.iter().any(|iface| iface.enabled()));
    let transport_required = selected_tcp_server.bind_addr.is_some()
        || has_enabled_configured_interface
        || args.interface_worker_process_count > 0;

    let mut transport: Option<Arc<Transport>> = None;
    let peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>> = Arc::new(Mutex::new(HashMap::new()));
    let mut announce_destination: Option<Arc<tokio::sync::Mutex<SingleInputDestination>>> = None;
    let mut propagation_destination: Option<Arc<tokio::sync::Mutex<SingleInputDestination>>> = None;
    let mut control_destination: Option<Arc<tokio::sync::Mutex<SingleInputDestination>>> = None;
    let mut delivery_destination_hash_hex: Option<String> = None;
    let mut propagation_destination_hash_hex: Option<String> = None;
    let mut control_destination_hash_hex: Option<String> = None;
    let mut delivery_source_hash = [0u8; 16];
    let mut startup_successes = 0usize;
    let mut startup_failures = Vec::new();
    let mut seeded_tcp_interfaces = Vec::new();
    let mut interface_worker_bridges = Vec::new();

    if transport_required {
        if let Some(addr) = selected_tcp_server.bind_addr.as_ref() {
            eprintln!(
                "{}",
                pretty_boot_line(
                    "transport",
                    &format!("reticulumd transport listening on reticulum://{}", addr)
                )
            );
        }
        eprintln!("{}", pretty_daemon_line("transport enabled"));
        let transport_identity =
            rns_transport::identity_bridge::to_transport_private_identity(identity);
        let mut config = TransportConfig::new("daemon", &transport_identity, true);
        config.set_retransmit(true);
        if let Some(backend) = worker_process_backend.clone() {
            config.set_announce_worker_backend(backend.clone());
            config.set_outbound_worker_backend(backend.clone());
            config.set_single_destination_worker_backend(backend.clone());
            config.set_resource_worker_backend(backend);
        }
        let mut transport_instance = Transport::new(config);
        transport_instance
            .set_receipt_handler(Box::new(ReceiptBridge::new(receipt_map, receipt_tx.clone())))
            .await;
        let iface_manager = transport_instance.iface_manager();
        let mut server_iface = None;
        if let Some(addr) = selected_tcp_server.bind_addr.as_ref() {
            if args.interface_worker_process_count > 0 {
                let executable = super::interface_worker_process_executable_path(args);
                let channel_args = vec!["--interface-worker-tcp-listen".to_string(), addr.clone()];
                match interface_worker_mode::spawn_interface_worker_bridge_with_args(
                    iface_manager.clone(),
                    &executable,
                    channel_args,
                    IfaceRole::Unicast,
                    InterfaceMode::Full,
                    Duration::from_millis(args.interface_worker_process_shutdown_ms),
                    Duration::from_millis(args.interface_worker_process_restart_backoff_ms),
                    CancellationToken::new(),
                )
                .await
                {
                    Ok(handle) => {
                        eprintln!(
                            "[daemon] tcp_server interface worker enabled iface={} bind={} command={}",
                            handle.address,
                            addr,
                            executable.display()
                        );
                        startup_successes += 1;
                        server_iface = Some(handle.address);
                        interface_worker_bridges.push(handle);
                    }
                    Err(err) => {
                        startup_failures.push(InterfaceStartupFailure {
                            label: "tcp-server-process".into(),
                            kind: "tcp_server".into(),
                            error: format!("{err:?}"),
                        });
                    }
                }
            } else {
                let active_iface = iface_manager
                    .lock()
                    .await
                    .spawn(TcpServer::new(addr.clone(), iface_manager.clone()), TcpServer::spawn);
                eprintln!("[daemon] tcp_server enabled iface={} bind={}", active_iface, addr);
                startup_successes += 1;
                server_iface = Some(active_iface);
            }
        }

        if let Some(config) = daemon_config {
            let startup = interface_startup::startup_configured_interfaces(
                args,
                config,
                &selected_tcp_server,
                &iface_manager,
                server_iface.as_ref(),
                &mut configured_interfaces,
            )
            .await;
            startup_successes += startup.startup_successes;
            startup_failures.extend(startup.startup_failures);
            for iface in startup.tunnel_synth_ifaces {
                transport_instance.synthesize_tunnel_on_interface(iface).await;
            }
            seeded_tcp_interfaces.extend(startup.seeded_tcp_interfaces);
            interface_worker_bridges.extend(startup.interface_worker_bridges);
        }

        if args.interface_worker_process_count > 0 && interface_worker_bridges.is_empty() {
            let executable = super::interface_worker_process_executable_path(args);
            for index in 0..args.interface_worker_process_count {
                let channel_args = interface_worker_child_args(args);
                match interface_worker_mode::spawn_interface_worker_bridge_with_args(
                    iface_manager.clone(),
                    &executable,
                    channel_args,
                    IfaceRole::Unicast,
                    InterfaceMode::Full,
                    Duration::from_millis(args.interface_worker_process_shutdown_ms),
                    Duration::from_millis(args.interface_worker_process_restart_backoff_ms),
                    CancellationToken::new(),
                )
                .await
                {
                    Ok(handle) => {
                        eprintln!(
                            "[daemon] interface_worker_process enabled iface={} worker_index={} command={}",
                            handle.address,
                            index,
                            executable.display()
                        );
                        startup_successes += 1;
                        let mut record = InterfaceRecord {
                            kind: "interface_worker_process".into(),
                            enabled: true,
                            host: None,
                            port: None,
                            name: Some(format!("interface-worker-process-{index}")),
                            settings: None,
                        };
                        let runtime_iface = handle.address.to_string();
                        mark_interface_startup_status(
                            &mut record,
                            "spawned",
                            None,
                            Some(runtime_iface.as_str()),
                        );
                        mark_interface_runtime_managed(&mut record, "interface_worker_process");
                        configured_interfaces.push(record);
                        interface_worker_bridges.push(handle);
                    }
                    Err(err) => {
                        let label = format!("interface-worker-process-{index}");
                        startup_failures.push(InterfaceStartupFailure {
                            label: label.clone(),
                            kind: "interface_worker_process".into(),
                            error: format!("{err:?}"),
                        });
                        let mut record = InterfaceRecord {
                            kind: "interface_worker_process".into(),
                            enabled: true,
                            host: None,
                            port: None,
                            name: Some(label.clone()),
                            settings: None,
                        };
                        mark_interface_startup_status(
                            &mut record,
                            "failed",
                            Some(format!("{err:?}").as_str()),
                            None,
                        );
                        configured_interfaces.push(record);
                    }
                }
            }
        }

        match transport_instance.restore_reticulum_path_table(reticulum_storage_path).await {
            Ok(restored) if restored > 0 => {
                eprintln!("[daemon] restored {} Reticulum path table entries", restored);
            }
            Ok(_) => {}
            Err(err) => {
                eprintln!("[daemon] failed to restore Reticulum path table: {}", err);
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

        let destinations = transport_destinations::register_transport_destinations(
            &mut transport_instance,
            transport_identity.clone(),
            local_display_name,
            propagation_control_enabled,
        )
        .await;
        announce_destination = Some(destinations.delivery);
        propagation_destination = destinations.propagation;
        control_destination = destinations.control;
        delivery_destination_hash_hex = Some(destinations.delivery_destination_hash_hex);
        propagation_destination_hash_hex = destinations.propagation_destination_hash_hex;
        control_destination_hash_hex = destinations.control_destination_hash_hex;
        delivery_source_hash = destinations.delivery_source_hash;

        transport = Some(Arc::new(transport_instance));
    } else if let Some(config) = daemon_config {
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

    TransportStartupArtifacts {
        selected_tcp_server,
        transport,
        peer_crypto,
        announce_destination,
        propagation_destination,
        control_destination,
        delivery_destination_hash_hex,
        propagation_destination_hash_hex,
        control_destination_hash_hex,
        delivery_source_hash,
        configured_interfaces,
        startup_successes,
        startup_failures,
        seeded_tcp_interfaces,
        interface_worker_bridges,
    }
}

fn interface_worker_child_args(args: &Args) -> Vec<String> {
    let mut child_args = Vec::new();
    if let Some(bind_addr) = args.interface_worker_udp_bind.as_ref() {
        child_args.push("--interface-worker-udp-bind".to_string());
        child_args.push(bind_addr.clone());
    }
    if let Some(forward_addr) = args.interface_worker_udp_forward.as_ref() {
        child_args.push("--interface-worker-udp-forward".to_string());
        child_args.push(forward_addr.clone());
    }
    if let Some(connect_addr) = args.interface_worker_tcp_connect.as_ref() {
        child_args.push("--interface-worker-tcp-connect".to_string());
        child_args.push(connect_addr.clone());
    }
    child_args
}
