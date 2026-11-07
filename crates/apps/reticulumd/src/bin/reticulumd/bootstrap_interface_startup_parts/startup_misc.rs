async fn startup_meshtastic(
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
) -> Option<MeshtasticRuntimeRefresh> {
    let config = meshtastic::build_config(iface);
    if !config.simulation_loopback {
        record_startup_failure(
            record,
            startup_failures,
            label.to_string(),
            iface.kind.clone(),
            "Meshtastic daemon startup currently requires simulation_loopback = true".to_string(),
        );
        return None;
    }
    let adapter = rns_transport::iface::meshtastic::MeshtasticInterface::new(
        label.to_string(),
        config.transport,
    );
    let runtime_status = adapter.runtime_status_handle();
    let simulation_handle = adapter.handle();
    let runtime_iface = record_prepared_spawn(
        spawn_configured(
            &mut *iface_manager.lock().await,
            adapter,
            rns_transport::iface::meshtastic::MeshtasticInterface::spawn,
            IfaceRole::Unicast,
            iface,
        ),
        record,
        startup_failures,
        label,
    )?;
    let simulation =
        meshtastic::spawn_simulation_loopback(simulation_handle, config.simulation_node_id);
    let runtime_iface_string = runtime_iface.to_string();
    mark_interface_startup_status(record, "spawned", None, Some(runtime_iface_string.as_str()));
    with_interface_runtime_metadata(record, |runtime| {
        runtime.insert(
            "meshtastic".to_string(),
            serde_json::json!({
                "status": runtime_status.to_json(),
                "evidence": "simulated",
                "hardware_unverified": true,
                "simulation_node_id": config.simulation_node_id,
            }),
        );
    });
    log::info!(
        "[daemon] meshtastic simulation enabled iface={} name={} node_id={}",
        runtime_iface,
        label,
        config.simulation_node_id
    );
    Some(MeshtasticRuntimeRefresh { runtime_iface, status: runtime_status, simulation })
}

fn startup_tcp_server_record(
    index: usize,
    iface: &InterfaceConfig,
    label: &str,
    selected_tcp_server: &TcpServerSelection,
    server_iface: Option<&AddressHash>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
) -> Option<(String, InterfaceRecord, AddressHash)> {
    let selected_for_startup = selected_tcp_server.selected_index == Some(index);
    if !selected_for_startup {
        mark_interface_startup_status(
            record,
            "shadowed_by_transport_override",
            Some(&format!(
                "{} ignored because --transport selected the active bind endpoint",
                iface.kind
            )),
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
            "[daemon] {} startup skipped name={} endpoint={} selected={}",
            iface.kind,
            label,
            endpoint,
            selected_tcp_server.bind_addr.as_deref().unwrap_or("<none>")
        );
        return None;
    }

    if iface.port.is_none() {
        record_startup_failure(
            record,
            startup_failures,
            label.to_string(),
            iface.kind.clone(),
            format!("{} requires port for startup", iface.kind),
        );
        return None;
    }
    let active_iface = server_iface?;
    let runtime_iface = active_iface.to_string();
    mark_interface_startup_status(record, "active", None, Some(&runtime_iface));
    mark_tcp_server_record_runtime_status(record, server_iface.copied());
    server_iface.and_then(|active_iface| {
        hot_apply_interface_seed_key(record).map(|key| (key, record.clone(), *active_iface))
    })
}

async fn startup_pipe(
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
) -> Option<PipeRuntimeRefresh> {
    let adapter = match pipe::build_adapter(iface) {
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

    let pipe_metadata = pipe_runtime_metadata_json(&adapter);
    let runtime_status = adapter.runtime_status_handle();
    let pipe_iface = record_prepared_spawn(
        spawn_configured(
            &mut *iface_manager.lock().await,
            adapter,
            rns_transport::iface::pipe::PipeInterface::spawn,
            IfaceRole::Unicast,
            iface,
        ),
        record,
        startup_failures,
        label,
    )?;
    log::info!("[daemon] pipe enabled iface={} name={}", pipe_iface, label);
    let runtime_iface = pipe_iface.to_string();
    mark_interface_startup_status(record, "spawned", None, Some(runtime_iface.as_str()));
    with_interface_runtime_metadata(record, |runtime| {
        runtime.insert("pipe".to_string(), pipe_metadata);
    });
    Some(PipeRuntimeRefresh { runtime_iface: pipe_iface, status: runtime_status })
}

fn pipe_runtime_metadata_json(
    adapter: &rns_transport::iface::pipe::PipeInterface,
) -> serde_json::Value {
    let mut metadata = serde_json::Map::new();
    metadata.insert("status".to_string(), adapter.runtime_status_json());
    serde_json::Value::Object(metadata)
}
