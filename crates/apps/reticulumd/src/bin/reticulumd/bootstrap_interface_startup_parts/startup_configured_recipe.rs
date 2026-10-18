async fn startup_configured_interface_with_auto_plan_builder<F>(
    context: &ConfiguredStartupContext<'_>,
    index: usize,
    iface: &InterfaceConfig,
    record: &mut InterfaceRecord,
    batch: &mut InterfaceStartupBatch,
    auto_plan_builder: &mut F,
) where
    F: FnMut(&InterfaceConfig) -> Result<auto::AutoRuntimePlan, String>,
{
    let args = context.args;
    let selected_tcp_server = context.selected_tcp_server;
    let transport = context.transport;
    let iface_manager = context.iface_manager;
    let server_iface = context.server_iface;
    let reticulum_storage_path = context.reticulum_storage_path;
    let transport_identity_hash = context.transport_identity_hash;
    let InterfaceStartupBatch {
        configured_roots,
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
    } = batch;
    if !iface.enabled() {
        return;
    }
    let label = interface_label(iface, index);
    let mut native_root = None;
    match iface.kind.as_str() {
        "tcp_server" | "backbone" => {
            if selected_tcp_server.selected_index == Some(index) && iface.port.is_some() {
                native_root = server_iface.copied();
            }
            if let Some(seed) = startup_tcp_server_record(
                index,
                iface,
                &label,
                selected_tcp_server,
                server_iface,
                record,
                startup_failures,
            ) {
                native_root = Some(seed.2);
                seeded_hot_apply_interfaces.push(seed);
            }
        }
        "local" if iface.shared_instance_type.as_deref() == Some("unix") => {
            match startup_local_unix(
                args,
                iface,
                &label,
                record,
                startup_failures,
                context.shared_reconnect_events.clone(),
                transport,
            )
            .await
            {
                LocalUnixStartup::Active(address) => {
                    native_root = Some(address);
                    *startup_successes += 1;
                }
                LocalUnixStartup::Attached(client_iface) => {
                    *startup_successes += 1;
                    native_root = Some(client_iface);
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
                record,
                startup_failures,
                context.shared_reconnect_events.clone(),
                transport,
            )
            .await
            {
                LocalUnixStartup::Attached(client_iface) => {
                    *startup_successes += 1;
                    native_root = Some(client_iface);
                    tunnel_synth_ifaces.push(client_iface);
                }
                LocalUnixStartup::Active(_) | LocalUnixStartup::Failed => {}
            }
        }
        "local" => {
            if selected_tcp_server.local_attach_index == Some(index) {
                if let Some(client_iface) = startup_local_tcp_attach(
                    args,
                    iface,
                    &label,
                    selected_tcp_server,
                    record,
                    startup_failures,
                    context.shared_reconnect_events.clone(),
                    transport,
                )
                .await
                {
                    *startup_successes += 1;
                    native_root = Some(client_iface);
                    tunnel_synth_ifaces.push(client_iface);
                }
            } else if iface.synthetic_shared_instance
                && selected_tcp_server.selected_index != Some(index)
            {
                match startup_synthetic_local_tcp_sidecar(
                    args,
                    iface,
                    &label,
                    record,
                    startup_failures,
                    context.shared_reconnect_events.clone(),
                    transport,
                )
                .await
                {
                    LocalTcpSidecarStartup::Active(address) => {
                        native_root = Some(address);
                        *startup_successes += 1;
                    }
                    LocalTcpSidecarStartup::Attached(client_iface) => {
                        *startup_successes += 1;
                        native_root = Some(client_iface);
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
                record,
                startup_failures,
            ) {
                native_root = Some(seed.2);
                seeded_hot_apply_interfaces.push(seed);
            }
            if selected_tcp_server.selected_index == Some(index) && iface.port.is_some() {
                native_root = server_iface.copied();
            }
        }
        "local_client" => {
            if let Some(client_iface) = startup_local_tcp_client_attach(
                args,
                iface,
                &label,
                record,
                startup_failures,
                context.shared_reconnect_events.clone(),
                transport,
            )
            .await
            {
                *startup_successes += 1;
                native_root = Some(client_iface);
                tunnel_synth_ifaces.push(client_iface);
            }
        }
        "tcp_client" | "backbone_client" => {
            if let Some(client_iface) = startup_tcp_client(
                args,
                iface,
                &label,
                iface_manager,
                record,
                startup_failures,
                seeded_hot_apply_interfaces,
                tcp_runtime_refreshes,
                context.shared_reconnect_events.clone(),
            )
            .await
            {
                *startup_successes += 1;
                native_root = Some(client_iface);
                tunnel_synth_ifaces.push(client_iface);
            }
        }
        "udp" => {
            let mut sinks = UdpStartupSinks {
                startup_failures,
                runtime_refreshes: udp_runtime_refreshes,
                seeded_hot_apply_interfaces,
            };
            if startup_udp(args, iface, &label, transport, iface_manager, record, &mut sinks).await
            {
                *startup_successes += 1;
                native_root = udp_runtime_refreshes.last().map(|refresh| refresh.runtime_iface);
            }
        }
        "auto" => {
            if let Some((refresh, shutdown)) = startup_auto(
                iface,
                &label,
                iface_manager,
                record,
                startup_failures,
                auto_plan_builder,
            )
            .await
            {
                *startup_successes += 1;
                native_root = Some(refresh.runtime_iface);
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
                record,
                startup_failures,
                serial_runtime_refreshes,
            )
            .await
            {
                *startup_successes += 1;
                native_root = serial_runtime_refreshes.last().map(|refresh| refresh.runtime_iface);
            }
        }
        "weave" => {
            if let Some(refresh) =
                startup_weave(args, iface, &label, iface_manager, record, startup_failures).await
            {
                *startup_successes += 1;
                weave_control_bindings.push(WeaveControlBinding {
                    runtime_iface: refresh.runtime_iface,
                    name: label.clone(),
                    handle: refresh.handle.clone(),
                });
                native_root = Some(refresh.runtime_iface);
                weave_runtime_refreshes.push(refresh);
            }
        }
        "kiss" | "ax25_kiss" => {
            if startup_kiss(
                args,
                iface,
                &label,
                iface_manager,
                record,
                startup_failures,
                kiss_runtime_refreshes,
            )
            .await
            {
                *startup_successes += 1;
                native_root = kiss_runtime_refreshes.last().map(|refresh| refresh.runtime_iface);
            }
        }
        "kiss_tcp_client" => {
            if startup_kiss_tcp_client(
                args,
                iface,
                &label,
                iface_manager,
                record,
                startup_failures,
                kiss_runtime_refreshes,
            )
            .await
            {
                *startup_successes += 1;
                native_root = kiss_runtime_refreshes.last().map(|refresh| refresh.runtime_iface);
            }
        }
        "pipe" => {
            if let Some(refresh) =
                startup_pipe(iface, &label, iface_manager, record, startup_failures).await
            {
                *startup_successes += 1;
                native_root = Some(refresh.runtime_iface);
                pipe_runtime_refreshes.push(refresh);
            }
        }
        "i2p" => {
            if let Some(refresh) = startup_i2p(
                args,
                iface,
                &label,
                iface_manager,
                record,
                startup_failures,
                reticulum_storage_path,
                transport_identity_hash,
            )
            .await
            {
                *startup_successes += 1;
                native_root = Some(refresh.runtime_iface);
                i2p_runtime_refreshes.push(refresh);
            }
        }
        "meshtastic" => {
            if let Some(refresh) =
                startup_meshtastic(iface, &label, iface_manager, record, startup_failures).await
            {
                *startup_successes += 1;
                native_root = Some(refresh.runtime_iface);
                meshtastic_runtime_refreshes.push(refresh);
            }
        }
        "ble_gatt" => {
            if startup_ble(
                iface,
                &label,
                iface_manager,
                record,
                startup_failures,
                ble_gatt_runtime_refreshes,
            )
            .await
            {
                *startup_successes += 1;
                native_root =
                    ble_gatt_runtime_refreshes.last().map(|refresh| refresh.runtime_iface);
            }
        }
        "reticulum_ble" => {
            if startup_reticulum_ble(
                iface,
                &label,
                iface_manager,
                record,
                startup_failures,
                reticulum_ble_runtime_refreshes,
                transport_identity_hash,
            )
            .await
            {
                *startup_successes += 1;
                native_root =
                    reticulum_ble_runtime_refreshes.last().map(|refresh| refresh.runtime_iface);
            }
        }
        "vrn76_kiss_ble" => {
            let startup =
                startup_vrn76_kiss_ble(iface, &label, iface_manager, record, startup_failures)
                    .await;
            if startup.started {
                *startup_successes += 1;
                #[cfg(feature = "vrn76-kiss-ble")]
                if let Some(refresh) = startup.refresh {
                    native_root = Some(refresh.runtime_iface);
                    vrn76_runtime_refreshes.push(refresh);
                }
            }
        }
        "lora" => {
            let started =
                startup_lora(args, iface, &label, iface_manager, record, startup_failures).await;
            if started.started {
                *startup_successes += 1;
                if let Some(refresh) = started.refresh {
                    native_root = Some(refresh.runtime_iface);
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
                record,
                startup_failures,
                rnode_management_bindings,
            )
            .await
            {
                *startup_successes += 1;
                native_root = Some(refresh.runtime_iface);
                rnode_multi_runtime_refreshes.push(refresh);
            }
        }
        _ => record_startup_failure(
            record,
            startup_failures,
            label,
            iface.kind.clone(),
            format!("unsupported interface kind '{}'", iface.kind),
        ),
    }
    if let Some(address) = native_root {
        configured_roots.push((index, address));
    }
}
