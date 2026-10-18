use super::*;

pub(super) async fn spawn_trusted_stream(
    iface_manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
    config: &reticulum_daemon::config::InterfaceConfig,
) -> Option<(AddressHash, Option<HotApplyRuntimeStatus>)> {
    let (Some(host), Some(port)) = (config.host.as_deref(), config.port) else {
        return None;
    };
    let adapter =
        crate::bootstrap::build_tcp_client_adapter(format!("{host}:{port}"), config, None);
    let status = adapter.runtime_status_handle();
    match crate::interfaces::common::spawn_configured(
        &mut *iface_manager.lock().await,
        adapter,
        TcpClient::spawn,
        IfaceRole::Unicast,
        config,
    ) {
        Ok(address) => Some((address, Some(HotApplyRuntimeStatus::TcpClient(status)))),
        Err(error) => {
            log::warn!("[daemon] trusted stream policy rejected name={:?}: {error}", config.name);
            None
        }
    }
}

pub(super) async fn spawn_hot_apply_interface(
    iface_manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
    transport: Option<&Arc<Transport>>,
    record: &InterfaceRecord,
) -> Option<(AddressHash, Option<HotApplyRuntimeStatus>)> {
    let mode = interface_record_mode(record);
    let result = match record.kind.as_str() {
        "tcp_client" => {
            let adapter = TcpClient::new(tcp_endpoint(record)?);
            let status = adapter.runtime_status_handle();
            let address = {
                let mut manager = iface_manager.lock().await;
                manager.try_spawn_as_with_mode(
                    adapter,
                    TcpClient::spawn,
                    IfaceRole::Unicast,
                    mode,
                    |manager, address| apply_record_runtime_config(manager, address, record),
                )
            };
            address.map(|address| (address, Some(HotApplyRuntimeStatus::TcpClient(status))))
        }
        "tcp_server" => {
            let bind_addr = match tcp_server_bind_addr(record) {
                Ok(bind_addr) => bind_addr,
                Err(error) => {
                    log::warn!(
                        "[daemon] hot-apply tcp_server rejected invalid bind address: {}",
                        error
                    );
                    return None;
                }
            };
            let mut adapter = TcpServer::new(bind_addr, iface_manager.clone());
            if let Some(client_mtu) = tcp_server_client_mtu(record) {
                adapter = adapter.with_client_mtu(client_mtu);
            }
            let status = adapter.runtime_status_handle();
            let address = {
                let mut manager = iface_manager.lock().await;
                manager.try_spawn_as_with_mode(
                    adapter,
                    TcpServer::spawn,
                    IfaceRole::Unicast,
                    mode,
                    |manager, address| apply_record_runtime_config(manager, address, record),
                )
            };
            address.map(|address| (address, Some(HotApplyRuntimeStatus::TcpListener(status))))
        }
        "udp" => {
            let (bind_addr, forward_addr) = match udp_bind_and_forward_addr(record) {
                Ok(addresses) => addresses,
                Err(error) => {
                    log::warn!(
                        "[daemon] hot-apply udp rejected invalid bind/forward address: {error}"
                    );
                    return None;
                }
            };
            let adapter = UdpInterface::new(bind_addr.clone(), forward_addr.clone());
            if adapter.is_multicast() {
                let transport = transport?;
                let result = transport
                    .try_add_multicast_udp_interface_with_status(
                        bind_addr,
                        forward_addr,
                        mode,
                        |manager, address| apply_record_runtime_config(manager, address, record),
                    )
                    .await;
                result.map(|(address, status)| (address, Some(HotApplyRuntimeStatus::Udp(status))))
            } else {
                let status = adapter.runtime_status_handle();
                iface_manager
                    .lock()
                    .await
                    .try_spawn_as_with_mode(
                        adapter,
                        UdpInterface::spawn,
                        IfaceRole::Unicast,
                        mode,
                        |manager, address| apply_record_runtime_config(manager, address, record),
                    )
                    .map(|address| (address, Some(HotApplyRuntimeStatus::Udp(status))))
            }
        }
        "pipe" => {
            let adapter = match pipe_adapter(record) {
                Ok(adapter) => adapter,
                Err(error) => {
                    log::warn!("[daemon] hot-apply pipe rejected invalid command: {error}");
                    return None;
                }
            };
            let status = adapter.runtime_status_handle();
            let address = iface_manager.lock().await.try_spawn_as_with_mode(
                adapter,
                PipeInterface::spawn,
                IfaceRole::Unicast,
                mode,
                |manager, address| apply_record_runtime_config(manager, address, record),
            );
            address.map(|address| (address, Some(HotApplyRuntimeStatus::Pipe(status))))
        }
        _ => return None,
    };
    match result {
        Ok(spawned) => Some(spawned),
        Err(error) => {
            log::warn!(
                "[daemon] hot-apply {} policy rejected before startup: {error}",
                record.kind
            );
            None
        }
    }
}
