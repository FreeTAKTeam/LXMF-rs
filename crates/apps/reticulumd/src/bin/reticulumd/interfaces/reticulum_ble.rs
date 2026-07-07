#[path = "reticulum_ble_parts.rs"]
mod reticulum_ble_parts;

pub(crate) use reticulum_ble_parts::{
    ReticulumBleRuntimeStatusHandle, IDENTITY_CHAR_UUID, RX_CHAR_UUID, SERVICE_UUID, TX_CHAR_UUID,
};

pub(crate) use reticulum_ble_parts::spawn;
