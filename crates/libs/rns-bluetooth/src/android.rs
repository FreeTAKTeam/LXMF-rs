//! Version gate shared with the Android companion. Native handles remain Android-owned.
use crate::{BluetoothError, ErrorKind, Result};
pub const BRIDGE_VERSION: u32 = 1;
pub fn require_bridge_version(actual: u32) -> Result<()> {
    if actual != BRIDGE_VERSION {
        return Err(BluetoothError::new(ErrorKind::Unsupported, "Android bridge version"));
    }
    Ok(())
}
