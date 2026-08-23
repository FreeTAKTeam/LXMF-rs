use alloc::sync::Arc;
use tokio::sync::Mutex;

use crate::hash::AddressHash;
use crate::iface::InterfaceManager;
use crate::packet::IfacFlag;

pub(super) async fn violates_ifac_policy(
    iface_manager: &Arc<Mutex<InterfaceManager>>,
    address: AddressHash,
    flag: IfacFlag,
) -> bool {
    let ifac_enabled = iface_manager
        .lock()
        .await
        .shared_config(&address)
        .is_some_and(|config| config.network_name.is_some() || config.passphrase.is_some());
    let ifac_flag_set = flag == IfacFlag::Authenticated;
    let description = match (ifac_enabled, ifac_flag_set) {
        (true, false) => Some("missing IFAC flag on IFAC-enabled interface"),
        (false, true) => Some("IFAC flag set on interface without IFAC enabled"),
        _ => None,
    };
    if let Some(description) = description {
        iface_manager.lock().await.record_ifac_violation(address, description);
        true
    } else {
        false
    }
}
