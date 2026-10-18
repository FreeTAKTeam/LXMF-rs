async fn startup_rnode_multi(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    rnode_management_bindings: &mut Vec<RNodeManagementBinding>,
) -> Option<RNodeMultiRuntimeRefresh> {
    let adapter = match rnode_multi::build_adapter(iface, iface_manager.clone()) {
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
    let subinterfaces = adapter.subinterfaces().to_vec();
    let subinterface_count = subinterfaces.len();
    let rnode_multi_metadata = rnode_multi_runtime_metadata_json(iface, &subinterfaces);
    let runtime_status = adapter.runtime_status_handle();
    let management_handle = adapter.rnode_management_handle();
    let rnode_multi_iface = record_prepared_spawn(
        spawn_configured(
            &mut *iface_manager.lock().await,
            adapter,
            rns_transport::iface::rnode_multi::RNodeMultiInterface::spawn,
            IfaceRole::Multicast,
            iface,
        ),
        record,
        startup_failures,
        label,
    )?;
    log::info!(
        "[daemon] rnode_multi enabled iface={} name={} device={} baud_rate={} subinterfaces={}",
        rnode_multi_iface,
        label,
        iface.device.as_deref().unwrap_or("<unset>"),
        iface.baud_rate.unwrap_or_default(),
        subinterface_count
    );
    let runtime_iface = rnode_multi_iface.to_string();
    mark_interface_startup_status(record, "spawned", None, Some(runtime_iface.as_str()));
    with_interface_runtime_metadata(record, |runtime| {
        runtime.insert("rnode_multi".to_string(), rnode_multi_metadata);
    });
    let mut allowed_vports =
        subinterfaces.iter().map(|subinterface| subinterface.vport).collect::<Vec<_>>();
    allowed_vports.sort_unstable();
    allowed_vports.dedup();
    rnode_management_bindings.push(RNodeManagementBinding {
        runtime_iface: rnode_multi_iface,
        name: label.to_string(),
        handle: DaemonRNodeManagementHandle::RNodeMulti {
            handle: management_handle,
            allowed_vports,
        },
    });
    Some(RNodeMultiRuntimeRefresh { runtime_iface: rnode_multi_iface, status: runtime_status })
}

fn rnode_multi_runtime_metadata_json(
    iface: &InterfaceConfig,
    subinterfaces: &[rns_transport::iface::rnode_multi::RNodeMultiSubInterfaceConfig],
) -> serde_json::Value {
    let mut metadata = serde_json::Map::new();
    if let Some(device) = iface.device.as_deref().filter(|value| !value.trim().is_empty()) {
        metadata.insert("device".to_string(), serde_json::Value::String(device.to_string()));
    }
    if let Some(baud_rate) = iface.baud_rate {
        metadata.insert("baud_rate".to_string(), serde_json::Value::Number(baud_rate.into()));
    }
    metadata.insert(
        "subinterface_count".to_string(),
        serde_json::Value::Number((subinterfaces.len() as u64).into()),
    );
    metadata.insert(
        "subinterfaces".to_string(),
        serde_json::Value::Array(
            subinterfaces.iter().map(rnode_multi_subinterface_runtime_json).collect(),
        ),
    );
    metadata.insert(
        "radio_status".to_string(),
        rns_transport::iface::rnode_multi::RNodeMultiRuntimeStatus::from_subinterfaces(
            subinterfaces,
        )
        .to_json(),
    );
    serde_json::Value::Object(metadata)
}

fn rnode_multi_subinterface_runtime_json(
    subinterface: &rns_transport::iface::rnode_multi::RNodeMultiSubInterfaceConfig,
) -> serde_json::Value {
    let mut entry = serde_json::Map::new();
    entry.insert("name".to_string(), serde_json::Value::String(subinterface.name.clone()));
    entry.insert(
        "vport".to_string(),
        serde_json::Value::Number(u64::from(subinterface.vport).into()),
    );
    entry.insert("outgoing".to_string(), serde_json::Value::Bool(subinterface.outgoing));

    let config = subinterface.config;
    entry.insert("frequency_hz".to_string(), serde_json::Value::Number(config.frequency_hz.into()));
    entry.insert(
        "bandwidth_hz".to_string(),
        serde_json::Value::Number(u64::from(config.bandwidth_hz).into()),
    );
    entry.insert(
        "spreading_factor".to_string(),
        serde_json::Value::Number(u64::from(config.spreading_factor).into()),
    );
    entry.insert(
        "coding_rate".to_string(),
        serde_json::Value::Number(u64::from(config.coding_rate).into()),
    );
    entry.insert(
        "tx_power_dbm".to_string(),
        serde_json::Value::Number(i64::from(config.tx_power_dbm).into()),
    );
    entry.insert(
        "max_payload_bytes".to_string(),
        serde_json::Value::Number(u64::from(config.max_payload_bytes).into()),
    );
    if let Some(value) = config.airtime_limit_short_hundredths {
        entry.insert(
            "airtime_limit_short_hundredths".to_string(),
            serde_json::Value::Number(u64::from(value).into()),
        );
    }
    if let Some(value) = config.airtime_limit_long_hundredths {
        entry.insert(
            "airtime_limit_long_hundredths".to_string(),
            serde_json::Value::Number(u64::from(value).into()),
        );
    }
    serde_json::Value::Object(entry)
}
