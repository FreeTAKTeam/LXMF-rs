#[allow(clippy::too_many_arguments)]
async fn startup_configured_interfaces_with_auto_plan_builder<F>(
    args: &Args,
    config: &DaemonConfig,
    selected_tcp_server: &TcpServerSelection,
    transport: &Transport,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    server_iface: Option<&AddressHash>,
    configured_interfaces: &mut [InterfaceRecord],
    reticulum_storage_path: &std::path::Path,
    shared_reconnect_events: Option<tokio::sync::mpsc::Sender<AddressHash>>,
    transport_identity_hash: Option<[u8; 16]>,
    mut auto_plan_builder: F,
) -> InterfaceStartupBatch
where
    F: FnMut(&InterfaceConfig) -> Result<auto::AutoRuntimePlan, String>,
{
    let mut startup_successes = 0usize;
    let mut startup_failures = Vec::new();
    let mut seeded_hot_apply_interfaces = Vec::new();
    let mut tunnel_synth_ifaces = Vec::new();
    let mut auto_runtime_refreshes = Vec::new();
    let mut auto_runtime_shutdowns = Vec::new();
    let mut pipe_runtime_refreshes = Vec::new();
    let mut udp_runtime_refreshes = Vec::new();
    let mut serial_runtime_refreshes = Vec::new();
    let mut kiss_runtime_refreshes = Vec::new();
    let mut ble_gatt_runtime_refreshes = Vec::new();
    let mut reticulum_ble_runtime_refreshes = Vec::new();
    let mut i2p_runtime_refreshes = Vec::new();
    let mut meshtastic_runtime_refreshes = Vec::new();
    let mut tcp_runtime_refreshes = Vec::new();
    let mut weave_runtime_refreshes = Vec::new();
    let mut rnode_multi_runtime_refreshes = Vec::new();
    let mut lora_runtime_refreshes = Vec::new();
    #[cfg(feature = "vrn76-kiss-ble")]
    let mut vrn76_runtime_refreshes = Vec::new();
    let mut rnode_management_bindings = Vec::new();
    let mut weave_control_bindings = Vec::new();

    for (index, iface) in config.interfaces.iter().enumerate() {
        if !iface.enabled() {
            continue;
        }
        let label = interface_label(iface, index);
        match iface.kind.as_str() {
            "tcp_server" | "backbone" => {
                if let Some(seed) = startup_tcp_server_record(
                    index,
                    iface,
                    &label,
                    selected_tcp_server,
                    server_iface,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                ) {
                    seeded_hot_apply_interfaces.push(seed);
                }
            }
            "local" if iface.shared_instance_type.as_deref() == Some("unix") => {
                match startup_local_unix(
                    args,
                    iface,
                    &label,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                    shared_reconnect_events.clone(),
                    transport,
                )
                .await
                {
                    LocalUnixStartup::Active => {
                        startup_successes += 1;
                    }
                    LocalUnixStartup::Attached(client_iface) => {
                        startup_successes += 1;
                        tunnel_synth_ifaces.push(client_iface);
                    }
                    LocalUnixStartup::Failed => {}
                }
            }
            "local_client" if iface.shared_instance_type.as_deref() == Some("unix") => {
                match startup_local_unix_client_attach(
                    args,
                    iface,
                    &label,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                    shared_reconnect_events.clone(),
                    transport,
                )
                .await
                {
                    LocalUnixStartup::Attached(client_iface) => {
                        startup_successes += 1;
                        tunnel_synth_ifaces.push(client_iface);
                    }
                    LocalUnixStartup::Active | LocalUnixStartup::Failed => {}
                }
            }
            "local" => {
                if selected_tcp_server.local_attach_index == Some(index) {
                    if let Some(client_iface) = startup_local_tcp_attach(
                        args,
                        iface,
                        &label,
                        selected_tcp_server,
                        &mut configured_interfaces[index],
                        &mut startup_failures,
                        shared_reconnect_events.clone(),
                        transport,
                    )
                    .await
                    {
                        startup_successes += 1;
                        tunnel_synth_ifaces.push(client_iface);
                    }
                } else if iface.synthetic_shared_instance
                    && selected_tcp_server.selected_index != Some(index)
                {
                    match startup_synthetic_local_tcp_sidecar(
                        args,
                        iface,
                        &label,
                        &mut configured_interfaces[index],
                        &mut startup_failures,
                        shared_reconnect_events.clone(),
                        transport,
                    )
                    .await
                    {
                        LocalTcpSidecarStartup::Active => {
                            startup_successes += 1;
                        }
                        LocalTcpSidecarStartup::Attached(client_iface) => {
                            startup_successes += 1;
                            tunnel_synth_ifaces.push(client_iface);
                        }
                        LocalTcpSidecarStartup::Failed => {}
                    }
                } else if let Some(seed) = startup_tcp_server_record(
                    index,
                    iface,
                    &label,
                    selected_tcp_server,
                    server_iface,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                ) {
                    seeded_hot_apply_interfaces.push(seed);
                }
            }
            "local_client" => {
                if let Some(client_iface) = startup_local_tcp_client_attach(
                    args,
                    iface,
                    &label,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                    shared_reconnect_events.clone(),
                    transport,
                )
                .await
                {
                    startup_successes += 1;
                    tunnel_synth_ifaces.push(client_iface);
                }
            }
            "tcp_client" | "backbone_client" => {
                if let Some(client_iface) = startup_tcp_client(
                    args,
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                    &mut seeded_hot_apply_interfaces,
                    &mut tcp_runtime_refreshes,
                    shared_reconnect_events.clone(),
                )
                .await
                {
                    startup_successes += 1;
                    tunnel_synth_ifaces.push(client_iface);
                }
            }
            "udp" => {
                let mut sinks = UdpStartupSinks {
                    startup_failures: &mut startup_failures,
                    runtime_refreshes: &mut udp_runtime_refreshes,
                    seeded_hot_apply_interfaces: &mut seeded_hot_apply_interfaces,
                };
                if startup_udp(
                    args,
                    iface,
                    &label,
                    transport,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut sinks,
                )
                .await
                {
                    startup_successes += 1;
                }
            }
            "auto" => {
                if let Some((refresh, shutdown)) = startup_auto(
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                    &mut auto_plan_builder,
                )
                .await
                {
                    startup_successes += 1;
                    auto_runtime_refreshes.push(refresh);
                    auto_runtime_shutdowns.push(shutdown);
                }
            }
            "serial" => {
                if startup_serial(
                    args,
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                    &mut serial_runtime_refreshes,
                )
                .await
                {
                    startup_successes += 1;
                }
            }
            "weave" => {
                if let Some(refresh) = startup_weave(
                    args,
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                )
                .await
                {
                    startup_successes += 1;
                    weave_control_bindings.push(WeaveControlBinding {
                        runtime_iface: refresh.runtime_iface,
                        name: label.clone(),
                        handle: refresh.handle.clone(),
                    });
                    weave_runtime_refreshes.push(refresh);
                }
            }
            "kiss" | "ax25_kiss" => {
                if startup_kiss(
                    args,
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                    &mut kiss_runtime_refreshes,
                )
                .await
                {
                    startup_successes += 1;
                }
            }
            "kiss_tcp_client" => {
                if startup_kiss_tcp_client(
                    args,
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                    &mut kiss_runtime_refreshes,
                )
                .await
                {
                    startup_successes += 1;
                }
            }
            "pipe" => {
                if let Some(refresh) = startup_pipe(
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                )
                .await
                {
                    startup_successes += 1;
                    pipe_runtime_refreshes.push(refresh);
                }
            }
            "i2p" => {
                if let Some(refresh) = startup_i2p(
                    args,
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                    reticulum_storage_path,
                    transport_identity_hash,
                )
                .await
                {
                    startup_successes += 1;
                    i2p_runtime_refreshes.push(refresh);
                }
            }
            "meshtastic" => {
                if let Some(refresh) = startup_meshtastic(
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                )
                .await
                {
                    startup_successes += 1;
                    meshtastic_runtime_refreshes.push(refresh);
                }
            }
            "ble_gatt" => {
                if startup_ble(
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                    &mut ble_gatt_runtime_refreshes,
                )
                .await
                {
                    startup_successes += 1;
                }
            }
            "reticulum_ble" => {
                if startup_reticulum_ble(
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                    &mut reticulum_ble_runtime_refreshes,
                    transport_identity_hash,
                )
                .await
                {
                    startup_successes += 1;
                }
            }
            "vrn76_kiss_ble" => {
                let startup = startup_vrn76_kiss_ble(
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                )
                .await;
                if startup.started {
                    startup_successes += 1;
                    #[cfg(feature = "vrn76-kiss-ble")]
                    if let Some(refresh) = startup.refresh {
                        vrn76_runtime_refreshes.push(refresh);
                    }
                }
            }
            "lora" => {
                let started = startup_lora(
                    args,
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                )
                .await;
                if started.started {
                    startup_successes += 1;
                    if let Some(refresh) = started.refresh {
                        lora_runtime_refreshes.push(refresh);
                    }
                    if let Some(binding) = started.management_binding {
                        rnode_management_bindings.push(binding);
                    }
                }
            }
            "rnode_multi" => {
                if let Some(refresh) = startup_rnode_multi(
                    args,
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                    &mut rnode_management_bindings,
                )
                .await
                {
                    startup_successes += 1;
                    rnode_multi_runtime_refreshes.push(refresh);
                }
            }
            _ => record_startup_failure(
                &mut configured_interfaces[index],
                &mut startup_failures,
                label,
                iface.kind.clone(),
                format!("unsupported interface kind '{}'", iface.kind),
            ),
        }
    }

    InterfaceStartupBatch {
        startup_successes,
        startup_failures,
        seeded_hot_apply_interfaces,
        tunnel_synth_ifaces,
        auto_runtime_refreshes,
        auto_runtime_shutdowns,
        pipe_runtime_refreshes,
        udp_runtime_refreshes,
        serial_runtime_refreshes,
        kiss_runtime_refreshes,
        ble_gatt_runtime_refreshes,
        reticulum_ble_runtime_refreshes,
        i2p_runtime_refreshes,
        meshtastic_runtime_refreshes,
        tcp_runtime_refreshes,
        weave_runtime_refreshes,
        rnode_multi_runtime_refreshes,
        lora_runtime_refreshes,
        #[cfg(feature = "vrn76-kiss-ble")]
        vrn76_runtime_refreshes,
        rnode_management_bindings,
        weave_control_bindings,
    }
}
