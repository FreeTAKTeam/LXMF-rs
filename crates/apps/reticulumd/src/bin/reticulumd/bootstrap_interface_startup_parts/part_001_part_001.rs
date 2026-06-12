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
