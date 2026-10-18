struct ConfiguredStartupContext<'a> {
    args: &'a Args,
    selected_tcp_server: &'a TcpServerSelection,
    transport: &'a Transport,
    iface_manager: &'a Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    server_iface: Option<&'a AddressHash>,
    reticulum_storage_path: &'a std::path::Path,
    shared_reconnect_events: Option<tokio::sync::mpsc::Sender<AddressHash>>,
    transport_identity_hash: Option<[u8; 16]>,
}

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
    let context = ConfiguredStartupContext {
        args,
        selected_tcp_server,
        transport,
        iface_manager,
        server_iface,
        reticulum_storage_path,
        shared_reconnect_events,
        transport_identity_hash,
    };
    let mut batch = InterfaceStartupBatch::default();
    for (index, iface) in config.interfaces.iter().enumerate() {
        if !iface.enabled() {
            continue;
        }
        startup_configured_interface_with_auto_plan_builder(
            &context,
            index,
            iface,
            &mut configured_interfaces[index],
            &mut batch,
            &mut auto_plan_builder,
        )
        .await;
    }
    batch
}
