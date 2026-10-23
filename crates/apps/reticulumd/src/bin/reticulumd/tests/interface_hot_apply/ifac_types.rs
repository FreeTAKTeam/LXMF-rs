use super::*;
use crate::interface_hot_apply::interface_hot_apply_parts::record_settings::interface_record_shared_config;
use serde_json::Value;

fn record(settings: Option<Value>) -> InterfaceRecord {
    InterfaceRecord { settings, ..udp_record("typed-ifac", "127.0.0.1", 4242) }
}

fn rejects(settings: Value) {
    let (tx, mut commands) = tokio::sync::mpsc::channel(1);
    let bridge = test_bridge(tx);
    let error = bridge
        .apply_interfaces(vec![record(Some(settings))])
        .expect_err("malformed IFAC cannot be acknowledged");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(error.to_string(), InterfaceMutationFailure::InvalidIfacConfiguration.to_string());
    assert!(commands.try_recv().is_err(), "rejection must precede queue admission");
}

fn accepts(record: InterfaceRecord) -> InterfaceSharedConfig {
    let (tx, mut commands) = tokio::sync::mpsc::channel(1);
    let bridge = test_bridge(tx);
    let policy = interface_record_shared_config(&record);
    bridge.apply_interfaces(vec![record]).expect("valid optional IFAC input");
    assert!(commands.try_recv().is_ok());
    policy
}

#[test]
fn raw_ifac_rejects_every_malformed_alias_even_below_a_valid_preferred_value() {
    for key in
        ["network_name", "networkname", "ifac_netname", "pass_phrase", "passphrase", "ifac_netkey"]
    {
        for value in [
            json!(true),
            json!(7),
            json!(["do-not-log-ifac-value"]),
            json!({"do-not-log-ifac-value":true}),
        ] {
            let mut settings = json!({
                "network_name":"typed-net", "networkname":"fallback-net", "ifac_netname":"legacy-net",
                "pass_phrase":"typed-key", "passphrase":"fallback-key", "ifac_netkey":"legacy-key",
            });
            settings[key] = value;
            rejects(settings);
        }
    }
}

#[test]
fn raw_ifac_rejects_malformed_or_out_of_range_sizes_before_queueing() {
    for value in [
        json!(true),
        json!("128"),
        json!(128.0),
        json!(9.5),
        json!([]),
        json!({}),
        json!(u64::MAX),
        json!(i64::MAX),
        json!(520),
    ] {
        rejects(json!({"network_name":"typed-net", "ifac_size":value}));
    }
}

#[test]
fn raw_ifac_present_negative_or_default_size_still_requires_nonempty_credentials() {
    for bits in [i64::MIN, -1, 0, 7] {
        rejects(json!({"ifac_size":bits}));
        rejects(
            json!({"ifac_size":bits,"network_name":"","networkname":null,"ifac_netname":"","pass_phrase":"","passphrase":null,"ifac_netkey":""}),
        );
    }
}

#[test]
fn raw_ifac_signed_sizes_match_startup_presence_and_native_flooring() {
    for (bits, normalized, bytes) in [
        (i64::MIN, 0, 16),
        (-1, 0, 16),
        (0, 0, 16),
        (7, 7, 16),
        (8, 8, 1),
        (9, 9, 1),
        (519, 519, 64),
    ] {
        let policy = accepts(record(Some(json!({"network_name":"typed-net", "ifac_size":bits}))));
        assert_eq!(policy.ifac_size, Some(normalized));
        assert_eq!(
            policy.ifac_context().expect("valid IFAC").expect("authenticated policy").ifac_size(),
            bytes
        );
    }
}

#[test]
fn raw_ifac_preserves_absence_null_empty_fallback_and_byte_significant_whitespace() {
    for settings in [
        None,
        Some(Value::Null),
        Some(json!({})),
        Some(json!({"ifac_size":null,"network_name":null,"passphrase":null})),
    ] {
        assert!(accepts(record(settings)).ifac_context().expect("optional IFAC").is_none());
    }
    let policy = accepts(record(Some(json!({
        "network_name":null,"networkname":"","ifac_netname":" field net ",
        "pass_phrase":"","passphrase":null,"ifac_netkey":" shared key ","ifac_size":null,
    }))));
    assert_eq!(policy.network_name.as_deref(), Some(" field net "));
    assert_eq!(policy.passphrase.as_deref(), Some(" shared key "));
    assert_eq!(
        policy.ifac_context().expect("valid aliases").expect("authenticated aliases").ifac_size(),
        16
    );
    let whitespace = accepts(record(Some(json!({"network_name":"   ","passphrase":" \t "}))));
    assert_eq!(whitespace.network_name.as_deref(), Some("   "));
    assert_eq!(whitespace.passphrase.as_deref(), Some(" \t "));
}

#[test]
fn raw_ifac_preserves_opaque_settings_metadata_compatibility() {
    for value in [json!(true), json!(["do-not-log-ifac-value"]), json!("do-not-log-ifac-value")] {
        let input = record(Some(value.clone()));
        assert!(interface_record_shared_config(&input)
            .ifac_context()
            .expect("opaque settings")
            .is_none());
        let (tx, mut commands) = tokio::sync::mpsc::channel(1);
        let applied =
            test_bridge(tx).apply_interfaces(vec![input]).expect("opaque metadata compatibility");
        assert!(commands.try_recv().is_ok());
        assert_eq!(applied[0].settings.as_ref().expect("settings")["configured_settings"], value);
    }
    let opaque =
        accepts(record(Some(json!({"configured_settings":{"network_name":false,"ifac_size":-1}}))));
    assert!(opaque.ifac_context().expect("opaque nested metadata").is_none());
}
