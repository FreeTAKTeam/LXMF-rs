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
                ..
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
