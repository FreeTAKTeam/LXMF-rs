use super::{native, BleRuntimeSettings, BleSpawnResult};
use reticulum_daemon::config::InterfaceConfig;
use rns_transport::iface::InterfaceManager;
use std::sync::Arc;

pub(super) async fn spawn(
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    iface: &InterfaceConfig,
    settings: BleRuntimeSettings,
) -> Result<BleSpawnResult, String> {
    native::spawn_with_backend("linux", iface_manager, iface, settings).await
}
