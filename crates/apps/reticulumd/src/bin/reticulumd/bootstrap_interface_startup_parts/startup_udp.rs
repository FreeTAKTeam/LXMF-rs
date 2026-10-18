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
    let spawned = if is_multicast {
        transport
            .try_add_multicast_udp_interface_with_status(
                bind_addr.clone(),
                forward_addr.clone(),
                mode,
                |manager, address| apply_interface_runtime_config(manager, address, iface),
            )
            .await
    } else {
        spawn_configured(
            &mut *iface_manager.lock().await,
            adapter,
            UdpInterface::spawn,
            IfaceRole::Unicast,
            iface,
        )
        .map(|address| (address, status))
    };
    let Some((udp_iface, status)) =
        record_prepared_spawn(spawned, record, sinks.startup_failures, label)
    else {
        return false;
    };
    sinks.runtime_refreshes.push(UdpRuntimeRefresh { runtime_iface: udp_iface, status });
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
    if let Some(key) = hot_apply_interface_seed_key(record) {
        sinks.seeded_hot_apply_interfaces.push((key, record.clone(), udp_iface));
    }
    true
}

async fn startup_auto<F>(
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    plan_builder: &mut F,
) -> Option<(AutoRuntimeRefresh, AutoRuntimeShutdown)>
where
    F: FnMut(&InterfaceConfig) -> Result<auto::AutoRuntimePlan, String>,
{
    match plan_builder(iface) {
        Ok(plan) => {
            let adopted_count = plan.adopted_devices.len();
            let candidate_count = plan.candidates.len();
            with_interface_runtime_metadata(record, |runtime| {
                runtime.insert("auto".to_string(), plan.runtime_json());
            });
            match activate_auto_plan(&plan, iface, iface_manager).await {
                Ok(activation) => {
                    let summary = &activation.runtime.summary;
                    let host_iface = activation.host_iface;
                    let runtime_iface = host_iface.to_string();
                    with_interface_runtime_metadata(record, |runtime| {
                        runtime.insert(
                            "auto_discovery_runtime".to_string(),
                            auto::discovery_runtime_summary_json(summary),
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
                    Some((
                        AutoRuntimeRefresh { runtime_iface: host_iface, status: activation.status },
                        AutoRuntimeShutdown {
                            host_iface,
                            runtime: activation.runtime,
                            #[cfg(test)]
                            iface_manager: Arc::clone(iface_manager),
                        },
                    ))
                }
                Err(err) => {
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

/// Activates an already-built AutoInterface plan through the daemon's
/// InterfaceManager channel and transport adapter. Native discovery remains
/// the caller's responsibility so software tests can supply owned bindings.
async fn activate_auto_plan(
    plan: &auto::AutoRuntimePlan,
    iface: &InterfaceConfig,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
) -> Result<AutoInterfaceActivation, String> {
    let runtime_status = auto::AutoRuntimeStatusHandle::from_startup_plan(&plan.startup_plan);
    let mode = iface.interface_mode().unwrap_or(InterfaceMode::Full);
    let (host_iface, transport_runtime) = {
        let mut manager = iface_manager.lock().await;
        let channel = manager.new_channel_with_role_and_mode(128, IfaceRole::Multicast, mode);
        let host_iface = channel.address;
        if let Err(error) = apply_interface_runtime_config(&mut manager, host_iface, iface) {
            manager.stop_interface(host_iface);
            return Err(format!("AutoInterface runtime policy could not be installed: {error}"));
        }
        (
            host_iface,
            auto::AutoInterfaceTransportRuntime::from_channel(channel, Arc::clone(iface_manager)),
        )
    };

    match plan
        .spawn_discovery_runtime_with_native_scope_ids_and_transport(
            Some(transport_runtime),
            Some(runtime_status.clone()),
        )
        .await
    {
        Ok(runtime) => Ok(AutoInterfaceActivation { host_iface, runtime, status: runtime_status }),
        Err(error) => {
            if !iface_manager.lock().await.stop_interface(host_iface) {
                log::debug!(
                    "AutoInterface host already absent after startup failure iface={host_iface}"
                );
            }
            Err(error)
        }
    }
}

struct AutoInterfaceActivation {
    host_iface: rns_transport::hash::AddressHash,
    runtime: rns_transport::iface::auto_runtime::AutoDiscoveryRuntime,
    status: auto::AutoRuntimeStatusHandle,
}

impl AutoInterfaceActivation {
    #[cfg(test)]
    async fn stop(
        self,
        iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    ) -> bool {
        self.runtime.stop().await;
        iface_manager.lock().await.stop_interface(self.host_iface)
    }
}
