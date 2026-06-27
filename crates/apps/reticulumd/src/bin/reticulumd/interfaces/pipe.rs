use reticulum_daemon::config::InterfaceConfig;
use rns_transport::iface::pipe::PipeInterface;
use std::time::Duration;

pub(crate) fn build_adapter(iface: &InterfaceConfig) -> Result<PipeInterface, String> {
    let command = iface
        .command
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "pipe.command is required".to_string())?;
    PipeInterface::parse_command(command)?;

    let respawn_delay = iface.respawn_delay.unwrap_or(5.0);
    if respawn_delay < 0.0 || !respawn_delay.is_finite() {
        return Err("pipe.respawn_delay must be finite and >= 0".to_string());
    }

    Ok(PipeInterface::new(command.to_string())
        .with_respawn_delay(Duration::from_secs_f64(respawn_delay))
        .with_mtu(iface.mtu.unwrap_or(PipeInterface::DEFAULT_MTU)))
}
