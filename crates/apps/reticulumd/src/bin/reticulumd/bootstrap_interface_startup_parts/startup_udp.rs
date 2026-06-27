async fn startup_udp(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    transport: &Transport,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    sinks: &mut UdpStartupSinks<'_>,
) -> bool {
    let (bind_addr, forward_addr) = match udp::bind_and_forward_addr(iface) {
        Ok(addrs) => addrs,
        Err(err) => {
            record_startup_failure(
                record,
                sinks.startup_failures,
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
                sinks.startup_failures,
                label.to_string(),
                iface.kind.clone(),
                err,
            );
            return false;
        }
    }

    let mode = iface.interface_mode().unwrap_or(InterfaceMode::Full);
    let adapter = UdpInterface::new(bind_addr.clone(), forward_addr.clone());
    let status = adapter.runtime_status_handle();
    let is_multicast = adapter.is_multicast();
    let udp_iface = if is_multicast {
        let (udp_iface, status) = transport
            .add_multicast_udp_interface_with_status(bind_addr.clone(), forward_addr.clone())
            .await;
        iface_manager.lock().await.set_mode(udp_iface, mode);
        sinks.runtime_refreshes.push(UdpRuntimeRefresh { runtime_iface: udp_iface, status });
        udp_iface
    } else {
        let udp_iface = iface_manager.lock().await.spawn_as_with_mode(
            adapter,
            UdpInterface::spawn,
            IfaceRole::Unicast,
            mode,
        );
        sinks.runtime_refreshes.push(UdpRuntimeRefresh { runtime_iface: udp_iface, status });
        udp_iface
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
    mark_udp_runtime_status(
        record,
        bind_addr.as_str(),
        forward_addr.as_deref(),
        is_multicast,
        udp_iface,
    );
    true
}

async fn startup_auto(
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
) -> Option<AutoRuntimeRefresh> {
    match auto::build_native_startup_plan(iface) {
        Ok(plan) => {
            let adopted_count = plan.adopted_devices.len();
            let candidate_count = plan.candidates.len();
            let runtime_status =
                auto::AutoRuntimeStatusHandle::from_startup_plan(&plan.startup_plan);
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
                ), Some(runtime_status.clone()))
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
                    Some(AutoRuntimeRefresh { runtime_iface: host_iface, status: runtime_status })
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
                    None
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
            None
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
    serial_runtime_refreshes: &mut Vec<SerialRuntimeRefresh>,
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
    let status = adapter.runtime_status_handle();
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
    mark_serial_runtime_status(record, iface, serial_iface);
    serial_runtime_refreshes.push(SerialRuntimeRefresh { runtime_iface: serial_iface, status });
    true
}

async fn startup_kiss(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    kiss_runtime_refreshes: &mut Vec<KissRuntimeRefresh>,
) -> bool {
    let adapter = match if iface.kind == "ax25_kiss" {
        kiss::build_ax25_adapter(iface)
    } else {
        kiss::build_adapter(iface)
    } {
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
    let status = adapter.runtime_status_handle();
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
        "[daemon] {} enabled iface={} name={} device={} baud_rate={}",
        iface.kind,
        kiss_iface,
        label,
        iface.device.as_deref().unwrap_or("<unset>"),
        iface.baud_rate.unwrap_or_default()
    );
    let runtime_iface = kiss_iface.to_string();
    mark_interface_startup_status(record, "spawned", None, Some(runtime_iface.as_str()));
    mark_kiss_runtime_status(record, iface, kiss_iface);
    kiss_runtime_refreshes.push(KissRuntimeRefresh {
        runtime_iface: kiss_iface,
        runtime_key: "kiss",
        status,
    });
    true
}

async fn startup_kiss_tcp_client(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    kiss_runtime_refreshes: &mut Vec<KissRuntimeRefresh>,
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
    let status = adapter.runtime_status_handle();
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
    mark_kiss_tcp_runtime_status(record, iface, kiss_iface);
    kiss_runtime_refreshes.push(KissRuntimeRefresh {
        runtime_iface: kiss_iface,
        runtime_key: "kiss_tcp",
        status,
    });
    true
}

async fn startup_ble(
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    ble_gatt_runtime_refreshes: &mut Vec<BleGattRuntimeRefresh>,
) -> bool {
    match ble::spawn(iface_manager.clone(), iface).await {
        Ok(spawned) => {
            mark_ble_spawn_success(iface, label, iface_manager, record, spawned.iface).await;
            ble_gatt_runtime_refreshes.push(BleGattRuntimeRefresh {
                runtime_iface: spawned.iface,
                status: spawned.status,
            });
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

async fn mark_ble_spawn_success(
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    ble_iface: AddressHash,
) {
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
    mark_ble_gatt_runtime_status(record, iface, ble_iface);
}

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
