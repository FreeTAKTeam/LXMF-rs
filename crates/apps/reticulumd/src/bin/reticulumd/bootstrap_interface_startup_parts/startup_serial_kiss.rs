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

    let status = adapter.runtime_status_handle();
    let Some(serial_iface) = record_prepared_spawn(
        spawn_configured(
            &mut *iface_manager.lock().await,
            adapter,
            |context| async move { rns_transport::iface::serial::SerialInterface::spawn(context).await },
            IfaceRole::Unicast,
            iface,
        ),
        record,
        startup_failures,
        label,
    ) else {
        return false;
    };
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

    let status = adapter.runtime_status_handle();
    let Some(kiss_iface) = record_prepared_spawn(
        spawn_configured(
            &mut *iface_manager.lock().await,
            adapter,
            |context| async move { rns_transport::iface::kiss::KissInterface::spawn(context).await },
            IfaceRole::Unicast,
            iface,
        ),
        record,
        startup_failures,
        label,
    ) else {
        return false;
    };
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

    let status = adapter.runtime_status_handle();
    let Some(kiss_iface) = record_prepared_spawn(
        spawn_configured(
            &mut *iface_manager.lock().await,
            adapter,
            |context| async move {
                rns_transport::iface::kiss::KissTcpClientInterface::spawn(context).await;
            },
            IfaceRole::Unicast,
            iface,
        ),
        record,
        startup_failures,
        label,
    ) else {
        return false;
    };
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
