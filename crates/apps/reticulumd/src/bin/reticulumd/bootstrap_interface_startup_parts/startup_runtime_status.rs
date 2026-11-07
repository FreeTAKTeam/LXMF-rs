fn mark_udp_runtime_status(
    record: &mut InterfaceRecord,
    bind_addr: &str,
    forward_addr: Option<&str>,
    is_multicast: bool,
    runtime_iface: AddressHash,
) {
    let role = if is_multicast {
        "multicast"
    } else if forward_addr.is_some() {
        "peer"
    } else {
        "listener"
    };
    with_interface_runtime_metadata(record, |runtime| {
        runtime.insert(
            "udp".to_string(),
            serde_json::json!({
                "status": {
                    "link_state": "configured",
                    "role": role,
                    "bind_addr": bind_addr,
                    "forward_addr": forward_addr,
                    "iface": runtime_iface.to_string(),
                }
            }),
        );
    });
}

fn mark_serial_runtime_status(
    record: &mut InterfaceRecord,
    iface: &InterfaceConfig,
    runtime_iface: AddressHash,
) {
    with_interface_runtime_metadata(record, |runtime| {
        runtime.insert(
            "serial".to_string(),
            serde_json::json!({
                "status": {
                    "link_state": "configured",
                    "device": iface.device.as_deref(),
                    "baud_rate": iface.baud_rate,
                    "data_bits": iface.data_bits,
                    "parity": iface.parity.as_deref(),
                    "stop_bits": iface.stop_bits,
                    "flow_control": iface.flow_control_name(),
                    "mtu": iface.mtu,
                    "iface": runtime_iface.to_string(),
                }
            }),
        );
    });
}

fn mark_kiss_runtime_status(
    record: &mut InterfaceRecord,
    iface: &InterfaceConfig,
    runtime_iface: AddressHash,
) {
    with_interface_runtime_metadata(record, |runtime| {
        runtime.insert(
            "kiss".to_string(),
            serde_json::json!({
                "status": kiss_runtime_status_json(iface, runtime_iface, None),
            }),
        );
    });
}

fn mark_kiss_tcp_runtime_status(
    record: &mut InterfaceRecord,
    iface: &InterfaceConfig,
    runtime_iface: AddressHash,
) {
    with_interface_runtime_metadata(record, |runtime| {
        runtime.insert(
            "kiss_tcp".to_string(),
            serde_json::json!({
                "status": kiss_runtime_status_json(iface, runtime_iface, Some(format!(
                    "{}:{}",
                    iface.host.as_deref().unwrap_or("<unset>"),
                    iface.port.unwrap_or_default()
                ))),
            }),
        );
    });
}

fn kiss_runtime_status_json(
    iface: &InterfaceConfig,
    runtime_iface: AddressHash,
    endpoint: Option<String>,
) -> serde_json::Value {
    serde_json::json!({
        "link_state": "configured",
        "bearer": if endpoint.is_some() { "tcp" } else { "serial" },
        "device": iface.device.as_deref(),
        "endpoint": endpoint,
        "baud_rate": iface.baud_rate,
        "mtu": iface.mtu,
        "preamble_ms": iface.preamble_ms,
        "tx_tail_ms": iface.tx_tail_ms,
        "persistence": iface.persistence,
        "slot_time_ms": iface.slot_time_ms,
        "kiss_flow_control": iface.kiss_flow_control,
        "ax25": iface.kind == "ax25_kiss",
        "callsign": iface.callsign.as_deref(),
        "ssid": iface.ssid,
        "id_callsign": iface.id_callsign.as_deref(),
        "id_interval": iface.id_interval,
        "iface": runtime_iface.to_string(),
    })
}

fn mark_ble_gatt_runtime_status(
    record: &mut InterfaceRecord,
    iface: &InterfaceConfig,
    runtime_iface: AddressHash,
) {
    with_interface_runtime_metadata(record, |runtime| {
        runtime.insert(
            "ble_gatt".to_string(),
            serde_json::json!({
                "status": {
                    "link_state": "configured",
                    "adapter": iface.adapter.as_deref(),
                    "peripheral_id": iface.peripheral_id.as_deref(),
                    "service_uuid": iface.service_uuid.as_deref(),
                    "write_char_uuid": iface.write_char_uuid.as_deref(),
                    "notify_char_uuid": iface.notify_char_uuid.as_deref(),
                    "mtu": iface.mtu,
                    "scan_timeout_ms": iface.scan_timeout_ms,
                    "connect_timeout_ms": iface.ble_connect_timeout_ms.or(iface.connect_timeout_ms),
                    "iface": runtime_iface.to_string(),
                }
            }),
        );
    });
}

fn mark_reticulum_ble_runtime_status(
    record: &mut InterfaceRecord,
    iface: &InterfaceConfig,
    runtime_iface: AddressHash,
    local_identity: [u8; 16],
) {
    let local_identity =
        local_identity.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    with_interface_runtime_metadata(record, |runtime| {
        runtime.insert(
            "reticulum_ble".to_string(),
            serde_json::json!({
                "status": {
                    "link_state": "degraded",
                    "adapter": iface.adapter.as_deref(),
                    "service_uuid": iface.service_uuid.as_deref().unwrap_or(reticulum_ble::SERVICE_UUID),
                    "tx_char_uuid": iface.notify_char_uuid.as_deref().unwrap_or(reticulum_ble::TX_CHAR_UUID),
                    "rx_char_uuid": iface.write_char_uuid.as_deref().unwrap_or(reticulum_ble::RX_CHAR_UUID),
                    "identity_char_uuid": iface.identity_char_uuid.as_deref().unwrap_or(reticulum_ble::IDENTITY_CHAR_UUID),
                    "local_identity": local_identity,
                    "mtu": iface.mtu.unwrap_or(185),
                    "max_connections": iface.max_connections.unwrap_or(7),
                    "scan_duration_ms": iface.scan_duration_ms.unwrap_or(10_000),
                    "discovery_interval_ms": iface.discovery_interval_ms.unwrap_or(5_000),
                    "discovery_interval_idle_ms": iface.discovery_interval_idle_ms.unwrap_or(30_000),
                    "advertising_refresh_interval_ms": iface.advertising_refresh_interval_ms.unwrap_or(30_000),
                    "min_rssi_dbm": iface.min_rssi_dbm.unwrap_or(-85),
                    "enable_central": iface.enable_central.unwrap_or(true),
                    "enable_peripheral": iface.enable_peripheral.unwrap_or(true),
                    "central_links": 0,
                    "peripheral_links": 0,
                    "scan_state": if iface.enable_central.unwrap_or(true) { "native_backend_pending" } else { "disabled" },
                    "advertising_state": if iface.enable_peripheral.unwrap_or(true) { "native_backend_pending" } else { "disabled" },
                    "peer_identities": [],
                    "active_addresses": [],
                    "fragments_rx": 0,
                    "fragments_tx": 0,
                    "packets_rx": 0,
                    "packets_tx": 0,
                    "malformed_fragments": 0,
                    "reconnects": 0,
                    "duplicate_rejections": 0,
                    "stale_reassembly_drops": 0,
                    "last_error": "reticulum_ble native dual-role backend is not yet available",
                    "iface": runtime_iface.to_string(),
                }
            }),
        );
    });
}
