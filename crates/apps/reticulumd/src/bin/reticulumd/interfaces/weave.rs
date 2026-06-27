use reticulum_daemon::config::InterfaceConfig;
use rns_transport::iface::weave::WeaveInterface;
use rns_transport::iface::InterfaceManager;

pub(crate) fn build_adapter(
    iface: &InterfaceConfig,
    iface_manager: std::sync::Arc<tokio::sync::Mutex<InterfaceManager>>,
) -> Result<WeaveInterface, String> {
    let device = iface
        .device
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "weave.device is required".to_string())?;
    let baud_rate = iface.baud_rate.ok_or_else(|| "weave.baud_rate is required".to_string())?;
    if baud_rate == 0 {
        return Err("weave.baud_rate must be > 0".to_string());
    }

    Ok(WeaveInterface::new(device.to_string(), iface_manager)
        .with_baud_rate(baud_rate)
        .with_mtu(iface.mtu.unwrap_or(1024)))
}
