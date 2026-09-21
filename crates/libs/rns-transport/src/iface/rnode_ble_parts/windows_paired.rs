#[cfg(all(feature = "rnode-ble", target_os = "windows"))]
use std::collections::BTreeSet;

/// Extract the Bluetooth address suffix used by Windows `DeviceInformation.Id`.
///
/// Windows exposes paired BLE devices through IDs such as
/// `BluetoothLE#BluetoothLE...-AA:BB:CC:DD:EE:FF`.  The reference RNode
/// implementation uses the final `-`-delimited component as the address; keep
/// the parser strict so an unrelated device ID cannot become a BLE target.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn windows_paired_address_from_device_id(device_id: &str) -> Option<String> {
    let address = device_id.rsplit('-').next()?.trim();
    let normalized = address
        .chars()
        .filter(|character| !matches!(character, ':' | '-'))
        .collect::<String>();
    (normalized.len() == 12 && normalized.chars().all(|character| character.is_ascii_hexdigit()))
        .then(|| normalized.to_ascii_lowercase())
}

#[cfg(all(feature = "rnode-ble", target_os = "windows"))]
async fn native_rnode_windows_paired_addresses() -> Result<Option<Vec<String>>, String> {
    use windows::Devices::Bluetooth::BluetoothLEDevice;
    use windows::Devices::Enumeration::DeviceInformation;

    let selector = BluetoothLEDevice::GetDeviceSelectorFromPairingState(true)
        .map_err(|error| format!("create Windows paired BLE selector: {error}"))?;
    let devices = DeviceInformation::FindAllAsyncAqsFilter(&selector)
        .map_err(|error| format!("enumerate Windows paired BLE devices: {error}"))?
        .await
        .map_err(|error| format!("read Windows paired BLE devices: {error}"))?;
    let addresses = devices
        .into_iter()
        .filter_map(|device| device.Id().ok())
        .filter_map(|device_id| windows_paired_address_from_device_id(&device_id.to_string()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    log::debug!(
        "RNode BLE Windows paired-device resolver found {} address(es)",
        addresses.len()
    );
    Ok(Some(addresses))
}

#[cfg(any(not(feature = "rnode-ble"), not(target_os = "windows")))]
async fn native_rnode_windows_paired_addresses() -> Result<Option<Vec<String>>, String> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::windows_paired_address_from_device_id;

    #[test]
    fn parses_windows_paired_device_address_suffix() {
        assert_eq!(
            windows_paired_address_from_device_id(
                "BluetoothLE#BluetoothLE00:11:22:33:44:55-00:11:22:33:44:55"
            ),
            Some("001122334455".to_string())
        );
    }

    #[test]
    fn rejects_non_address_device_id_suffixes() {
        assert_eq!(windows_paired_address_from_device_id("BluetoothLE#RNode-Test"), None);
        assert_eq!(
            windows_paired_address_from_device_id("BluetoothLE#BluetoothLE-00:11:22:33:44"),
            None
        );
    }
}
