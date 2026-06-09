use super::{native, BleRuntimeSettings};
use reticulum_daemon::config::InterfaceConfig;
use rns_transport::hash::AddressHash;
use rns_transport::iface::InterfaceManager;
use std::sync::Arc;

pub(super) async fn spawn(
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    iface: &InterfaceConfig,
    settings: BleRuntimeSettings,
) -> Result<AddressHash, String> {
    native::spawn_with_backend("android", iface_manager, iface, settings).await
}
