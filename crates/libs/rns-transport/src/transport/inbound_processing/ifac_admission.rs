use alloc::sync::Arc;
use tokio::sync::Mutex;

use crate::hash::AddressHash;
use crate::iface::InterfaceManager;
use crate::packet::IfacFlag;

pub(super) async fn violates_ifac_policy(
    iface_manager: &Arc<Mutex<InterfaceManager>>,
    address: AddressHash,
    flag: IfacFlag,
    wire_authenticated: bool,
) -> bool {
    let ifac_enabled = iface_manager.lock().await.shared_config(&address).is_some_and(|config| {
        config.network_name.as_deref().is_some_and(|value| !value.is_empty())
            || config.passphrase.as_deref().is_some_and(|value| !value.is_empty())
    });
    let ifac_flag_set = flag == IfacFlag::Authenticated || wire_authenticated;
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
