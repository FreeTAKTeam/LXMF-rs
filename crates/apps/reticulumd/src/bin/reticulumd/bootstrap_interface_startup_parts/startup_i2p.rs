#[allow(clippy::too_many_arguments)]
async fn startup_i2p(
    strict_interface_startup: bool,
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    reticulum_storage_path: &std::path::Path,
    transport_identity_hash: Option<[u8; 16]>,
) -> Option<I2pRuntimeRefresh> {
    let effective_iface = i2p_config_with_storage_default(iface, reticulum_storage_path);
    let adapter = match i2p::build_adapter(&effective_iface, iface_manager.clone()) {
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
    }
    .with_transport_identity_hash(transport_identity_hash);

    if strict_interface_startup {
        if let Err(err) = adapter.preflight_sam().await {
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

    let peer_count = adapter.peers().len();
    let i2p_metadata = match i2p_runtime_metadata_json(
        strict_interface_startup,
        &effective_iface,
        &adapter,
        peer_count,
    )
    .await
    {
        Ok(metadata) => metadata,
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
    let runtime_status = adapter.runtime_status_handle();
    let i2p_iface = record_prepared_spawn(
        spawn_configured(
            &mut *iface_manager.lock().await,
            adapter,
            rns_transport::iface::i2p::I2pInterface::spawn,
            IfaceRole::Multicast,
            iface,
        ),
        record,
        startup_failures,
        label,
    )?;
    log::info!(
        "[daemon] i2p enabled iface={} name={} sam={}:{} peers={}",
        i2p_iface,
        label,
        iface.sam_host.as_deref().unwrap_or("<unset>"),
        iface.sam_port.unwrap_or_default(),
        peer_count
    );
    let runtime_iface = i2p_iface.to_string();
    mark_interface_startup_status(record, "spawned", None, Some(runtime_iface.as_str()));
    with_interface_runtime_metadata(record, |runtime| {
        runtime.insert("i2p".to_string(), i2p_metadata);
    });
    Some(I2pRuntimeRefresh { runtime_iface: i2p_iface, status: runtime_status })
}

fn i2p_config_with_storage_default(
    iface: &InterfaceConfig,
    reticulum_storage_path: &std::path::Path,
) -> InterfaceConfig {
    let mut effective = iface.clone();
    if effective.state_path.as_deref().map(str::trim).filter(|value| !value.is_empty()).is_none() {
        effective.state_path = Some(reticulum_storage_path.to_string_lossy().to_string());
    }
    effective
}

async fn i2p_runtime_metadata_json(
    strict_interface_startup: bool,
    iface: &InterfaceConfig,
    adapter: &rns_transport::iface::i2p::I2pInterface,
    peer_count: usize,
) -> Result<serde_json::Value, String> {
    let mut metadata = serde_json::Map::new();
    metadata.insert(
        "sam_endpoint".to_string(),
        serde_json::Value::String(format!(
            "{}:{}",
            iface.sam_host.as_deref().unwrap_or("127.0.0.1"),
            iface.sam_port.unwrap_or(rns_transport::iface::i2p::I2pInterface::DEFAULT_SAM_PORT)
        )),
    );
    metadata
        .insert("peer_count".to_string(), serde_json::Value::Number((peer_count as u64).into()));
    metadata.insert(
        "connectable".to_string(),
        serde_json::Value::Bool(iface.connectable.unwrap_or(false)),
    );
    metadata.insert("tunnel_status".to_string(), adapter.runtime_status_json());
    if let Some(state_path) = iface.state_path.as_deref().filter(|value| !value.trim().is_empty()) {
        let path = std::path::PathBuf::from(state_path);
        let transport_identity_hash = adapter.transport_identity_hash();
        let key_path = rns_transport::iface::i2p::i2p_private_key_path_with_identity(
            &path,
            adapter.name(),
            transport_identity_hash.as_ref(),
        );
        metadata.insert(
            "private_key_path".to_string(),
            serde_json::Value::String(key_path.to_string_lossy().to_string()),
        );
        match std::fs::read_to_string(key_path.as_path()) {
            Ok(private_key) => {
                metadata.insert("private_key_persisted".to_string(), serde_json::Value::Bool(true));
                if let Ok(endpoint) =
                    rns_transport::iface::i2p::i2p_b32_from_private_destination(&private_key)
                {
                    metadata.insert(
                        "reachable_endpoint".to_string(),
                        serde_json::Value::String(endpoint),
                    );
                }
            }
            Err(_) => {
                metadata
                    .insert("private_key_persisted".to_string(), serde_json::Value::Bool(false));
            }
        }
        if adapter.connectable() && !metadata.contains_key("reachable_endpoint") {
            match tokio::time::timeout(
                std::time::Duration::from_secs(2),
                rns_transport::iface::i2p::connectable_session_destination_with_identity(
                    adapter.sam_addr(),
                    adapter.name(),
                    Some(&path),
                    transport_identity_hash.as_ref(),
                ),
            )
            .await
            {
                Ok(Ok(private_key)) => {
                    metadata
                        .insert("private_key_persisted".to_string(), serde_json::Value::Bool(true));
                    match rns_transport::iface::i2p::i2p_b32_from_private_destination(&private_key)
                    {
                        Ok(endpoint) => {
                            metadata.insert(
                                "reachable_endpoint".to_string(),
                                serde_json::Value::String(endpoint),
                            );
                        }
                        Err(err) => {
                            metadata.insert(
                                "reachable_endpoint_error".to_string(),
                                serde_json::Value::String(err),
                            );
                        }
                    }
                }
                Ok(Err(err)) if strict_interface_startup => {
                    return Err(format!(
                        "i2p connectable destination preparation failed sam={} err={}",
                        adapter.sam_addr(),
                        err
                    ));
                }
                Ok(Err(err)) => {
                    metadata.insert(
                        "destination_prepare_error".to_string(),
                        serde_json::Value::String(err.to_string()),
                    );
                }
                Err(_) if strict_interface_startup => {
                    return Err(format!(
                        "i2p connectable destination preparation timed out sam={}",
                        adapter.sam_addr()
                    ));
                }
                Err(_) => {
                    metadata.insert(
                        "destination_prepare_error".to_string(),
                        serde_json::Value::String("timed out".to_string()),
                    );
                }
            }
        }
    }
    Ok(serde_json::Value::Object(metadata))
}
