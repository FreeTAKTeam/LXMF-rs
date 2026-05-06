use super::super::{
    mark_interface_runtime_fields, mark_interface_startup_status, strict_tcp_client_preflight,
};
use super::{InterfaceStartupFailure, TcpServerSelection};
use crate::interface_hot_apply::legacy_tcp_interface_key;
use crate::interfaces::{ble, common::interface_label, lora, serial, udp};
use crate::Args;
use reticulum_daemon::config::{DaemonConfig, InterfaceConfig};
use rns_rpc::InterfaceRecord;
use rns_transport::hash::AddressHash;
use rns_transport::iface::tcp_client::TcpClient;
use rns_transport::iface::udp::UdpInterface;
use rns_transport::iface::{IfaceRole, InterfaceMode};
use std::sync::Arc;

pub(super) struct InterfaceStartupBatch {
    pub(super) startup_successes: usize,
    pub(super) startup_failures: Vec<InterfaceStartupFailure>,
    pub(super) hot_apply_seeded_tcp: Vec<(String, InterfaceRecord, AddressHash)>,
    pub(super) tunnel_synth_ifaces: Vec<AddressHash>,
}

pub(super) async fn startup_configured_interfaces(
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
    let mut tunnel_synth_ifaces = Vec::new();

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
                if selected_tcp_server.selected_index == Some(index) {
                    if let Some(active_iface) = server_iface {
                        let mode = iface.interface_mode().unwrap_or(InterfaceMode::Full);
                        iface_manager.lock().await.set_mode(*active_iface, mode);
                    }
                }
            }
            "tcp_client" => {
                if let Some(client_iface) = startup_tcp_client(
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
                    tunnel_synth_ifaces.push(client_iface);
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

    InterfaceStartupBatch {
        startup_successes,
        startup_failures,
        hot_apply_seeded_tcp,
        tunnel_synth_ifaces,
    }
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
) -> Option<AddressHash> {
    let (Some(host), Some(port)) = (iface.host.as_ref(), iface.port) else {
        record_startup_failure(
            record,
            startup_failures,
            label.to_string(),
            iface.kind.clone(),
            "tcp_client requires host and port for startup".to_string(),
        );
        return None;
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
            return None;
        }
    }

    let mode = iface.interface_mode().unwrap_or(InterfaceMode::Full);
    let client_iface = iface_manager.lock().await.spawn_as_with_mode(
        TcpClient::new(endpoint),
        TcpClient::spawn,
        IfaceRole::Unicast,
        mode,
    );
    eprintln!(
        "[daemon] tcp_client enabled iface={} name={} host={} port={}",
        client_iface, label, host, port
    );
    let runtime_iface = client_iface.to_string();
    mark_interface_startup_status(record, "spawned", None, Some(runtime_iface.as_str()));
    if let Some(key) = legacy_tcp_interface_key(record) {
        hot_apply_seeded_tcp.push((key, record.clone(), client_iface));
    }
    Some(client_iface)
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

    let mode = iface.interface_mode().unwrap_or(InterfaceMode::Full);
    let udp_iface = iface_manager.lock().await.spawn_as_with_mode(
        UdpInterface::new(bind_addr.clone(), forward_addr.clone()),
        UdpInterface::spawn,
        IfaceRole::Unicast,
        mode,
    );
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

    let mode = iface.interface_mode().unwrap_or(InterfaceMode::Full);
    let serial_iface = iface_manager.lock().await.spawn_as_with_mode(
        adapter,
        |context| async move { rns_transport::iface::serial::SerialInterface::spawn(context).await },
        IfaceRole::Unicast,
        mode,
    );
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
            let mode = iface.interface_mode().unwrap_or(InterfaceMode::Full);
            iface_manager.lock().await.set_mode(ble_iface, mode);
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
