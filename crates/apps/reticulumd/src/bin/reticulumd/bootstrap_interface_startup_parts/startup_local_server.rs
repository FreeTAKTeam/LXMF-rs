#[cfg_attr(not(unix), allow(dead_code))]
enum LocalUnixStartup {
    Active(AddressHash),
    Attached(AddressHash),
    Failed,
}

enum LocalTcpSidecarStartup {
    Active(AddressHash),
    Attached(AddressHash),
    Failed,
}

#[allow(clippy::too_many_arguments)]
async fn startup_synthetic_local_tcp_sidecar(
    strict_interface_startup: bool,
    iface: &InterfaceConfig,
    label: &str,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    shared_reconnect_events: Option<tokio::sync::mpsc::Sender<AddressHash>>,
    transport: &Transport,
) -> LocalTcpSidecarStartup {
    let iface_manager = &transport.iface_manager();
    let Some(port) = iface.port else {
        record_startup_failure(
            record,
            startup_failures,
            label.to_string(),
            iface.kind.clone(),
            "synthetic local shared-instance requires port for startup".to_string(),
        );
        return LocalTcpSidecarStartup::Failed;
    };

    let host = match tcp_listener_bind_host(iface) {
        Ok(host) => host,
        Err(err) => {
            record_startup_failure(
                record,
                startup_failures,
                label.to_string(),
                iface.kind.clone(),
                err,
            );
            return LocalTcpSidecarStartup::Failed;
        }
    };
    let bind_addr = tcp_bind_addr(host.as_str(), port);
    if tcp_bind_addr_is_in_use(&bind_addr) {
        if let Some(client_iface) = startup_local_tcp_attach_endpoint(
            strict_interface_startup,
            iface,
            label,
            bind_addr.as_str(),
            record,
            startup_failures,
            shared_reconnect_events,
            transport,
        )
        .await
        {
            return LocalTcpSidecarStartup::Attached(client_iface);
        }
        return LocalTcpSidecarStartup::Failed;
    }

    let mut adapter = TcpServer::new(bind_addr.clone(), iface_manager.clone())
        .with_plaintext_shared_clients(iface.synthetic_shared_instance)
        .with_client_mtu(iface.mtu.unwrap_or(TcpClient::DEFAULT_MTU))
        .with_prefer_ipv6(iface.prefer_ipv6.unwrap_or(false));
    if let Some(bitrate_bps) = iface.force_shared_instance_bitrate {
        adapter = adapter.with_client_forced_bitrate(bitrate_bps);
    }
    let Some(local_iface) = record_prepared_spawn(
        spawn_configured(
            &mut *iface_manager.lock().await,
            adapter,
            TcpServer::spawn,
            IfaceRole::Unicast,
            iface,
        ),
        record,
        startup_failures,
        label,
    ) else {
        return LocalTcpSidecarStartup::Failed;
    };
    log::info!(
        "[daemon] synthetic local tcp sidecar enabled iface={} name={} bind={}",
        local_iface,
        label,
        bind_addr
    );
    let runtime_iface = local_iface.to_string();
    mark_interface_startup_status(record, "active", None, Some(runtime_iface.as_str()));
    LocalTcpSidecarStartup::Active(local_iface)
}

#[cfg(unix)]
async fn startup_local_unix(
    strict_interface_startup: bool,
    iface: &InterfaceConfig,
    label: &str,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    shared_reconnect_events: Option<tokio::sync::mpsc::Sender<AddressHash>>,
    transport: &Transport,
) -> LocalUnixStartup {
    let iface_manager = &transport.iface_manager();
    let Some(socket_path) =
        iface.socket_path.as_deref().map(str::trim).filter(|value| !value.is_empty())
    else {
        record_startup_failure(
            record,
            startup_failures,
            label.to_string(),
            iface.kind.clone(),
            "local unix requires socket_path for startup".to_string(),
        );
        return LocalUnixStartup::Failed;
    };

    let endpoint = rns_transport::iface::local::LocalUnixEndpoint::from_config_value(socket_path);
    match rns_transport::iface::local::LocalUnixServer::preflight_bind_available(&endpoint).await {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::AddrInUse => {
            return startup_local_unix_attach(
                strict_interface_startup,
                iface,
                label,
                endpoint,
                record,
                startup_failures,
                shared_reconnect_events,
                transport,
            )
            .await;
        }
        Err(err) => {
            record_startup_failure(
                record,
                startup_failures,
                label.to_string(),
                iface.kind.clone(),
                format!(
                    "local unix bind preflight failed endpoint={} err={}",
                    endpoint.label(),
                    err
                ),
            );
            return LocalUnixStartup::Failed;
        }
    }

    let adapter = rns_transport::iface::local::LocalUnixServer::new_endpoint(
        endpoint.clone(),
        iface_manager.clone(),
    )
    .with_plaintext_shared_clients(iface.synthetic_shared_instance)
    .with_client_mtu(iface.mtu.unwrap_or(TcpClient::DEFAULT_MTU));
    let adapter = if let Some(bitrate_bps) = iface.force_shared_instance_bitrate {
        adapter.with_client_forced_bitrate(bitrate_bps)
    } else {
        adapter
    };
    let Some(local_iface) = record_prepared_spawn(
        spawn_configured(
            &mut *iface_manager.lock().await,
            adapter,
            rns_transport::iface::local::LocalUnixServer::spawn,
            IfaceRole::Unicast,
            iface,
        ),
        record,
        startup_failures,
        label,
    ) else {
        return LocalUnixStartup::Failed;
    };
    log::info!(
        "[daemon] local unix enabled iface={} name={} socket_path={}",
        local_iface,
        label,
        endpoint.label()
    );
    let runtime_iface = local_iface.to_string();
    mark_interface_startup_status(record, "active", None, Some(runtime_iface.as_str()));
    LocalUnixStartup::Active(local_iface)
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
async fn startup_local_unix_attach(
    strict_interface_startup: bool,
    iface: &InterfaceConfig,
    label: &str,
    endpoint: rns_transport::iface::local::LocalUnixEndpoint,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    shared_reconnect_events: Option<tokio::sync::mpsc::Sender<AddressHash>>,
    transport: &Transport,
) -> LocalUnixStartup {
    let iface_manager = &transport.iface_manager();
    if strict_interface_startup {
        if let Err(err) = rns_transport::iface::local::preflight_unix_connect(&endpoint).await {
            record_startup_failure(
                record,
                startup_failures,
                label.to_string(),
                iface.kind.clone(),
                err,
            );
            return LocalUnixStartup::Failed;
        }
    }

    let mut adapter = rns_transport::iface::local::LocalUnixClient::new_connect(endpoint.clone())
        .with_mtu(iface.mtu.unwrap_or(TcpClient::DEFAULT_MTU));
    if let Some(bitrate_bps) = iface.force_shared_instance_bitrate {
        adapter = adapter.with_forced_bitrate(bitrate_bps);
    }
    if let Some(events) = shared_reconnect_events {
        adapter = adapter.with_reconnect_events(events);
    }
    let Some(client_iface) = record_prepared_spawn(
        spawn_configured_attachment(
            iface_manager,
            transport,
            adapter,
            rns_transport::iface::local::LocalUnixClient::spawn,
            iface,
        )
        .await,
        record,
        startup_failures,
        label,
    ) else {
        return LocalUnixStartup::Failed;
    };
    log::info!(
        "[daemon] local unix attached iface={} name={} socket_path={}",
        client_iface,
        label,
        endpoint.label()
    );
    let runtime_iface = client_iface.to_string();
    mark_interface_startup_status(record, "attached", None, Some(runtime_iface.as_str()));
    LocalUnixStartup::Attached(client_iface)
}

#[cfg(not(unix))]
async fn startup_local_unix(
    _strict_interface_startup: bool,
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
        "local unix shared_instance_type is only supported on unix platforms".to_string(),
    );
    LocalUnixStartup::Failed
}
