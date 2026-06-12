async fn startup_udp(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    transport: &Transport,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
) -> bool {
    let (bind_addr, forward_addr) = match udp::bind_and_forward_addr(iface) {
        Ok(addrs) => addrs,
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
        if let Err(err) = udp::strict_preflight(bind_addr.as_str()).await {
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
    let adapter = UdpInterface::new(bind_addr.clone(), forward_addr.clone());
    let udp_iface = if adapter.is_multicast() {
        let udp_iface =
            transport.add_multicast_udp_interface(bind_addr.clone(), forward_addr.clone()).await;
        iface_manager.lock().await.set_mode(udp_iface, mode);
        udp_iface
    } else {
        iface_manager.lock().await.spawn_as_with_mode(
            adapter,
            UdpInterface::spawn,
            IfaceRole::Unicast,
            mode,
        )
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
    true
}

async fn startup_auto(
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
) -> bool {
    match auto::build_native_startup_plan(iface) {
        Ok(plan) => {
            let adopted_count = plan.adopted_devices.len();
            let candidate_count = plan.candidates.len();
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
                ))
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
                    true
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
                    false
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
            false
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
    true
}

async fn startup_kiss(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
) -> bool {
    let adapter = match kiss::build_adapter(iface) {
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
        "[daemon] kiss enabled iface={} name={} device={} baud_rate={}",
        kiss_iface,
        label,
        iface.device.as_deref().unwrap_or("<unset>"),
        iface.baud_rate.unwrap_or_default()
    );
    let runtime_iface = kiss_iface.to_string();
    mark_interface_startup_status(record, "spawned", None, Some(runtime_iface.as_str()));
    true
}

async fn startup_kiss_tcp_client(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
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
    true
}

async fn startup_ble(
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
) -> bool {
    match ble::spawn(iface_manager.clone(), iface).await {
        Ok(ble_iface) => {
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
