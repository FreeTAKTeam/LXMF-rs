use super::super::{
    mark_interface_runtime_fields, mark_interface_startup_status, strict_tcp_client_preflight,
    with_interface_runtime_metadata,
};
use super::{InterfaceStartupFailure, TcpServerSelection};
use crate::interface_hot_apply::tcp_interface_key;
use crate::interfaces::{
    auto, ble, common::interface_label, kiss, lora, serial, udp, vrn76_kiss_ble,
};
use crate::Args;
use reticulum_daemon::config::{DaemonConfig, InterfaceConfig};
use rns_rpc::InterfaceRecord;
use rns_transport::hash::AddressHash;
use rns_transport::iface::tcp_client::TcpClient;
use rns_transport::iface::udp::UdpInterface;
use rns_transport::iface::{IfaceRole, InterfaceMode};
use rns_transport::transport::Transport;
use std::sync::Arc;

pub(super) struct InterfaceStartupBatch {
    pub(super) startup_successes: usize,
    pub(super) startup_failures: Vec<InterfaceStartupFailure>,
    pub(super) seeded_tcp_interfaces: Vec<(String, InterfaceRecord, AddressHash)>,
    pub(super) tunnel_synth_ifaces: Vec<AddressHash>,
}

pub(super) async fn startup_configured_interfaces(
    args: &Args,
    config: &DaemonConfig,
    selected_tcp_server: &TcpServerSelection,
    transport: &Transport,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    server_iface: Option<&AddressHash>,
    configured_interfaces: &mut [InterfaceRecord],
) -> InterfaceStartupBatch {
    let mut startup_successes = 0usize;
    let mut startup_failures = Vec::new();
    let mut seeded_tcp_interfaces = Vec::new();
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
                        let mut manager = iface_manager.lock().await;
                        manager.set_mode(*active_iface, mode);
                        apply_interface_runtime_config(&mut manager, *active_iface, iface);
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
                    transport,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                )
                .await
                {
                    startup_successes += 1;
                }
            }
            "auto" => {
                if startup_auto(
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
            "kiss" => {
                if startup_kiss(
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
            "kiss_tcp_client" => {
                if startup_kiss_tcp_client(
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
            "vrn76_kiss_ble" => {
                if startup_vrn76_kiss_ble(
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
        log::warn!(
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
    let adapter = build_tcp_client_adapter(endpoint, iface);
    let client_iface = iface_manager.lock().await.spawn_as_with_mode(
        adapter,
        TcpClient::spawn,
        IfaceRole::Unicast,
        mode,
    );
    {
        let mut manager = iface_manager.lock().await;
        apply_interface_runtime_config(&mut manager, client_iface, iface);
    }
    log::info!(
        "[daemon] tcp_client enabled iface={} name={} host={} port={}",
        client_iface,
        label,
        host,
        port
    );
    let runtime_iface = client_iface.to_string();
    mark_interface_startup_status(record, "spawned", None, Some(runtime_iface.as_str()));
    if let Some(key) = tcp_interface_key(record) {
        seeded_tcp_interfaces.push((key, record.clone(), client_iface));
    }
    Some(client_iface)
}

fn build_tcp_client_adapter(endpoint: String, iface: &InterfaceConfig) -> TcpClient {
    let adapter = TcpClient::new(endpoint);
    if let Some(mtu) = iface.mtu {
        adapter.with_mtu(mtu)
    } else {
        adapter
    }
}

#[cfg(test)]
mod tests {
    use super::{build_tcp_client_adapter, startup_udp};
    use crate::Args;
    use reticulum_daemon::config::InterfaceConfig;
    use rns_rpc::InterfaceRecord;
    use rns_transport::hash::AddressHash;
    use rns_transport::identity_bridge::to_transport_private_identity;
    use rns_transport::iface::IfaceRole;
    use rns_transport::transport::{Transport, TransportConfig};
    use std::path::PathBuf;

    #[test]
    fn tcp_client_builder_uses_python_fixed_mtu() {
        let iface = InterfaceConfig {
            kind: "tcp_client".to_string(),
            enabled: Some(true),
            host: Some("rmap.world".to_string()),
            port: Some(4242),
            mtu: Some(4096),
            ..InterfaceConfig::default()
        };

        let adapter = build_tcp_client_adapter("rmap.world:4242".to_string(), &iface);

        assert_eq!(adapter.addr(), "rmap.world:4242");
        assert_eq!(adapter.mtu_value(), 4096);
    }

    #[tokio::test]
    async fn udp_startup_tags_multicast_config_as_multicast_role() {
        let args = test_args();
        let iface = InterfaceConfig {
            kind: "udp".to_string(),
            enabled: Some(true),
            name: Some("auto-style-multicast".to_string()),
            host: Some("239.255.0.1".to_string()),
            port: Some(0),
            target_host: Some("239.255.0.1".to_string()),
            target_port: Some(4242),
            ..InterfaceConfig::default()
        };
        let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let transport_identity = to_transport_private_identity(&identity);
        let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
        let manager = transport.iface_manager();
        let mut record = InterfaceRecord {
            kind: iface.kind.clone(),
            enabled: true,
            host: iface.host.clone(),
            port: iface.port,
            name: iface.name.clone(),
            settings: iface.settings_json(),
        };
        let mut startup_failures = Vec::new();

        let started = startup_udp(
            &args,
            &iface,
            "auto-style-multicast",
            &transport,
            &manager,
            &mut record,
            &mut startup_failures,
        )
        .await;

        assert!(started);
        assert!(startup_failures.is_empty());
        let runtime_iface = record
            .settings
            .as_ref()
            .and_then(|settings| settings.get("_runtime"))
            .and_then(|runtime| runtime.get("iface"))
            .and_then(|iface| iface.as_str())
            .expect("runtime iface");
        let runtime_iface =
            AddressHash::new_from_hex_string(runtime_iface.trim_matches('/')).expect("iface hash");
        assert_eq!(manager.lock().await.role(&runtime_iface), Some(IfaceRole::Multicast));
    }

    fn test_args() -> Args {
        Args {
            rpc: None,
            db: PathBuf::from("reticulum.db"),
            config: None,
            identity: None,
            announce_interval_secs: 0,
            transport: Some("127.0.0.1:0".to_string()),
            strict_interface_startup: false,
            rpc_tls_cert: None,
            rpc_tls_key: None,
            rpc_tls_client_ca: None,
            rpc_token_issuer: None,
            rpc_token_audience: None,
            rpc_token_secret_env: None,
            rpc_token_jti_ttl_ms: 60_000,
            rpc_token_clock_skew_ms: 5_000,
            rpc_unix: None,
            #[cfg(feature = "zmq-pipeline-rpc")]
            zmq_rpc_command: None,
        }
    }
}

async fn startup_udp(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    transport: &Transport,
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
    let adapter = UdpInterface::new(bind_addr.clone(), forward_addr.clone());
    let udp_iface = if adapter.is_multicast() {
        let udp_iface =
            transport.add_multicast_udp_interface(bind_addr.clone(), forward_addr.clone()).await;
        iface_manager.lock().await.set_mode(udp_iface, mode);
        udp_iface
    } else {
        iface_manager.lock().await.spawn_as_with_mode(
            adapter,
            UdpInterface::spawn,
            IfaceRole::Unicast,
            mode,
        )
    };
    {
        let mut manager = iface_manager.lock().await;
        apply_interface_runtime_config(&mut manager, udp_iface, iface);
    }
    log::info!(
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

async fn startup_auto(
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
) -> bool {
    match auto::build_native_startup_plan(iface) {
        Ok(plan) => {
            let adopted_count = plan.adopted_devices.len();
            let candidate_count = plan.candidates.len();
            with_interface_runtime_metadata(record, |runtime| {
                runtime.insert("auto".to_string(), plan.runtime_json());
            });
            let mode = iface.interface_mode().unwrap_or(InterfaceMode::Full);
            let (host_iface, transport_runtime) = {
                let mut manager = iface_manager.lock().await;
                let channel =
                    manager.new_channel_with_role_and_mode(128, IfaceRole::Multicast, mode);
                let host_iface = channel.address;
                apply_interface_runtime_config(&mut manager, host_iface, iface);
                (
                    host_iface,
                    auto::AutoInterfaceTransportRuntime::from_channel(
                        channel,
                        Arc::clone(iface_manager),
                    ),
                )
            };
            let runtime_iface = host_iface.to_string();
            match plan
                .spawn_discovery_runtime_with_native_scope_ids_and_transport(Some(
                    transport_runtime,
                ))
                .await
            {
                Ok(summary) => {
                    with_interface_runtime_metadata(record, |runtime| {
                        runtime.insert(
                            "auto_discovery_runtime".to_string(),
                            auto::discovery_runtime_summary_json(&summary),
                        );
                    });
                    log::info!(
                        "[daemon] auto enabled iface={} name={} discovery_loops={}/{} data_loops={}/{} initial_peer_announces={} repeat_schedulers={} peer_job_schedulers={} adopted={} candidates={}",
                        runtime_iface,
                        label,
                        summary.receive_loop_count,
                        summary.bound_socket_count,
                        summary.data_receive_loop_count,
                        summary.data_socket_count,
                        summary.initial_peer_announce_count,
                        summary.repeat_peer_announce_scheduler_count,
                        summary.peer_job_scheduler_count,
                        adopted_count,
                        candidate_count
                    );
                    mark_interface_startup_status(
                        record,
                        "spawned",
                        None,
                        Some(runtime_iface.as_str()),
                    );
                    mark_interface_runtime_fields(record, "running", 0);
                    true
                }
                Err(err) => {
                    let _ = iface_manager.lock().await.stop_interface(host_iface);
                    record_startup_failure(
                        record,
                        startup_failures,
                        label.to_string(),
                        iface.kind.clone(),
                        format!("AutoInterface discovery runtime startup failed: {err}"),
                    );
                    false
                }
            }
        }
        Err(err) => {
            record_startup_failure(
                record,
                startup_failures,
                label.to_string(),
                iface.kind.clone(),
                format!("AutoInterface OS interface discovery failed: {err}"),
            );
            false
        }
    }
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
    {
        let mut manager = iface_manager.lock().await;
        apply_interface_runtime_config(&mut manager, serial_iface, iface);
    }
    log::info!(
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

async fn startup_kiss(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
) -> bool {
    let adapter = match kiss::build_adapter(iface) {
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
    let kiss_iface = iface_manager.lock().await.spawn_as_with_mode(
        adapter,
        |context| async move { rns_transport::iface::kiss::KissInterface::spawn(context).await },
        IfaceRole::Unicast,
        mode,
    );
    {
        let mut manager = iface_manager.lock().await;
        apply_interface_runtime_config(&mut manager, kiss_iface, iface);
    }
    log::info!(
        "[daemon] kiss enabled iface={} name={} device={} baud_rate={}",
        kiss_iface,
        label,
        iface.device.as_deref().unwrap_or("<unset>"),
        iface.baud_rate.unwrap_or_default()
    );
    let runtime_iface = kiss_iface.to_string();
    mark_interface_startup_status(record, "spawned", None, Some(runtime_iface.as_str()));
    true
}

async fn startup_kiss_tcp_client(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
) -> bool {
    let adapter = match kiss::build_tcp_client_adapter(iface) {
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
        if let Err(err) = strict_tcp_client_preflight(adapter.addr()).await {
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
    let kiss_iface = iface_manager.lock().await.spawn_as_with_mode(
        adapter,
        |context| async move {
            rns_transport::iface::kiss::KissTcpClientInterface::spawn(context).await;
        },
        IfaceRole::Unicast,
        mode,
    );
    {
        let mut manager = iface_manager.lock().await;
        apply_interface_runtime_config(&mut manager, kiss_iface, iface);
    }
    log::info!(
        "[daemon] kiss_tcp_client enabled iface={} name={} endpoint={}:{}",
        kiss_iface,
        label,
        iface.host.as_deref().unwrap_or("<unset>"),
        iface.port.unwrap_or_default()
    );
    let runtime_iface = kiss_iface.to_string();
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
            let mut manager = iface_manager.lock().await;
            manager.set_mode(ble_iface, mode);
            apply_interface_runtime_config(&mut manager, ble_iface, iface);
            log::info!(
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

async fn startup_vrn76_kiss_ble(
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
) -> bool {
    let config = match vrn76_kiss_ble::build_config(iface) {
        Ok(config) => config,
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

    #[cfg(feature = "vrn76-kiss-ble")]
    {
        let adapter = vrn76_kiss_ble::build_native_interface(iface, config);
        let mode = iface.interface_mode().unwrap_or(InterfaceMode::Full);
        let vrn76_iface = iface_manager.lock().await.spawn_as_with_mode(
            adapter,
            |context| async move {
                rns_transport::iface::vrn76_kiss_ble::NativeVrn76KissBleInterface::spawn(context)
                    .await;
            },
            IfaceRole::Unicast,
            mode,
        );
        {
            let mut manager = iface_manager.lock().await;
            apply_interface_runtime_config(&mut manager, vrn76_iface, iface);
        }
        log::info!(
            "[daemon] vrn76_kiss_ble enabled iface={} name={} peripheral_id={}",
            vrn76_iface,
            label,
            iface.peripheral_id.as_deref().unwrap_or("<unset>")
        );
        let runtime_iface = vrn76_iface.to_string();
        mark_interface_startup_status(record, "spawned", None, Some(runtime_iface.as_str()));
        true
    }

    #[cfg(not(feature = "vrn76-kiss-ble"))]
    {
        let vrn76_kiss_ble::Vrn76KissBleDaemonConfig {
            peripheral_id,
            adapter,
            transport,
            reconnect_backoff,
            max_reconnect_backoff,
        } = config;
        let _ = (
            iface_manager,
            peripheral_id,
            adapter,
            transport,
            reconnect_backoff,
            max_reconnect_backoff,
        );
        record_startup_failure(
            record,
            startup_failures,
            label.to_string(),
            iface.kind.clone(),
            "vrn76_kiss_ble requires reticulumd feature vrn76-kiss-ble".to_string(),
        );
        false
    }
}

async fn startup_lora(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
) -> bool {
    if let Err(err) = lora::startup(iface) {
        record_startup_failure(
            record,
            startup_failures,
            label.to_string(),
            iface.kind.clone(),
            err,
        );
        return false;
    }

    if !lora::has_active_device(iface) {
        mark_interface_startup_status(record, "validated_startup_only", None, None);
        return true;
    }

    if iface.device.as_deref().is_some_and(lora::is_ble_rnode_port) {
        let config = match lora::build_rnode_ble_config(iface) {
            Ok(config) => config,
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
        #[cfg(not(feature = "rnode-ble"))]
        {
            let _ = (args, iface_manager);
            let lora::RnodeBleDaemonConfig {
                peripheral_id,
                adapter,
                lora,
                transport,
                startup_response_timeout,
                reconnect_backoff,
                max_reconnect_backoff,
            } = config;
            let _ = (
                peripheral_id,
                adapter,
                lora,
                transport,
                startup_response_timeout,
                reconnect_backoff,
                max_reconnect_backoff,
            );
            record_startup_failure(
                record,
                startup_failures,
                label.to_string(),
                iface.kind.clone(),
                "RNodeInterface ble:// requires reticulumd feature rnode-ble".to_string(),
            );
            return false;
        }
        #[cfg(feature = "rnode-ble")]
        {
            let adapter = lora::build_native_rnode_ble_interface(iface, config);
            let mode = iface.interface_mode().unwrap_or(InterfaceMode::Full);
            let rnode_iface = iface_manager.lock().await.spawn_as_with_mode(
                adapter,
                |context| async move {
                    rns_transport::iface::rnode_ble::NativeRnodeBleKissInterface::spawn(context)
                        .await;
                },
                IfaceRole::Unicast,
                mode,
            );
            {
                let mut manager = iface_manager.lock().await;
                apply_interface_runtime_config(&mut manager, rnode_iface, iface);
            }
            log::info!(
                "[daemon] rnode_ble enabled iface={} name={} device={}",
                rnode_iface,
                label,
                iface.device.as_deref().unwrap_or("<unset>")
            );
            let runtime_iface = rnode_iface.to_string();
            mark_interface_startup_status(record, "spawned", None, Some(runtime_iface.as_str()));
            return true;
        }
    }

    let adapter = match lora::build_adapter(iface) {
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
    let lora_iface = iface_manager.lock().await.spawn_as_with_mode(
        adapter,
        |context| async move { rns_transport::iface::lora::LoraInterface::spawn(context).await },
        IfaceRole::Unicast,
        mode,
    );
    {
        let mut manager = iface_manager.lock().await;
        apply_interface_runtime_config(&mut manager, lora_iface, iface);
    }
    log::info!(
        "[daemon] lora enabled iface={} name={} device={} baud_rate={}",
        lora_iface,
        label,
        iface.device.as_deref().unwrap_or("<unset>"),
        iface.baud_rate.unwrap_or_default()
    );
    let runtime_iface = lora_iface.to_string();
    mark_interface_startup_status(record, "spawned", None, Some(runtime_iface.as_str()));
    true
}

fn record_startup_failure(
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    label: String,
    kind: String,
    error: String,
) {
    log::error!("[daemon] interface startup rejected name={} err={}", label, error);
    mark_interface_startup_status(record, "failed", Some(error.as_str()), None);
    startup_failures.push(InterfaceStartupFailure { label, kind, error });
}

fn apply_interface_runtime_config(
    manager: &mut rns_transport::iface::InterfaceManager,
    address: AddressHash,
    iface: &InterfaceConfig,
) {
    manager.set_outgoing(address, iface.outgoing());
    if iface.bitrate.is_some() || iface.announce_cap.is_some() {
        let (current_bitrate, current_announce_cap) =
            manager.announce_pacing(&address).unwrap_or((62_500, 2));
        let bitrate = iface.bitrate.unwrap_or(current_bitrate);
        let announce_cap = iface.announce_cap.unwrap_or(current_announce_cap);
        manager.set_announce_pacing(address, bitrate, announce_cap);
    }
}
