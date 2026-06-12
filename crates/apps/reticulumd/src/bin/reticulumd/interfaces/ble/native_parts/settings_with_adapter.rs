#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_os = "macos"))]
    fn settings_with_adapter(adapter: Option<&str>) -> BleRuntimeSettings {
        BleRuntimeSettings {
            adapter: adapter.map(ToOwned::to_owned),
            peripheral_id: "AA:BB:CC:DD:EE:FF".to_string(),
            service_uuid: "12345678-1234-1234-1234-1234567890ab".to_string(),
            write_char_uuid: "2A37".to_string(),
            notify_char_uuid: "2A38".to_string(),
            mtu: 247,
            scan_timeout: Duration::from_millis(100),
            connect_timeout: Duration::from_millis(100),
            reconnect_backoff: Duration::from_millis(50),
            max_reconnect_backoff: Duration::from_millis(100),
        }
    }

    #[test]
    fn identifiers_match_normalizes_case_and_separators() {
        assert!(identifiers_match("AA:BB:CC:DD", "aabbccdd"));
        assert!(identifiers_match("AB-CD-EF", "abcdef"));
        assert!(!identifiers_match("AB-CD-EF", "abcdee"));
    }

    #[test]
    fn parse_gatt_uuid_accepts_short_and_full_forms() {
        assert_eq!(
            parse_gatt_uuid("write_char_uuid", "2A37").expect("16-bit UUID").to_string(),
            "00002a37-0000-1000-8000-00805f9b34fb"
        );
        assert_eq!(
            parse_gatt_uuid("write_char_uuid", "12345678").expect("32-bit UUID").to_string(),
            "12345678-0000-1000-8000-00805f9b34fb"
        );
        assert_eq!(
            parse_gatt_uuid("write_char_uuid", "12345678-1234-1234-1234-1234567890ab")
                .expect("128-bit UUID")
                .to_string(),
            "12345678-1234-1234-1234-1234567890ab"
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[tokio::test(flavor = "current_thread")]
    async fn native_scan_with_unknown_adapter_exercises_adapter_selection_path() {
        let mut backend = NativeBleBackend::new("native-test");
        let settings = settings_with_adapter(Some("__adapter_that_should_not_exist__"));
        let err = backend.scan(&settings).await.expect_err("unknown adapter should fail scan");

        assert!(
            err.message.contains("configured adapter")
                || err.message.contains("no BLE adapters available")
                || err.message.contains("create BLE manager")
                || err.message.contains("read adapter info")
                || err.message.contains("enumerate BLE adapters"),
            "unexpected scan failure reason: {}",
            err.message
        );
    }
}
