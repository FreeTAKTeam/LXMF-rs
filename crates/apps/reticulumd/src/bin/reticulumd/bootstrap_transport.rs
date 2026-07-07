use super::{
    encode_propagation_node_app_data, mark_interface_runtime_managed,
    mark_interface_startup_status, pretty_boot_line, pretty_daemon_line, pretty_warn_line,
    select_tcp_server_bind, InterfaceStartupFailure, TcpServerSelection,
};
#[path = "bootstrap_interface_startup.rs"]
mod interface_startup;
#[path = "bootstrap_transport_path_restore.rs"]
mod path_restore;
#[path = "bootstrap_transport_destinations.rs"]
mod transport_destinations;
use crate::bridge::PeerCrypto;
use crate::interfaces::common::interface_label;
use crate::Args;
#[cfg(test)]
pub(super) use interface_startup::LoraRuntimeStatusSource;
#[cfg(feature = "vrn76-kiss-ble")]
pub(super) use interface_startup::Vrn76RuntimeRefresh;
pub(super) use interface_startup::{
    AutoRuntimeRefresh, BleGattRuntimeRefresh, I2pRuntimeRefresh, KissRuntimeRefresh,
    LoraRuntimeRefresh, PipeRuntimeRefresh, RNodeManagementBinding, RNodeMultiRuntimeRefresh,
    ReticulumBleRuntimeRefresh, SerialRuntimeRefresh, TcpRuntimeRefresh, TcpRuntimeStatusSource,
    UdpRuntimeRefresh, WeaveControlBinding, WeaveRuntimeRefresh,
};
use path_restore::{
    mark_path_table_restore_status, mark_path_table_restore_status_on_enabled_interfaces,
    PathTableRestoreStatus,
};
use reticulum_daemon::announce_names::PropagationNodeAnnounceConfig;
use reticulum_daemon::config::DaemonConfig;
use reticulum_daemon::receipt_bridge::ReceiptBridge;
use rns_core::identity::PrivateIdentity;
use rns_rpc::InterfaceRecord;
use rns_transport::destination::SingleInputDestination;
use rns_transport::hash::AddressHash;
use rns_transport::iface::tcp_client::TcpSocketTuning;
use rns_transport::iface::tcp_server::TcpServer;
use rns_transport::transport::{RestoredReticulumPathIdentity, Transport, TransportConfig};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const STREAM_RECONNECT_EVENT_CHANNEL_CAPACITY: usize = 32;

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
    pub(super) restored_path_identities: Vec<RestoredReticulumPathIdentity>,
    pub(super) startup_successes: usize,
    pub(super) startup_failures: Vec<InterfaceStartupFailure>,
    pub(super) seeded_hot_apply_interfaces: Vec<(String, InterfaceRecord, AddressHash)>,
    pub(super) auto_runtime_refreshes: Vec<AutoRuntimeRefresh>,
    pub(super) pipe_runtime_refreshes: Vec<PipeRuntimeRefresh>,
    pub(super) udp_runtime_refreshes: Vec<UdpRuntimeRefresh>,
    pub(super) serial_runtime_refreshes: Vec<SerialRuntimeRefresh>,
    pub(super) kiss_runtime_refreshes: Vec<KissRuntimeRefresh>,
    pub(super) ble_gatt_runtime_refreshes: Vec<BleGattRuntimeRefresh>,
    pub(super) reticulum_ble_runtime_refreshes: Vec<ReticulumBleRuntimeRefresh>,
    pub(super) i2p_runtime_refreshes: Vec<I2pRuntimeRefresh>,
    pub(super) tcp_runtime_refreshes: Vec<TcpRuntimeRefresh>,
    pub(super) weave_runtime_refreshes: Vec<WeaveRuntimeRefresh>,
    pub(super) rnode_multi_runtime_refreshes: Vec<RNodeMultiRuntimeRefresh>,
    pub(super) lora_runtime_refreshes: Vec<LoraRuntimeRefresh>,
    #[cfg(feature = "vrn76-kiss-ble")]
    pub(super) vrn76_runtime_refreshes: Vec<Vrn76RuntimeRefresh>,
    pub(super) rnode_management_bindings: Vec<RNodeManagementBinding>,
    pub(super) weave_control_bindings: Vec<WeaveControlBinding>,
}

pub(super) struct TransportStartupInput<'a> {
    pub(super) args: &'a Args,
    pub(super) daemon_config: Option<&'a DaemonConfig>,
    pub(super) identity: &'a PrivateIdentity,
    pub(super) reticulum_storage_path: &'a std::path::Path,
    pub(super) local_display_name: Option<&'a str>,
    pub(super) local_announce_capabilities: &'a [String],
    pub(super) propagation_announce_app_data: Option<Vec<u8>>,
    pub(super) configured_interfaces: Vec<InterfaceRecord>,
    pub(super) receipt_map: Arc<Mutex<HashMap<String, String>>>,
    pub(super) receipt_tx:
        tokio::sync::mpsc::Sender<reticulum_daemon::receipt_bridge::ReceiptEvent>,
    pub(super) propagation_control_enabled: bool,
    pub(super) propagation_announce_config: PropagationNodeAnnounceConfig,
}

fn spawn_stream_reconnect_tunnel_synthesizer(
    transport: Arc<Transport>,
    mut reconnect_rx: tokio::sync::mpsc::Receiver<AddressHash>,
) {
    tokio::spawn(async move {
        while let Some(iface) = reconnect_rx.recv().await {
            if transport.synthesize_tunnel_on_interface(iface).await {
                log::info!("[daemon] stream reconnect synthesized tunnel iface={}", iface);
            } else {
                log::warn!("[daemon] stream reconnect could not synthesize tunnel iface={}", iface);
            }
        }
    });
}

fn build_selected_tcp_server_adapter(
    addr: String,
    iface_manager: Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    selected_tcp_server: &TcpServerSelection,
) -> TcpServer {
    let mut server = selected_tcp_server
        .client_mtu
        .map(|mtu| TcpServer::new(addr.clone(), iface_manager.clone()).with_client_mtu(mtu))
        .unwrap_or_else(|| TcpServer::new(addr, iface_manager));
    server = server.with_prefer_ipv6(selected_tcp_server.prefer_ipv6);
    if let Some(bitrate_bps) = selected_tcp_server.client_forced_bitrate_bps {
        server = server.with_client_forced_bitrate(bitrate_bps);
    }
    if selected_tcp_server.kind == "backbone" {
        server = server
            .with_client_socket_tuning(TcpSocketTuning::backbone())
            .with_backbone_client_liveness();
    } else if selected_tcp_server.kind == "tcp_server" && selected_tcp_server.i2p_tunneled {
        server = server.with_client_socket_tuning(TcpSocketTuning::i2p_tunneled());
    }
    server
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
        local_announce_capabilities,
        propagation_announce_app_data,
        mut configured_interfaces,
        receipt_map,
        receipt_tx,
        propagation_control_enabled,
        propagation_announce_config,
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
    let transport_required =
        selected_tcp_server.bind_addr.is_some() || has_enabled_configured_interface;

    let mut transport: Option<Arc<Transport>> = None;
    let peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>> = Arc::new(Mutex::new(HashMap::new()));
    let mut announce_destination: Option<Arc<tokio::sync::Mutex<SingleInputDestination>>> = None;
    let mut propagation_destination: Option<Arc<tokio::sync::Mutex<SingleInputDestination>>> = None;
    let mut control_destination: Option<Arc<tokio::sync::Mutex<SingleInputDestination>>> = None;
    let mut delivery_destination_hash_hex: Option<String> = None;
    let mut propagation_destination_hash_hex: Option<String> = None;
    let mut control_destination_hash_hex: Option<String> = None;
    let mut delivery_source_hash = [0u8; 16];
    let mut restored_path_identities = Vec::new();
    let mut startup_successes = 0usize;
    let mut startup_failures = Vec::new();
    let mut seeded_hot_apply_interfaces = Vec::new();
    let mut auto_runtime_refreshes = Vec::new();
    let mut pipe_runtime_refreshes = Vec::new();
    let mut udp_runtime_refreshes = Vec::new();
    let mut serial_runtime_refreshes = Vec::new();
    let mut kiss_runtime_refreshes = Vec::new();
    let mut ble_gatt_runtime_refreshes = Vec::new();
    let mut reticulum_ble_runtime_refreshes = Vec::new();
    let mut i2p_runtime_refreshes = Vec::new();
    let mut tcp_runtime_refreshes = Vec::new();
    let mut weave_runtime_refreshes = Vec::new();
    let mut rnode_multi_runtime_refreshes = Vec::new();
    let mut lora_runtime_refreshes = Vec::new();
    #[cfg(feature = "vrn76-kiss-ble")]
    let mut vrn76_runtime_refreshes = Vec::new();
    let mut rnode_management_bindings = Vec::new();
    let mut weave_control_bindings = Vec::new();

    if transport_required {
        if let Some(addr) = selected_tcp_server.bind_addr.as_ref() {
            log::info!(
                "{}",
                pretty_boot_line(
                    "transport",
                    &format!("reticulumd transport listening on reticulum://{}", addr)
                )
            );
        }
        log::info!("{}", pretty_daemon_line("transport enabled"));
        let transport_identity =
            rns_transport::identity_bridge::to_transport_private_identity(identity);
        let mut config = TransportConfig::new("daemon", &transport_identity, true);
        config.set_retransmit(true);
        let mut transport_instance = Transport::new(config);
        transport_instance
            .set_receipt_handler(Box::new(ReceiptBridge::new(receipt_map, receipt_tx.clone())))
            .await;
        let iface_manager = transport_instance.iface_manager();
        let (stream_reconnect_tx, stream_reconnect_rx) =
            tokio::sync::mpsc::channel::<AddressHash>(STREAM_RECONNECT_EVENT_CHANNEL_CAPACITY);
        let mut server_iface = None;
        if let Some(addr) = selected_tcp_server.bind_addr.as_ref() {
            let server = build_selected_tcp_server_adapter(
                addr.clone(),
                iface_manager.clone(),
                &selected_tcp_server,
            );
            let runtime_status = server.runtime_status_handle();
            let active_iface = iface_manager.lock().await.spawn(server, TcpServer::spawn);
            log::info!(
                "[daemon] {} enabled iface={} bind={}",
                selected_tcp_server.kind,
                active_iface,
                addr
            );
            startup_successes += 1;
            server_iface = Some(active_iface);
            tcp_runtime_refreshes.push(TcpRuntimeRefresh {
                runtime_iface: active_iface,
                status: TcpRuntimeStatusSource::Listener(runtime_status),
            });
        }

        if let Some(config) = daemon_config {
            let mut transport_identity_hash = [0_u8; 16];
            transport_identity_hash.copy_from_slice(identity.address_hash().as_slice());
            let startup = interface_startup::startup_configured_interfaces(
                args,
                config,
                &selected_tcp_server,
                &transport_instance,
                &iface_manager,
                server_iface.as_ref(),
                &mut configured_interfaces,
                reticulum_storage_path,
                Some(stream_reconnect_tx.clone()),
                Some(transport_identity_hash),
            )
            .await;
            startup_successes += startup.startup_successes;
            startup_failures.extend(startup.startup_failures);
            if startup.connected_to_shared_instance {
                transport_instance.set_connected_to_shared_instance(true).await;
            }
            for iface in startup.tunnel_synth_ifaces {
                transport_instance.synthesize_tunnel_on_interface(iface).await;
            }
            seeded_hot_apply_interfaces.extend(startup.seeded_hot_apply_interfaces);
            auto_runtime_refreshes.extend(startup.auto_runtime_refreshes);
            pipe_runtime_refreshes.extend(startup.pipe_runtime_refreshes);
            udp_runtime_refreshes.extend(startup.udp_runtime_refreshes);
            serial_runtime_refreshes.extend(startup.serial_runtime_refreshes);
            kiss_runtime_refreshes.extend(startup.kiss_runtime_refreshes);
            ble_gatt_runtime_refreshes.extend(startup.ble_gatt_runtime_refreshes);
            reticulum_ble_runtime_refreshes.extend(startup.reticulum_ble_runtime_refreshes);
            i2p_runtime_refreshes.extend(startup.i2p_runtime_refreshes);
            tcp_runtime_refreshes.extend(startup.tcp_runtime_refreshes);
            weave_runtime_refreshes.extend(startup.weave_runtime_refreshes);
            rnode_multi_runtime_refreshes.extend(startup.rnode_multi_runtime_refreshes);
            lora_runtime_refreshes.extend(startup.lora_runtime_refreshes);
            #[cfg(feature = "vrn76-kiss-ble")]
            vrn76_runtime_refreshes.extend(startup.vrn76_runtime_refreshes);
            rnode_management_bindings.extend(startup.rnode_management_bindings);
            weave_control_bindings.extend(startup.weave_control_bindings);
        }

        let path_table_restore_status = match transport_instance
            .restore_reticulum_path_table_report(reticulum_storage_path)
            .await
        {
            Ok(report) => {
                let restored = report.restored_active_paths;
                restored_path_identities = report.restored_identities;
                if restored > 0 {
                    log::info!("[daemon] restored {} Reticulum path table entries", restored);
                }
                PathTableRestoreStatus::Ok { restored_active_paths: restored }
            }
            Err(err) => {
                let message = err.to_string();
                log::error!("[daemon] failed to restore Reticulum path table: {}", message);
                PathTableRestoreStatus::Error { message }
            }
        };
        mark_path_table_restore_status_on_enabled_interfaces(
            &mut configured_interfaces,
            &path_table_restore_status,
        );

        if selected_tcp_server.selected_index.is_none() {
            if let (Some(addr), Some(active_iface)) =
                (selected_tcp_server.bind_addr.as_ref(), server_iface.as_ref())
            {
                let (host, port) = addr.rsplit_once(':').unwrap_or(("0.0.0.0", "0"));
                let mut server_record = InterfaceRecord {
                    kind: selected_tcp_server.kind.clone(),
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
                mark_path_table_restore_status(&mut server_record, &path_table_restore_status);
                configured_interfaces.push(server_record);
            }
        }

        let destinations = transport_destinations::register_transport_destinations(
            &mut transport_instance,
            transport_identity.clone(),
            local_display_name,
            local_announce_capabilities,
            propagation_announce_app_data,
            propagation_control_enabled,
            propagation_announce_config,
        )
        .await;
        announce_destination = Some(destinations.delivery);
        propagation_destination = destinations.propagation;
        control_destination = destinations.control;
        delivery_destination_hash_hex = Some(destinations.delivery_destination_hash_hex);
        propagation_destination_hash_hex = destinations.propagation_destination_hash_hex;
        control_destination_hash_hex = destinations.control_destination_hash_hex;
        delivery_source_hash = destinations.delivery_source_hash;

        let transport_arc = Arc::new(transport_instance);
        spawn_stream_reconnect_tunnel_synthesizer(transport_arc.clone(), stream_reconnect_rx);
        transport = Some(transport_arc);
    } else if let Some(config) = daemon_config {
        log::warn!(
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
        restored_path_identities,
        startup_successes,
        startup_failures,
        seeded_hot_apply_interfaces,
        auto_runtime_refreshes,
        pipe_runtime_refreshes,
        udp_runtime_refreshes,
        serial_runtime_refreshes,
        kiss_runtime_refreshes,
        ble_gatt_runtime_refreshes,
        reticulum_ble_runtime_refreshes,
        i2p_runtime_refreshes,
        tcp_runtime_refreshes,
        weave_runtime_refreshes,
        rnode_multi_runtime_refreshes,
        lora_runtime_refreshes,
        #[cfg(feature = "vrn76-kiss-ble")]
        vrn76_runtime_refreshes,
        rnode_management_bindings,
        weave_control_bindings,
    }
}

#[cfg(test)]
mod tests {
    use super::{build_selected_tcp_server_adapter, TcpServerSelection};
    use rns_transport::iface::InterfaceManager;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn selected_backbone_server_adapter_enables_socket_tuning_and_liveness() {
        let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
        let tcp = TcpServerSelection {
            bind_addr: Some("127.0.0.1:0".to_string()),
            kind: "tcp_server".to_string(),
            ..TcpServerSelection::default()
        };
        let tcp_server =
            build_selected_tcp_server_adapter("127.0.0.1:0".to_string(), manager.clone(), &tcp);

        assert!(tcp_server.client_socket_tuning().is_empty());
        assert!(!tcp_server.client_hdlc_liveness_enabled());
        assert_eq!(tcp_server.client_forced_bitrate_bps(), None);
        assert!(!tcp_server.prefer_ipv6());

        let backbone = TcpServerSelection {
            bind_addr: Some("127.0.0.1:0".to_string()),
            kind: "backbone".to_string(),
            client_mtu: Some(1_048_576),
            prefer_ipv6: true,
            ..TcpServerSelection::default()
        };
        let backbone_server =
            build_selected_tcp_server_adapter("127.0.0.1:0".to_string(), manager, &backbone);

        assert_eq!(backbone_server.client_socket_tuning().nodelay, Some(true));
        assert_eq!(backbone_server.client_socket_tuning().keepalive, Some(true));
        assert!(backbone_server.client_hdlc_liveness_enabled());
        assert!(backbone_server.prefer_ipv6());
    }

    #[test]
    fn selected_local_server_adapter_forces_shared_instance_bitrate() {
        let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
        let local = TcpServerSelection {
            bind_addr: Some("127.0.0.1:37428".to_string()),
            kind: "local".to_string(),
            client_forced_bitrate_bps: Some(1_000_000),
            ..TcpServerSelection::default()
        };

        let server =
            build_selected_tcp_server_adapter("127.0.0.1:37428".to_string(), manager, &local);

        assert_eq!(server.client_forced_bitrate_bps(), Some(1_000_000));
        assert!(server.client_socket_tuning().is_empty());
        assert!(!server.client_hdlc_liveness_enabled());
    }

    #[test]
    fn selected_i2p_tunneled_tcp_server_adapter_applies_client_socket_profile() {
        let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
        let tcp = TcpServerSelection {
            bind_addr: Some("127.0.0.1:0".to_string()),
            kind: "tcp_server".to_string(),
            i2p_tunneled: true,
            ..TcpServerSelection::default()
        };

        let tcp_server =
            build_selected_tcp_server_adapter("127.0.0.1:0".to_string(), manager, &tcp);

        assert_eq!(tcp_server.client_socket_tuning().nodelay, Some(true));
        assert_eq!(tcp_server.client_socket_tuning().keepalive, Some(true));
        assert_eq!(
            tcp_server.client_socket_tuning().tcp_keepalive_idle,
            Some(Duration::from_secs(10))
        );
        assert_eq!(
            tcp_server.client_socket_tuning().tcp_keepalive_interval,
            Some(Duration::from_secs(9))
        );
        assert_eq!(tcp_server.client_socket_tuning().tcp_keepalive_retries, Some(5));
        assert_eq!(
            tcp_server.client_socket_tuning().tcp_user_timeout,
            Some(Duration::from_secs(45))
        );
        assert!(!tcp_server.client_hdlc_liveness_enabled());
    }
}
