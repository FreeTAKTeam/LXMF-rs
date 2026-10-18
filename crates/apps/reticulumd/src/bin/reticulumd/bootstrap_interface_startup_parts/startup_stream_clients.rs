async fn startup_weave(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
) -> Option<WeaveRuntimeRefresh> {
    let adapter = match weave::build_adapter(iface, iface_manager.clone()) {
        Ok(adapter) => adapter,
        Err(err) => {
            record_startup_failure(
                record,
                startup_failures,
                label.to_string(),
                iface.kind.clone(),
                err,
            );
            return None;
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
            return None;
        }
    }

    let weave_metadata = weave_runtime_metadata_json(&adapter);
    let runtime_status = adapter.runtime_status_handle();
    let control_handle = adapter.weave_management_handle();
    let weave_iface = record_prepared_spawn(
        spawn_configured(
            &mut *iface_manager.lock().await,
            adapter,
            rns_transport::iface::weave::WeaveInterface::spawn,
            IfaceRole::Multicast,
            iface,
        ),
        record,
        startup_failures,
        label,
    )?;
    log::info!(
        "[daemon] weave enabled iface={} name={} device={} baud_rate={}",
        weave_iface,
        label,
        iface.device.as_deref().unwrap_or("<unset>"),
        iface.baud_rate.unwrap_or_default()
    );
    let runtime_iface = weave_iface.to_string();
    mark_interface_startup_status(record, "spawned", None, Some(runtime_iface.as_str()));
    with_interface_runtime_metadata(record, |runtime| {
        runtime.insert("weave".to_string(), weave_metadata);
    });
    Some(WeaveRuntimeRefresh {
        runtime_iface: weave_iface,
        status: runtime_status,
        handle: control_handle,
    })
}

fn weave_runtime_metadata_json(
    adapter: &rns_transport::iface::weave::WeaveInterface,
) -> serde_json::Value {
    let mut metadata = serde_json::Map::new();
    metadata.insert("status".to_string(), adapter.runtime_status_json());
    serde_json::Value::Object(metadata)
}

#[allow(clippy::too_many_arguments)]
async fn startup_tcp_client(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    seeded_hot_apply_interfaces: &mut Vec<(String, InterfaceRecord, AddressHash)>,
    tcp_runtime_refreshes: &mut Vec<TcpRuntimeRefresh>,
    stream_reconnect_events: Option<tokio::sync::mpsc::Sender<AddressHash>>,
) -> Option<AddressHash> {
    let (Some(host), Some(port)) = (iface.host.as_ref(), iface.port) else {
        record_startup_failure(
            record,
            startup_failures,
            label.to_string(),
            iface.kind.clone(),
            format!("{} requires host and port for startup", iface.kind),
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

    let adapter = build_tcp_client_adapter(endpoint, iface, stream_reconnect_events);
    let runtime_status = adapter.runtime_status_handle();
    let client_iface = record_prepared_spawn(
        spawn_configured(
            &mut *iface_manager.lock().await,
            adapter,
            TcpClient::spawn,
            IfaceRole::Unicast,
            iface,
        ),
        record,
        startup_failures,
        label,
    )?;
    log::info!(
        "[daemon] {} enabled iface={} name={} host={} port={}",
        iface.kind,
        client_iface,
        label,
        host,
        port
    );
    let runtime_iface = client_iface.to_string();
    mark_interface_startup_status(record, "spawned", None, Some(runtime_iface.as_str()));
    tcp_runtime_refreshes.push(TcpRuntimeRefresh {
        runtime_iface: client_iface,
        status: TcpRuntimeStatusSource::Stream(runtime_status),
    });
    if let Some(key) = hot_apply_interface_seed_key(record) {
        seeded_hot_apply_interfaces.push((key, record.clone(), client_iface));
    }
    Some(client_iface)
}

pub(crate) fn build_tcp_client_adapter(
    endpoint: String,
    iface: &InterfaceConfig,
    stream_reconnect_events: Option<tokio::sync::mpsc::Sender<AddressHash>>,
) -> TcpClient {
    let mut adapter = TcpClient::new(endpoint);
    if iface.kind == "backbone_client" {
        adapter = adapter.with_socket_tuning(TcpSocketTuning::backbone()).with_backbone_liveness();
    } else if iface.kind == "tcp_client" && iface.i2p_tunneled == Some(true) {
        adapter = adapter.with_socket_tuning(TcpSocketTuning::i2p_tunneled());
    }
    if let Some(connect_timeout) = iface.connect_timeout {
        adapter = adapter.with_connect_timeout(Duration::from_secs(connect_timeout));
    }
    if iface.max_reconnect_tries.is_some() {
        adapter = adapter.with_max_reconnect_tries(iface.max_reconnect_tries);
    }
    if let Some(prefer_ipv6) = iface.prefer_ipv6 {
        adapter = adapter.with_prefer_ipv6(prefer_ipv6);
    }
    if let Some(mtu) = iface.mtu {
        adapter = adapter.with_mtu(mtu);
    }
    if matches!(iface.kind.as_str(), "local" | "local_client") {
        if let Some(bitrate_bps) = iface.force_shared_instance_bitrate {
            adapter = adapter.with_forced_bitrate(bitrate_bps);
        }
    }
    if matches!(iface.kind.as_str(), "tcp_client" | "backbone_client") {
        if let Some(events) = stream_reconnect_events {
            adapter = adapter.with_reconnect_events(events);
        }
    }
    adapter
}
