use super::{
    encode_propagation_node_app_data, mark_interface_runtime_fields,
    mark_interface_runtime_managed, mark_interface_startup_status, pretty_boot_line,
    pretty_daemon_line, pretty_warn_line, select_tcp_server_bind, strict_tcp_client_preflight,
    InterfaceStartupFailure, TcpServerSelection,
};
use crate::bridge::PeerCrypto;
use crate::interface_hot_apply::legacy_tcp_interface_key;
use crate::interfaces::{ble, common::interface_label, lora, serial, udp};
use crate::Args;
use reticulum_daemon::announce_names::encode_delivery_display_name_app_data;
use reticulum_daemon::config::{DaemonConfig, InterfaceConfig};
use reticulum_daemon::receipt_bridge::ReceiptBridge;
use rns_core::identity::PrivateIdentity;
use rns_rpc::InterfaceRecord;
use rns_transport::destination::{DestinationName, SingleInputDestination};
use rns_transport::hash::AddressHash;
use rns_transport::iface::tcp_client::TcpClient;
use rns_transport::iface::tcp_server::TcpServer;
use rns_transport::iface::udp::UdpInterface;
use rns_transport::transport::{Transport, TransportConfig};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

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
    pub(super) hot_apply_seeded_tcp: Vec<(String, InterfaceRecord, AddressHash)>,
}

pub(super) struct TransportStartupInput<'a> {
    pub(super) args: &'a Args,
    pub(super) daemon_config: Option<&'a DaemonConfig>,
    pub(super) identity: &'a PrivateIdentity,
    pub(super) local_display_name: Option<&'a str>,
    pub(super) configured_interfaces: Vec<InterfaceRecord>,
    pub(super) receipt_map: Arc<Mutex<HashMap<String, String>>>,
    pub(super) receipt_tx:
        tokio::sync::mpsc::UnboundedSender<reticulum_daemon::receipt_bridge::ReceiptEvent>,
    pub(super) propagation_control_enabled: bool,
}

pub(super) async fn start_transport_and_interfaces(
    input: TransportStartupInput<'_>,
) -> TransportStartupArtifacts {
    let TransportStartupInput {
        args,
        daemon_config,
        identity,
        local_display_name,
        mut configured_interfaces,
        receipt_map,
        receipt_tx,
        propagation_control_enabled,
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
    let transport_required = selected_tcp_server.bind_addr.is_some();

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
    let mut hot_apply_seeded_tcp = Vec::new();

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
            rns_transport::identity_bridge::to_transport_private_identity(identity);
        let mut config = TransportConfig::new("daemon", &transport_identity, true);
        config.set_retransmit(true);
        let mut transport_instance = Transport::new(config);
        transport_instance
            .set_receipt_handler(Box::new(ReceiptBridge::new(receipt_map, receipt_tx.clone())))
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

        if let Some(config) = daemon_config {
            let startup = startup_configured_interfaces(
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
            hot_apply_seeded_tcp.extend(startup.hot_apply_seeded_tcp);
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
        transport_instance
            .set_destination_announce_app_data(
                announce_destination.as_ref().expect("delivery destination"),
                local_display_name.and_then(encode_delivery_display_name_app_data),
            )
            .await;

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
            transport_instance
                .set_destination_announce_app_data(
                    propagation_destination.as_ref().expect("propagation destination"),
                    encode_propagation_node_app_data(local_display_name),
                )
                .await;

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
        hot_apply_seeded_tcp,
    }
}

struct InterfaceStartupBatch {
    startup_successes: usize,
    startup_failures: Vec<InterfaceStartupFailure>,
    hot_apply_seeded_tcp: Vec<(String, InterfaceRecord, AddressHash)>,
}

async fn startup_configured_interfaces(
    args: &Args,
    config: &DaemonConfig,
    selected_tcp_server: &TcpServerSelection,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    server_iface: Option<&AddressHash>,
    configured_interfaces: &mut [InterfaceRecord],
) -> InterfaceStartupBatch {
    let mut startup_successes = 0usize;
    let mut startup_failures = Vec::new();
    let mut hot_apply_seeded_tcp = Vec::new();

    for (index, iface) in config.interfaces.iter().enumerate() {
        if !iface.enabled() {
            continue;
        }
        let label = interface_label(iface, index);
        match iface.kind.as_str() {
            "tcp_server" => {
                startup_tcp_server_record(
                    index,
                    iface,
                    &label,
                    selected_tcp_server,
                    server_iface,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                );
            }
            "tcp_client" => {
                if startup_tcp_client(
                    args,
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                    &mut hot_apply_seeded_tcp,
                )
                .await
                {
                    startup_successes += 1;
                }
            }
            "udp" => {
                if startup_udp(
                    args,
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                )
                .await
                {
                    startup_successes += 1;
                }
            }
            "serial" => {
                if startup_serial(
                    args,
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                )
                .await
                {
                    startup_successes += 1;
                }
            }
            "ble_gatt" => {
                if startup_ble(
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                )
                .await
                {
                    startup_successes += 1;
                }
            }
            "lora" => {
                if startup_lora(
                    iface,
                    &label,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                ) {
                    startup_successes += 1;
                }
            }
            _ => record_startup_failure(
                &mut configured_interfaces[index],
                &mut startup_failures,
                label,
                iface.kind.clone(),
                format!("unsupported interface kind '{}'", iface.kind),
            ),
        }
    }

    InterfaceStartupBatch { startup_successes, startup_failures, hot_apply_seeded_tcp }
}

fn startup_tcp_server_record(
    index: usize,
    iface: &InterfaceConfig,
    label: &str,
    selected_tcp_server: &TcpServerSelection,
    server_iface: Option<&AddressHash>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
) {
    let selected_for_startup = selected_tcp_server.selected_index == Some(index);
    if !selected_for_startup {
        mark_interface_startup_status(
            record,
            "shadowed_by_transport_override",
            Some("tcp_server ignored because --transport selected the active bind endpoint"),
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
        return;
    }

    if iface.port.is_none() {
        record_startup_failure(
            record,
            startup_failures,
            label.to_string(),
            iface.kind.clone(),
            "tcp_server requires port for startup".to_string(),
        );
        return;
    }
    let runtime_iface = server_iface.map(ToString::to_string);
    mark_interface_startup_status(record, "active", None, runtime_iface.as_deref());
}

async fn startup_tcp_client(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    hot_apply_seeded_tcp: &mut Vec<(String, InterfaceRecord, AddressHash)>,
) -> bool {
    let (Some(host), Some(port)) = (iface.host.as_ref(), iface.port) else {
        record_startup_failure(
            record,
            startup_failures,
            label.to_string(),
            iface.kind.clone(),
            "tcp_client requires host and port for startup".to_string(),
        );
        return false;
    };

    let endpoint = format!("{}:{}", host, port);
    if args.strict_interface_startup {
        if let Err(err) = strict_tcp_client_preflight(endpoint.as_str()).await {
            record_startup_failure(
                record,
                startup_failures,
                label.to_string(),
                iface.kind.clone(),
                err,
            );
            return false;
        }
    }

    let client_iface = iface_manager.lock().await.spawn(TcpClient::new(endpoint), TcpClient::spawn);
    eprintln!(
        "[daemon] tcp_client enabled iface={} name={} host={} port={}",
        client_iface, label, host, port
    );
    let runtime_iface = client_iface.to_string();
    mark_interface_startup_status(record, "spawned", None, Some(runtime_iface.as_str()));
    if let Some(key) = legacy_tcp_interface_key(record) {
        hot_apply_seeded_tcp.push((key, record.clone(), client_iface));
    }
    true
}

async fn startup_udp(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
) -> bool {
    let (bind_addr, forward_addr) = match udp::bind_and_forward_addr(iface) {
        Ok(addrs) => addrs,
        Err(err) => {
            record_startup_failure(
                record,
                startup_failures,
                label.to_string(),
                iface.kind.clone(),
                err,
            );
            return false;
        }
    };

    if args.strict_interface_startup {
        if let Err(err) = udp::strict_preflight(bind_addr.as_str()).await {
            record_startup_failure(
                record,
                startup_failures,
                label.to_string(),
                iface.kind.clone(),
                err,
            );
            return false;
        }
    }

    let udp_iface = iface_manager
        .lock()
        .await
        .spawn(UdpInterface::new(bind_addr.clone(), forward_addr.clone()), UdpInterface::spawn);
    eprintln!(
        "[daemon] udp enabled iface={} name={} bind={} forward={}",
        udp_iface,
        label,
        bind_addr,
        forward_addr.as_deref().unwrap_or("<none>")
    );
    let runtime_iface = udp_iface.to_string();
    mark_interface_startup_status(record, "spawned", None, Some(runtime_iface.as_str()));
    true
}

async fn startup_serial(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
) -> bool {
    let adapter = match serial::build_adapter(iface) {
        Ok(adapter) => adapter,
        Err(err) => {
            record_startup_failure(
                record,
                startup_failures,
                label.to_string(),
                iface.kind.clone(),
                err,
            );
            return false;
        }
    };

    if args.strict_interface_startup {
        if let Err(err) = adapter.preflight_open() {
            record_startup_failure(
                record,
                startup_failures,
                label.to_string(),
                iface.kind.clone(),
                err,
            );
            return false;
        }
    }

    let serial_iface = iface_manager.lock().await.spawn(adapter, |context| async move {
        rns_transport::iface::serial::SerialInterface::spawn(context).await
    });
    eprintln!(
        "[daemon] serial enabled iface={} name={} device={} baud_rate={}",
        serial_iface,
        label,
        iface.device.as_deref().unwrap_or("<unset>"),
        iface.baud_rate.unwrap_or_default()
    );
    let runtime_iface = serial_iface.to_string();
    mark_interface_startup_status(record, "spawned", None, Some(runtime_iface.as_str()));
    true
}

async fn startup_ble(
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
) -> bool {
    match ble::spawn(iface_manager.clone(), iface).await {
        Ok(ble_iface) => {
            eprintln!(
                "[daemon] ble_gatt enabled iface={} name={} peripheral_id={}",
                ble_iface,
                label,
                iface.peripheral_id.as_deref().unwrap_or("<unset>")
            );
            let runtime_iface = ble_iface.to_string();
            mark_interface_startup_status(record, "spawned", None, Some(runtime_iface.as_str()));
            mark_interface_runtime_fields(record, "running", 0);
            true
        }
        Err(err) => {
            record_startup_failure(
                record,
                startup_failures,
                label.to_string(),
                iface.kind.clone(),
                err,
            );
            mark_interface_runtime_fields(record, "degraded", 0);
            false
        }
    }
}

fn startup_lora(
    iface: &InterfaceConfig,
    label: &str,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
) -> bool {
    match lora::startup(iface) {
        Ok(()) => {
            mark_interface_startup_status(record, "validated_startup_only", None, None);
            true
        }
        Err(err) => {
            record_startup_failure(
                record,
                startup_failures,
                label.to_string(),
                iface.kind.clone(),
                err,
            );
            false
        }
    }
}

fn record_startup_failure(
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    label: String,
    kind: String,
    error: String,
) {
    eprintln!("[daemon] interface startup rejected name={} err={}", label, error);
    mark_interface_startup_status(record, "failed", Some(error.as_str()), None);
    startup_failures.push(InterfaceStartupFailure { label, kind, error });
}
