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
async fn native_rnode_windows_paired_device_ids() -> Result<Vec<String>, String> {
    use windows::Devices::Bluetooth::BluetoothLEDevice;
    use windows::Devices::Enumeration::DeviceInformation;

    let selector = BluetoothLEDevice::GetDeviceSelectorFromPairingState(true)
        .map_err(|error| format!("create Windows paired BLE selector: {error}"))?;
    let devices = DeviceInformation::FindAllAsyncAqsFilter(&selector)
        .map_err(|error| format!("enumerate Windows paired BLE devices: {error}"))?
        .await
        .map_err(|error| format!("read Windows paired BLE devices: {error}"))?;
    Ok(devices
        .into_iter()
        .map(|device| {
            device
                .Id()
                .map(|device_id| device_id.to_string())
                .map_err(|error| format!("read Windows paired BLE device ID: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?)
}

#[cfg(all(feature = "rnode-ble", target_os = "windows"))]
async fn native_rnode_windows_paired_addresses() -> Result<Option<Vec<String>>, String> {
    let device_ids = native_rnode_windows_paired_device_ids().await?;
    let addresses = windows_paired_addresses_from_device_ids(&device_ids);

    log::debug!(
        "RNode BLE Windows paired-device resolver found {} address(es)",
        addresses.len()
    );
    Ok(Some(addresses))
}

#[cfg(all(feature = "rnode-ble", target_os = "windows"))]
fn windows_paired_addresses_from_device_ids(device_ids: &[String]) -> Vec<String> {
    device_ids
        .iter()
        .filter_map(|device_id| windows_paired_address_from_device_id(device_id))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(any(not(feature = "rnode-ble"), not(target_os = "windows")))]
#[allow(dead_code)]
async fn native_rnode_windows_paired_addresses() -> Result<Option<Vec<String>>, String> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    #[cfg(all(feature = "rnode-ble", target_os = "windows"))]
    use std::collections::BTreeSet;

    use super::windows_paired_address_from_device_id;

    #[cfg(all(feature = "rnode-ble", target_os = "windows"))]
    use super::windows_paired_addresses_from_device_ids;

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

    #[cfg(all(feature = "rnode-ble", target_os = "windows"))]
    #[test]
    fn extracts_sorted_unique_addresses_from_reference_device_ids() {
        let device_ids = vec![
            "BluetoothLE#BLE-AABBCCDDEEFF-AA:BB:CC:DD:EE:FF".to_string(),
            "BluetoothLE#BLE-001122334455-00:11:22:33:44:55".to_string(),
            "BluetoothLE#BLE-AABBCCDDEEFF-AA:BB:CC:DD:EE:FF".to_string(),
            "BluetoothLE#RNode-Test".to_string(),
        ];

        assert_eq!(
            windows_paired_addresses_from_device_ids(&device_ids),
            vec!["001122334455".to_string(), "aabbccddeeff".to_string()]
        );
    }

    #[cfg(all(feature = "rnode-ble", target_os = "windows"))]
    #[tokio::test]
    async fn native_windows_paired_device_query_matches_reference_id_suffixes() {
        let device_ids = super::native_rnode_windows_paired_device_ids()
            .await
            .expect("query Windows paired BLE devices");
        let mut reference_addresses = BTreeSet::new();
        for device_id in &device_ids {
            let suffix = device_id
                .rsplit('-')
                .next()
                .expect("Windows device ID has a suffix")
                .trim()
                .to_ascii_lowercase();
            let normalized = suffix
                .chars()
                .filter(|character| !matches!(character, ':' | '-'))
                .collect::<String>();
            assert_eq!(normalized.len(), 12, "paired device suffix is a Bluetooth address");
            assert!(normalized.chars().all(|character| character.is_ascii_hexdigit()));
            reference_addresses.insert(normalized);
        }
        let rust_addresses = windows_paired_addresses_from_device_ids(&device_ids)
            .into_iter()
            .collect::<BTreeSet<_>>();

        assert!(
            rust_addresses.iter().all(|address| {
                address.len() == 12
                    && address.chars().all(|character| character.is_ascii_hexdigit())
                    && address.chars().all(|character| !character.is_ascii_uppercase())
            }),
            "Windows paired BLE addresses must be canonical 12-digit lowercase hex"
        );
        assert_eq!(rust_addresses, reference_addresses);
    }
}
