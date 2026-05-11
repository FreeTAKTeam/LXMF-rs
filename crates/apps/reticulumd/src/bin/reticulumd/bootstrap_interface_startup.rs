use super::super::{
    interface_worker_process_executable_path, mark_interface_runtime_fields,
    mark_interface_runtime_managed, mark_interface_startup_status, strict_tcp_client_preflight,
};
use super::{InterfaceStartupFailure, TcpServerSelection};
use crate::interface_hot_apply::tcp_interface_key;
use crate::interface_worker_mode::{self, InterfaceWorkerBridgeHandle};
use crate::interfaces::{ble, common::interface_label, lora, serial, udp};
use crate::Args;
use reticulum_daemon::config::{DaemonConfig, InterfaceConfig};
use rns_rpc::InterfaceRecord;
use rns_transport::hash::AddressHash;
use rns_transport::iface::tcp_client::TcpClient;
use rns_transport::iface::udp::UdpInterface;
use rns_transport::iface::{IfaceRole, InterfaceMode};
use std::sync::Arc;
use tokio::time::Duration;
use tokio_util::sync::CancellationToken;

pub(super) struct InterfaceStartupBatch {
    pub(super) startup_successes: usize,
    pub(super) startup_failures: Vec<InterfaceStartupFailure>,
    pub(super) seeded_tcp_interfaces: Vec<(String, InterfaceRecord, AddressHash)>,
    pub(super) tunnel_synth_ifaces: Vec<AddressHash>,
    pub(super) interface_worker_bridges: Vec<InterfaceWorkerBridgeHandle>,
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
    let mut seeded_tcp_interfaces = Vec::new();
    let mut tunnel_synth_ifaces = Vec::new();
    let mut interface_worker_bridges = Vec::new();

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
                    &mut seeded_tcp_interfaces,
                    &mut interface_worker_bridges,
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
                    &mut interface_worker_bridges,
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
                    &mut interface_worker_bridges,
                )
                .await
                {
                    startup_successes += 1;
                }
            }
            "ble_gatt" => {
                if startup_ble(
                    args,
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                    &mut interface_worker_bridges,
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
        seeded_tcp_interfaces,
        tunnel_synth_ifaces,
        interface_worker_bridges,
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
    seeded_tcp_interfaces: &mut Vec<(String, InterfaceRecord, AddressHash)>,
    interface_worker_bridges: &mut Vec<InterfaceWorkerBridgeHandle>,
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
    if args.interface_worker_process_count > 0 {
        let executable = interface_worker_process_executable_path(args);
        let child_args = vec!["--interface-worker-tcp-connect".to_string(), endpoint.clone()];
        match interface_worker_mode::spawn_interface_worker_bridge_with_args(
            iface_manager.clone(),
            executable.clone(),
            child_args,
            IfaceRole::Unicast,
            mode,
            Duration::from_millis(args.interface_worker_process_shutdown_ms),
            Duration::from_millis(args.interface_worker_process_restart_backoff_ms),
            CancellationToken::new(),
        )
        .await
        {
            Ok(handle) => {
                eprintln!(
                    "[daemon] tcp_client interface worker enabled iface={} name={} endpoint={} command={}",
                    handle.address,
                    label,
                    endpoint,
                    executable.display(),
                );
                let client_iface = handle.address;
                let runtime_iface = client_iface.to_string();
                mark_interface_startup_status(
                    record,
                    "spawned_process",
                    None,
                    Some(runtime_iface.as_str()),
                );
                mark_interface_runtime_managed(record, "interface_worker_process");
                if let Some(key) = tcp_interface_key(record) {
                    seeded_tcp_interfaces.push((key, record.clone(), client_iface));
                }
                interface_worker_bridges.push(handle);
                return Some(client_iface);
            }
            Err(err) => {
                record_startup_failure(
                    record,
                    startup_failures,
                    label.to_string(),
                    iface.kind.clone(),
                    format!("{err:?}"),
                );
                return None;
            }
        }
    }

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
    if let Some(key) = tcp_interface_key(record) {
        seeded_tcp_interfaces.push((key, record.clone(), client_iface));
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
    interface_worker_bridges: &mut Vec<InterfaceWorkerBridgeHandle>,
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
    if args.interface_worker_process_count > 0 {
        let executable = interface_worker_process_executable_path(args);
        let mut child_args = vec!["--interface-worker-udp-bind".to_string(), bind_addr.clone()];
        if let Some(forward_addr) = forward_addr.clone() {
            child_args.push("--interface-worker-udp-forward".to_string());
            child_args.push(forward_addr);
        }
        match interface_worker_mode::spawn_interface_worker_bridge_with_args(
            iface_manager.clone(),
            executable.clone(),
            child_args,
            IfaceRole::Unicast,
            mode,
            Duration::from_millis(args.interface_worker_process_shutdown_ms),
            Duration::from_millis(args.interface_worker_process_restart_backoff_ms),
            CancellationToken::new(),
        )
        .await
        {
            Ok(handle) => {
                eprintln!(
                    "[daemon] udp interface worker enabled iface={} name={} bind={} forward={} command={}",
                    handle.address,
                    label,
                    bind_addr,
                    forward_addr.as_deref().unwrap_or("<none>"),
                    executable.display(),
                );
                let runtime_iface = handle.address.to_string();
                mark_interface_startup_status(
                    record,
                    "spawned_process",
                    None,
                    Some(runtime_iface.as_str()),
                );
                mark_interface_runtime_managed(record, "interface_worker_process");
                interface_worker_bridges.push(handle);
                return true;
            }
            Err(err) => {
                record_startup_failure(
                    record,
                    startup_failures,
                    label.to_string(),
                    iface.kind.clone(),
                    format!("{err:?}"),
                );
                return false;
            }
        }
    }

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
    interface_worker_bridges: &mut Vec<InterfaceWorkerBridgeHandle>,
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
    if args.interface_worker_process_count > 0 {
        let executable = interface_worker_process_executable_path(args);
        let child_args = serial_interface_child_args(iface);
        match interface_worker_mode::spawn_interface_worker_bridge_with_args(
            iface_manager.clone(),
            executable.clone(),
            child_args,
            IfaceRole::Unicast,
            mode,
            Duration::from_millis(args.interface_worker_process_shutdown_ms),
            Duration::from_millis(args.interface_worker_process_restart_backoff_ms),
            CancellationToken::new(),
        )
        .await
        {
            Ok(handle) => {
                eprintln!(
                    "[daemon] serial interface worker enabled iface={} name={} device={} baud_rate={} command={}",
                    handle.address,
                    label,
                    iface.device.as_deref().unwrap_or("<unset>"),
                    iface.baud_rate.unwrap_or_default(),
                    executable.display(),
                );
                let runtime_iface = handle.address.to_string();
                mark_interface_startup_status(
                    record,
                    "spawned_process",
                    None,
                    Some(runtime_iface.as_str()),
                );
                mark_interface_runtime_managed(record, "interface_worker_process");
                interface_worker_bridges.push(handle);
                return true;
            }
            Err(err) => {
                record_startup_failure(
                    record,
                    startup_failures,
                    label.to_string(),
                    iface.kind.clone(),
                    format!("{err:?}"),
                );
                return false;
            }
        }
    }

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

fn serial_interface_child_args(iface: &InterfaceConfig) -> Vec<String> {
    let mut child_args = Vec::new();
    if let Some(device) = iface.device.as_ref() {
        child_args.push("--interface-worker-serial-device".to_string());
        child_args.push(device.clone());
    }
    if let Some(baud_rate) = iface.baud_rate {
        child_args.push("--interface-worker-serial-baud-rate".to_string());
        child_args.push(baud_rate.to_string());
    }
    if let Some(data_bits) = iface.data_bits {
        child_args.push("--interface-worker-serial-data-bits".to_string());
        child_args.push(data_bits.to_string());
    }
    if let Some(stop_bits) = iface.stop_bits {
        child_args.push("--interface-worker-serial-stop-bits".to_string());
        child_args.push(stop_bits.to_string());
    }
    if let Some(parity) = iface.parity.as_ref() {
        child_args.push("--interface-worker-serial-parity".to_string());
        child_args.push(parity.clone());
    }
    if let Some(flow_control) = iface.flow_control.as_ref() {
        child_args.push("--interface-worker-serial-flow-control".to_string());
        child_args.push(flow_control.clone());
    }
    if let Some(mtu) = iface.mtu {
        child_args.push("--interface-worker-serial-mtu".to_string());
        child_args.push(mtu.to_string());
    }
    if let Some(reconnect_backoff_ms) = iface.reconnect_backoff_ms {
        child_args.push("--interface-worker-serial-reconnect-backoff-ms".to_string());
        child_args.push(reconnect_backoff_ms.to_string());
    }
    if let Some(max_reconnect_backoff_ms) = iface.max_reconnect_backoff_ms {
        child_args.push("--interface-worker-serial-max-reconnect-backoff-ms".to_string());
        child_args.push(max_reconnect_backoff_ms.to_string());
    }
    child_args
}

async fn startup_ble(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    interface_worker_bridges: &mut Vec<InterfaceWorkerBridgeHandle>,
) -> bool {
    let mode = iface.interface_mode().unwrap_or(InterfaceMode::Full);
    if args.interface_worker_process_count > 0 {
        let executable = interface_worker_process_executable_path(args);
        let child_args = ble_interface_child_args(iface);
        match interface_worker_mode::spawn_interface_worker_bridge_with_args(
            iface_manager.clone(),
            executable.clone(),
            child_args,
            IfaceRole::Unicast,
            mode,
            Duration::from_millis(args.interface_worker_process_shutdown_ms),
            Duration::from_millis(args.interface_worker_process_restart_backoff_ms),
            CancellationToken::new(),
        )
        .await
        {
            Ok(handle) => {
                eprintln!(
                    "[daemon] ble_gatt interface worker enabled iface={} name={} peripheral_id={} command={}",
                    handle.address,
                    label,
                    iface.peripheral_id.as_deref().unwrap_or("<unset>"),
                    executable.display(),
                );
                let runtime_iface = handle.address.to_string();
                mark_interface_startup_status(
                    record,
                    "spawned_process",
                    None,
                    Some(runtime_iface.as_str()),
                );
                mark_interface_runtime_managed(record, "interface_worker_process");
                mark_interface_runtime_fields(record, "running", 0);
                interface_worker_bridges.push(handle);
                return true;
            }
            Err(err) => {
                record_startup_failure(
                    record,
                    startup_failures,
                    label.to_string(),
                    iface.kind.clone(),
                    format!("{err:?}"),
                );
                mark_interface_runtime_fields(record, "degraded", 0);
                return false;
            }
        }
    }

    match ble::spawn(iface_manager.clone(), iface).await {
        Ok(ble_iface) => {
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

fn ble_interface_child_args(iface: &InterfaceConfig) -> Vec<String> {
    let mut child_args = Vec::new();
    if let Some(adapter) = iface.adapter.as_ref() {
        child_args.push("--interface-worker-ble-adapter".to_string());
        child_args.push(adapter.clone());
    }
    if let Some(peripheral_id) = iface.peripheral_id.as_ref() {
        child_args.push("--interface-worker-ble-peripheral-id".to_string());
        child_args.push(peripheral_id.clone());
    }
    if let Some(service_uuid) = iface.service_uuid.as_ref() {
        child_args.push("--interface-worker-ble-service-uuid".to_string());
        child_args.push(service_uuid.clone());
    }
    if let Some(write_char_uuid) = iface.write_char_uuid.as_ref() {
        child_args.push("--interface-worker-ble-write-char-uuid".to_string());
        child_args.push(write_char_uuid.clone());
    }
    if let Some(notify_char_uuid) = iface.notify_char_uuid.as_ref() {
        child_args.push("--interface-worker-ble-notify-char-uuid".to_string());
        child_args.push(notify_char_uuid.clone());
    }
    if let Some(mtu) = iface.mtu {
        child_args.push("--interface-worker-ble-mtu".to_string());
        child_args.push(mtu.to_string());
    }
    if let Some(scan_timeout_ms) = iface.scan_timeout_ms {
        child_args.push("--interface-worker-ble-scan-timeout-ms".to_string());
        child_args.push(scan_timeout_ms.to_string());
    }
    if let Some(connect_timeout_ms) = iface.connect_timeout_ms {
        child_args.push("--interface-worker-ble-connect-timeout-ms".to_string());
        child_args.push(connect_timeout_ms.to_string());
    }
    if let Some(reconnect_backoff_ms) = iface.reconnect_backoff_ms {
        child_args.push("--interface-worker-ble-reconnect-backoff-ms".to_string());
        child_args.push(reconnect_backoff_ms.to_string());
    }
    if let Some(max_reconnect_backoff_ms) = iface.max_reconnect_backoff_ms {
        child_args.push("--interface-worker-ble-max-reconnect-backoff-ms".to_string());
        child_args.push(max_reconnect_backoff_ms.to_string());
    }
    child_args
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
