#[allow(clippy::too_many_arguments)]
async fn startup_local_tcp_attach(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    selected_tcp_server: &TcpServerSelection,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    shared_reconnect_events: Option<tokio::sync::mpsc::Sender<AddressHash>>,
    transport: &Transport,
) -> Option<AddressHash> {
    let Some(endpoint) = selected_tcp_server.local_attach_addr.as_deref() else {
        record_startup_failure(
            record,
            startup_failures,
            label.to_string(),
            iface.kind.clone(),
            "local attach requires a selected shared-instance endpoint".to_string(),
        );
        return None;
    };

    startup_local_tcp_attach_endpoint(
        args,
        iface,
        label,
        endpoint,
        record,
        startup_failures,
        shared_reconnect_events,
        transport,
    )
    .await
}

async fn startup_local_tcp_client_attach(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    shared_reconnect_events: Option<tokio::sync::mpsc::Sender<AddressHash>>,
    transport: &Transport,
) -> Option<AddressHash> {
    let (Some(host), Some(port)) = (iface.host.as_deref(), iface.port) else {
        record_startup_failure(
            record,
            startup_failures,
            label.to_string(),
            iface.kind.clone(),
            "local_client attach requires host and port for startup".to_string(),
        );
        return None;
    };
    let endpoint = format!("{}:{}", host, port);
    startup_local_tcp_attach_endpoint(
        args,
        iface,
        label,
        endpoint.as_str(),
        record,
        startup_failures,
        shared_reconnect_events,
        transport,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn startup_local_tcp_attach_endpoint(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    endpoint: &str,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    shared_reconnect_events: Option<tokio::sync::mpsc::Sender<AddressHash>>,
    transport: &Transport,
) -> Option<AddressHash> {
    let iface_manager = &transport.iface_manager();
    if args.strict_interface_startup {
        if let Err(err) = strict_tcp_client_preflight(endpoint).await {
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

    let mut adapter = build_tcp_client_adapter(endpoint.to_string(), iface, None);
    if let Some(events) = shared_reconnect_events {
        adapter = adapter.with_reconnect_events(events);
    }
    let client_iface = record_prepared_spawn(
        spawn_configured_attachment(iface_manager, transport, adapter, TcpClient::spawn, iface)
            .await,
        record,
        startup_failures,
        label,
    )?;
    log::info!(
        "[daemon] local attached iface={} name={} endpoint={}",
        client_iface,
        label,
        endpoint
    );
    let runtime_iface = client_iface.to_string();
    mark_interface_startup_status(record, "attached", None, Some(runtime_iface.as_str()));
    Some(client_iface)
}

#[cfg(unix)]
async fn startup_local_unix_client_attach(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    shared_reconnect_events: Option<tokio::sync::mpsc::Sender<AddressHash>>,
    transport: &Transport,
) -> LocalUnixStartup {
    let Some(socket_path) =
        iface.socket_path.as_deref().map(str::trim).filter(|value| !value.is_empty())
    else {
        record_startup_failure(
            record,
            startup_failures,
            label.to_string(),
            iface.kind.clone(),
            "local_client unix requires socket_path for startup".to_string(),
        );
        return LocalUnixStartup::Failed;
    };
    let endpoint = rns_transport::iface::local::LocalUnixEndpoint::from_config_value(socket_path);
    startup_local_unix_attach(
        args,
        iface,
        label,
        endpoint,
        record,
        startup_failures,
        shared_reconnect_events,
        transport,
    )
    .await
}

#[cfg(not(unix))]
async fn startup_local_unix_client_attach(
    _args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    _shared_reconnect_events: Option<tokio::sync::mpsc::Sender<AddressHash>>,
    _transport: &Transport,
) -> LocalUnixStartup {
    record_startup_failure(
        record,
        startup_failures,
        label.to_string(),
        iface.kind.clone(),
        "local_client unix shared_instance_type is only supported on unix platforms".to_string(),
    );
    LocalUnixStartup::Failed
}
