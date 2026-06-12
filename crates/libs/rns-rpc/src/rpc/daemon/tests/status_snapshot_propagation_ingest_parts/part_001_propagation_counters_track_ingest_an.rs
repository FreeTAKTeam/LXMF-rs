#[test]
fn propagation_counters_track_ingest_and_unpeered_attempts() {
    use sha2::{Digest, Sha256};

    let daemon = RpcDaemon::test_instance();
    let payload = [0x00_u8, 0x11_u8];
    let transient_id = hex::encode(Sha256::digest(payload));
    daemon
        .handle_rpc(rpc_request(
            60,
            "propagation_ingest",
            json!({
                "transient_id": transient_id,
                "payload_hex": hex::encode(payload),
            }),
        ))
        .expect("propagation ingest");
    daemon.record_unpeered_propagation_attempt(42);

    let result = daemon
        .handle_rpc(RpcRequest { id: 61, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    let propagation = result["propagation"].clone();
    assert_eq!(propagation["client_propagation_messages_received"].as_u64(), Some(1));
    assert_eq!(propagation["client_propagation_messages_served"].as_u64(), Some(0));
    assert_eq!(propagation["unpeered_propagation_incoming"].as_u64(), Some(1));
    assert_eq!(propagation["unpeered_propagation_rx_bytes"].as_u64(), Some(42));

    let fetched = daemon
        .handle_rpc(rpc_request(
            62,
            "propagation_fetch",
            json!({
                "transient_id": hex::encode(Sha256::digest(payload)),
            }),
        ))
        .expect("propagation fetch")
        .result
        .expect("propagation fetch result");
    assert_eq!(fetched["transferred_bytes"].as_u64(), Some(payload.len() as u64));
    assert_eq!(fetched["payload_bytes"].as_u64(), Some(payload.len() as u64));
    let result = daemon
        .handle_rpc(RpcRequest { id: 63, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(result["propagation"]["client_propagation_messages_served"].as_u64(), Some(1));
}

#[test]
fn propagation_ingest_rejects_missing_stamp_when_cost_required() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            64,
            "propagation_enable",
            json!({
                "enabled": true,
                "target_cost": 1,
            }),
        ))
        .expect("enable propagation");

    let err = daemon
        .handle_rpc(rpc_request(
            65,
            "propagation_ingest",
            json!({
                "payload_hex": hex::encode(b"unstamped-propagation-bytes"),
            }),
        ))
        .expect_err("missing propagation stamp must be rejected");
    assert!(err.to_string().contains("invalid propagation stamp"));

    let result = daemon
        .handle_rpc(RpcRequest { id: 66, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(result["propagation"]["client_propagation_messages_received"].as_u64(), Some(0));
}

#[test]
fn propagation_ingest_rejects_too_short_stamped_payload_when_cost_required() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            67,
            "propagation_enable",
            json!({
                "enabled": true,
                "target_cost": 1,
            }),
        ))
        .expect("enable propagation");

    let too_short_transient = vec![0xAA_u8; 112 + 32];
    let err = daemon
        .handle_rpc(rpc_request(
            68,
            "propagation_ingest",
            json!({
                "payload_hex": hex::encode(too_short_transient),
            }),
        ))
        .expect_err("too-short stamped propagation payload must be rejected");
    assert!(err.to_string().contains("invalid propagation stamp"));
}

#[test]
fn propagation_ingest_accepts_valid_stamp_when_cost_required() {
    use sha2::Digest;

    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            68,
            "propagation_enable",
            json!({
                "enabled": true,
                "target_cost": 1,
            }),
        ))
        .expect("enable propagation");

    let lxm_data = vec![0x42_u8; 113];
    let transient_data = stamped_propagation_payload(&lxm_data, 1);
    let response = daemon
        .handle_rpc(rpc_request(
            69,
            "propagation_ingest",
            json!({
                "payload_hex": hex::encode(&transient_data),
            }),
        ))
        .expect("valid stamped propagation ingest");
    let result = response.result.expect("ingest result");
    let transient_id = result["transient_id"].as_str().expect("transient id").to_string();
    assert_eq!(transient_id, hex::encode(sha2::Sha256::digest(&lxm_data)));
    assert_eq!(result["ingested_count"].as_u64(), Some(1));

    let fetched = daemon
        .handle_rpc(rpc_request(
            69,
            "propagation_fetch",
            json!({
                "transient_id": transient_id,
            }),
        ))
        .expect("fetch ingested propagation payload")
        .result
        .expect("fetch result");
    let expected_payload_hex = hex::encode(&lxm_data);
    assert_eq!(fetched["payload_hex"].as_str(), Some(expected_payload_hex.as_str()));
}

#[test]
fn propagation_ingest_derives_transient_id_from_payload_bytes() {
    use sha2::{Digest, Sha256};

    let daemon = RpcDaemon::test_instance();
    let payload = b"unstamped-propagation-wire-payload";
    let payload_hex = hex::encode(payload);
    let expected_transient_id = hex::encode(Sha256::digest(payload));

    let response = daemon
        .handle_rpc(rpc_request(
            70,
            "propagation_ingest",
            json!({
                "payload_hex": payload_hex,
            }),
        ))
        .expect("propagation ingest");
    let result = response.result.expect("ingest result");
    assert_eq!(result["transient_id"].as_str(), Some(expected_transient_id.as_str()));

    let fetched = daemon
        .handle_rpc(rpc_request(
            71,
            "propagation_fetch",
            json!({
                "transient_id": expected_transient_id,
            }),
        ))
        .expect("fetch ingested propagation payload")
        .result
        .expect("fetch result");
    assert_eq!(fetched["payload_hex"].as_str(), Some(payload_hex.as_str()));
}

#[test]
fn propagation_rpc_ingest_persists_payloads_to_store_for_fetch_after_cache_clear() {
    use sha2::{Digest, Sha256};

    let daemon = RpcDaemon::test_instance();
    let payload = b"stored-rpc-propagation-wire-payload";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));

    daemon
        .handle_rpc(rpc_request(
            72,
            "propagation_ingest",
            json!({
                "payload_hex": payload_hex,
            }),
        ))
        .expect("propagation ingest");

    let stored = daemon
        .store
        .get_propagation_entry(transient_id.as_str())
        .expect("load propagation entry")
        .expect("propagation entry persisted");
    assert_eq!(stored.payload_hex, payload_hex);

    daemon.propagation_payloads.lock().expect("propagation payload mutex poisoned").clear();
    let fetched = daemon
        .handle_rpc(rpc_request(
            73,
            "propagation_fetch",
            json!({
                "transient_id": transient_id,
            }),
        ))
        .expect("fetch stored propagation payload")
        .result
        .expect("fetch result");
    assert_eq!(fetched["payload_hex"].as_str(), Some(payload_hex.as_str()));
}

#[test]
fn propagation_ingest_accepts_stamped_payload_with_canonical_transient_id_when_cost_is_zero() {
    use sha2::{Digest, Sha256};

    let daemon = RpcDaemon::test_instance();
    let lxm_data = vec![0x55_u8; 113];
    let transient_data = stamped_propagation_payload(&lxm_data, 1);
    let canonical_transient_id = hex::encode(Sha256::digest(&lxm_data));

    let response = daemon
        .handle_rpc(rpc_request(
            72,
            "propagation_ingest",
            json!({
                "transient_id": canonical_transient_id,
                "payload_hex": hex::encode(&transient_data),
            }),
        ))
        .expect("stamped propagation ingest at zero target cost");
    let result = response.result.expect("ingest result");
    assert_eq!(result["ingested_count"].as_u64(), Some(1));
    assert_eq!(result["transient_id"].as_str(), Some(canonical_transient_id.as_str()));

    let fetched = daemon
        .handle_rpc(rpc_request(
            73,
            "propagation_fetch",
            json!({
                "transient_id": canonical_transient_id,
            }),
        ))
        .expect("fetch ingested propagation payload")
        .result
        .expect("fetch result");
    let expected_payload_hex = hex::encode(&lxm_data);
    assert_eq!(fetched["payload_hex"].as_str(), Some(expected_payload_hex.as_str()));
}

#[test]
fn propagation_ingest_normalizes_uppercase_transient_id_keys() {
    use sha2::{Digest, Sha256};

    let daemon = RpcDaemon::test_instance();
    let payload = b"case-normalized-propagation-wire-payload";
    let canonical_transient_id = hex::encode(Sha256::digest(payload));
    let uppercase_transient_id = canonical_transient_id.to_ascii_uppercase();

    let response = daemon
        .handle_rpc(rpc_request(
            73,
            "propagation_ingest",
            json!({
                "transient_id": uppercase_transient_id,
                "payload_hex": hex::encode(payload),
            }),
        ))
        .expect("propagation ingest with uppercase transient id");
    let result = response.result.expect("ingest result");
    assert_eq!(result["transient_id"].as_str(), Some(canonical_transient_id.as_str()));

    let fetched = daemon
        .handle_rpc(rpc_request(
            74,
            "propagation_fetch",
            json!({
                "transient_id": canonical_transient_id,
            }),
        ))
        .expect("fetch by lowercase canonical transient id")
        .result
        .expect("fetch result");
    assert_eq!(fetched["payload_hex"].as_str(), Some(hex::encode(payload).as_str()));
}

#[test]
fn propagation_alias_ingest_stores_python_served_unstamped_payload() {
    use sha2::{Digest, Sha256};

    let daemon = RpcDaemon::test_instance();
    let lxm_data = vec![0x65_u8; 113];
    let transient_data = stamped_propagation_payload(&lxm_data, 1);
    let canonical_transient_id = hex::encode(Sha256::digest(&lxm_data));

    daemon
        .ingest_propagation_payload_bytes_with_aliases(
            transient_data.as_slice(),
            canonical_transient_id.as_str(),
            &[],
        )
        .expect("alias ingest stamped propagation payload");

    let fetched = daemon
        .handle_rpc(rpc_request(
            73,
            "propagation_fetch",
            json!({
                "transient_id": canonical_transient_id,
            }),
        ))
        .expect("fetch ingested propagation payload")
        .result
        .expect("fetch result");
    let expected_payload_hex = hex::encode(&lxm_data);
    assert_eq!(fetched["payload_hex"].as_str(), Some(expected_payload_hex.as_str()));
}

#[test]
fn duplicate_alias_ingest_does_not_double_count_received() {
    use sha2::{Digest, Sha256};

    let daemon = RpcDaemon::test_instance();
    let lxm_data = b"duplicate-alias-ingest-payload".to_vec();
    let transient_data = stamped_propagation_payload(&lxm_data, 1);
    let canonical_transient_id = hex::encode(Sha256::digest(&lxm_data));

    for _ in 0..2 {
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                transient_data.as_slice(),
                canonical_transient_id.as_str(),
                &[],
            )
            .expect("alias ingest stamped propagation payload");
    }

    let status = daemon
        .handle_rpc(RpcRequest { id: 81, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(status["propagation"]["client_propagation_messages_received"].as_u64(), Some(1));
    assert_eq!(status["propagation"]["total_ingested"].as_u64(), Some(1));
    assert_eq!(status["propagation"]["last_ingest_count"].as_u64(), Some(0));
}

#[test]
fn duplicate_byte_ingest_does_not_double_count_received() {
    use sha2::{Digest, Sha256};

    let daemon = RpcDaemon::test_instance();
    let payload = b"duplicate-byte-ingest-payload";
    let transient_id = hex::encode(Sha256::digest(payload));

    for _ in 0..2 {
        daemon
            .ingest_propagation_payload_bytes(payload, Some(transient_id.as_str()))
            .expect("byte ingest propagation payload");
    }

    let status = daemon
        .handle_rpc(RpcRequest { id: 82, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(status["propagation"]["client_propagation_messages_received"].as_u64(), Some(1));
    assert_eq!(status["propagation"]["total_ingested"].as_u64(), Some(1));
    assert_eq!(status["propagation"]["last_ingest_count"].as_u64(), Some(0));
}

#[test]
fn propagation_fetch_transfer_limit_accounts_for_stripped_stamp_bytes() {
    use sha2::{Digest, Sha256};

    let daemon = RpcDaemon::test_instance();
    let destination = [0x77_u8; 16];
    let mut lxm_data = destination.to_vec();
    lxm_data.extend_from_slice(&[0x42_u8; 128]);
    let transient_data = stamped_propagation_payload(&lxm_data, 1);
    let transient_id = Sha256::digest(&lxm_data).to_vec();
    let transient_hex = hex::encode(&transient_id);
    daemon
        .ingest_propagation_payload_bytes_with_aliases(
            transient_data.as_slice(),
            transient_hex.as_str(),
            &[],
        )
        .expect("ingest stamped propagation payload");

    let python_budgeted_size = 24 + lxm_data.len() + 32 + 16;
    let too_small = daemon.fetch_propagation_payloads_for_destination(
        &destination,
        std::slice::from_ref(&transient_id),
        Some(python_budgeted_size - 1),
    );
    assert!(too_small.is_empty(), "limit below Python stamped size must skip payload");

    let exact = daemon.fetch_propagation_payloads_for_destination(
        &destination,
        &[transient_id],
        Some(python_budgeted_size),
    );
    assert_eq!(exact, vec![lxm_data]);
}
