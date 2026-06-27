use std::time::Duration;

use reticulum_daemon::config::InterfaceConfig;
use rns_transport::iface::i2p::I2pInterface;
use rns_transport::iface::InterfaceManager;

pub(crate) fn build_adapter(
    iface: &InterfaceConfig,
    iface_manager: std::sync::Arc<tokio::sync::Mutex<InterfaceManager>>,
) -> Result<I2pInterface, String> {
    let sam_host = iface
        .sam_host
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "i2p.sam_host is required".to_string())?;
    let sam_port = iface.sam_port.ok_or_else(|| "i2p.sam_port is required".to_string())?;
    let peers = iface.peers.clone().unwrap_or_default();
    let sam_endpoint = format!("{sam_host}:{sam_port}");
    let name =
        iface.name.as_deref().map(str::trim).filter(|value| !value.is_empty()).unwrap_or("i2p");

    let reconnect_wait = iface
        .reconnect_backoff_ms
        .map(Duration::from_millis)
        .unwrap_or_else(|| Duration::from_secs(15));

    Ok(I2pInterface::new(name.to_string(), iface_manager)
        .with_sam_endpoint(sam_endpoint)
        .with_peers(peers)
        .with_connectable(iface.connectable.unwrap_or(false))
        .with_state_path(iface.state_path.clone())
        .with_mtu(iface.mtu.unwrap_or(I2pInterface::DEFAULT_MTU))
        .with_reconnect_wait(reconnect_wait))
}
