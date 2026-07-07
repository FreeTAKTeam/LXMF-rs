#[path = "reticulum_ble_parts/fragment.rs"]
#[cfg_attr(not(test), allow(dead_code))]
mod fragment;
#[path = "reticulum_ble_parts/runtime_core.rs"]
#[cfg_attr(not(test), allow(dead_code))]
mod runtime_core;
#[path = "reticulum_ble_parts/runtime_iface.rs"]
mod runtime_iface;

pub(crate) use fragment::fragment_packet;
#[cfg(test)]
pub(crate) use fragment::FragmentError;
#[cfg(test)]
pub(crate) use runtime_core::{PeerRegistration, ReticulumBleRole, ReticulumBleRuntimeCore};
pub(crate) use runtime_iface::{
    spawn, ReticulumBleRuntimeStatusHandle, IDENTITY_CHAR_UUID, RX_CHAR_UUID, SERVICE_UUID,
    TX_CHAR_UUID,
};

#[cfg(test)]
#[path = "reticulum_ble_parts/tests.rs"]
mod tests;
