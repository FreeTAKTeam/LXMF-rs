async fn startup_vrn76_kiss_ble(
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
) -> Vrn76StartupResult {
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
            return Vrn76StartupResult::failed();
        }
    };

    #[cfg(feature = "vrn76-kiss-ble")]
    {
        let adapter = vrn76_kiss_ble::build_native_interface(iface, config);
        let runtime_status = adapter.runtime_status_handle();
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
        Vrn76StartupResult {
            started: true,
            refresh: Some(Vrn76RuntimeRefresh { runtime_iface: vrn76_iface, status: runtime_status }),
        }
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
        Vrn76StartupResult::failed()
    }
}

struct Vrn76StartupResult {
    started: bool,
    #[cfg(feature = "vrn76-kiss-ble")]
    refresh: Option<Vrn76RuntimeRefresh>,
}

impl Vrn76StartupResult {
    fn failed() -> Self {
        Self {
            started: false,
            #[cfg(feature = "vrn76-kiss-ble")]
            refresh: None,
        }
    }
}

struct LoraStartupResult {
    started: bool,
    refresh: Option<LoraRuntimeRefresh>,
    management_binding: Option<RNodeManagementBinding>,
}

async fn startup_lora(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
) -> LoraStartupResult {
    if let Err(err) = lora::startup(iface) {
        record_startup_failure(
            record,
            startup_failures,
            label.to_string(),
            iface.kind.clone(),
            err,
        );
        return LoraStartupResult { started: false, refresh: None, management_binding: None };
    }

    if !lora::has_active_device(iface) {
        mark_interface_startup_status(record, "validated_startup_only", None, None);
        return LoraStartupResult { started: true, refresh: None, management_binding: None };
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
                return LoraStartupResult { started: false, refresh: None, management_binding: None };
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
            return LoraStartupResult { started: false, refresh: None, management_binding: None };
        }
        #[cfg(feature = "rnode-ble")]
        {
            let adapter = lora::build_native_rnode_ble_interface(iface, config);
            let status_handle = adapter.runtime_status_handle();
            let management_handle = adapter.rnode_management_handle();
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
            let refresh = status_handle.clone().map(|status_handle| LoraRuntimeRefresh {
                runtime_iface: rnode_iface,
                status: LoraRuntimeStatusSource::RnodeBle(status_handle),
            });
            if let Some(status_handle) = status_handle.as_ref() {
                with_interface_runtime_metadata(record, |runtime| {
                    runtime.insert(
                        "lora".to_string(),
                        serde_json::json!({
                            "rnode_status": status_handle.to_json()
                        }),
                    );
                });
            }
            return LoraStartupResult {
                started: true,
                refresh,
                management_binding: Some(RNodeManagementBinding {
                    runtime_iface: rnode_iface,
                    name: label.to_string(),
                    handle: crate::bridge_rnode_management::DaemonRNodeManagementHandle::RnodeBle(
                        management_handle,
                    ),
                }),
            };
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
            return LoraStartupResult { started: false, refresh: None, management_binding: None };
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
            return LoraStartupResult { started: false, refresh: None, management_binding: None };
        }
    }

    let mode = iface.interface_mode().unwrap_or(InterfaceMode::Full);
    let (lora_iface, status_handle) = iface_manager.lock().await.spawn_as_with_mode_and_handle(
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
    let management_handle = {
        let guard = status_handle.lock().expect("lora interface mutex poisoned");
        guard.rnode_management_handle()
    };
    with_interface_runtime_metadata(record, |runtime| {
        runtime.insert(
            "lora".to_string(),
            serde_json::json!({
                "rnode_status": status_handle
                    .lock()
                    .expect("lora interface mutex poisoned")
                    .runtime_status_json()
            }),
        );
    });
    LoraStartupResult {
        started: true,
        refresh: Some(LoraRuntimeRefresh {
            runtime_iface: lora_iface,
            status: LoraRuntimeStatusSource::Lora(
                rns_transport::iface::lora::LoraRuntimeStatusHandle::new(status_handle),
            ),
        }),
        management_binding: Some(RNodeManagementBinding {
            runtime_iface: lora_iface,
            name: label.to_string(),
            handle: crate::bridge_rnode_management::DaemonRNodeManagementHandle::Lora(
                management_handle,
            ),
        }),
    }
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
    manager.set_gravity(address, iface.interface_gravity());
    manager.set_outgoing(address, iface.outgoing());
    if iface.bitrate.is_some() || iface.announce_cap.is_some() {
        let (current_bitrate, current_announce_cap) =
            manager.announce_pacing(&address).unwrap_or((62_500, 2));
        let bitrate = iface.bitrate.unwrap_or(current_bitrate);
        let announce_cap = iface.announce_cap.unwrap_or(current_announce_cap);
        manager.set_announce_pacing(address, bitrate, announce_cap);
    }
    manager.set_shared_config(
        address,
        rns_transport::iface::InterfaceSharedConfig {
            announce_rate_target: iface.announce_rate_target,
            announce_rate_grace: iface.announce_rate_grace,
            announce_rate_penalty: iface.announce_rate_penalty,
            bootstrap_only: iface.bootstrap_only,
            ifac_size: iface.ifac_size,
            network_name: iface.ifac_network_name().cloned(),
            passphrase: iface.ifac_passphrase().cloned(),
            ingress_control: iface.ingress_control,
            egress_control: iface.egress_control,
            ic_max_held_announces: iface.ic_max_held_announces,
            ic_burst_hold: iface.ic_burst_hold,
            ic_burst_freq_new: iface.ic_burst_freq_new,
            ic_burst_freq: iface.ic_burst_freq,
            ic_pr_burst_freq_new: iface.ic_pr_burst_freq_new,
            ic_pr_burst_freq: iface.ic_pr_burst_freq,
            ec_pr_freq: iface.ec_pr_freq,
            ic_new_time: iface.ic_new_time,
            ic_burst_penalty: iface.ic_burst_penalty,
            ic_held_release_interval: iface.ic_held_release_interval,
            discoverable: iface.discoverable,
            announce_interval: iface.discovery_announce_interval_secs(),
            discovery_stamp_value: iface.discovery_stamp_value,
            discovery_name: iface.discovery_name.clone(),
            discovery_encrypt: iface.discovery_encrypt,
            reachable_on: iface.reachable_on.clone(),
            publish_ifac: iface.publish_ifac,
            latitude: iface.latitude,
            longitude: iface.longitude,
            height: iface.height,
            discovery_frequency: iface.discovery_frequency,
            discovery_bandwidth: iface.discovery_bandwidth,
            discovery_modulation: iface.discovery_modulation,
        },
    );
}
