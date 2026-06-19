#[test]
fn propagation_destination_fetch_deduplicates_repeated_wanted_ids_like_python() {
    use sha2::{Digest, Sha256};

    let daemon = RpcDaemon::test_instance();
    let destination = [0x7a_u8; 16];
    let mut payload = destination.to_vec();
    payload.extend_from_slice(b" repeated wanted payload");
    let transient_id = Sha256::digest(&payload).to_vec();

    daemon
        .ingest_propagation_payload_bytes_with_aliases(
            payload.as_slice(),
            hex::encode(&transient_id).as_str(),
            &[],
        )
        .expect("ingest propagation payload");

    let fetched = daemon.fetch_propagation_payloads_for_destination(
        &destination,
        &[transient_id.clone(), transient_id],
        None,
    );
    assert_eq!(fetched, vec![payload]);

    let status = daemon
        .handle_rpc(RpcRequest { id: 82, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(status["propagation"]["client_propagation_messages_served"].as_u64(), Some(1));
}

#[test]
fn propagation_ingest_accepts_reference_cost_16_stamp() {
    use sha2::{Digest, Sha256};

    let daemon = RpcDaemon::test_instance();
    let destination = [0xa2_u8; 16];
    let mut lxm_data = destination.to_vec();
    lxm_data.extend(std::iter::repeat_n(0x42_u8, 180));
    let transient_id = hex::encode(Sha256::digest(&lxm_data));
    let payload = stamped_propagation_payload(&lxm_data, 16);

    let result = daemon
        .ingest_propagation_payload_bytes_at_cost(&payload, Some(&transient_id), 16)
        .expect("cost-16 reference propagation stamp should ingest");

    assert_eq!(result, transient_id);
    assert!(daemon.has_propagation_payload(&result));
}

#[test]
fn propagation_ingest_rejects_ignored_destination_before_queueing() {
    use sha2::{Digest, Sha256};

    let daemon = RpcDaemon::test_instance();
    let destination = [0x9a_u8; 16];
    let destination_hex = hex::encode(destination);
    let mut payload = destination.to_vec();
    payload.extend_from_slice(b" ignored propagation payload");
    let transient_id = hex::encode(Sha256::digest(&payload));

    daemon
        .handle_rpc(rpc_request(
            83,
            "set_delivery_policy",
            json!({
                "ignored_destinations": [destination_hex],
            }),
        ))
        .expect("set ignored destination policy");

    let err = daemon
        .handle_rpc(rpc_request(
            84,
            "propagation_ingest",
            json!({
                "payload_hex": hex::encode(&payload),
            }),
        ))
        .expect_err("ignored destination propagation payload must be rejected");
    assert!(err.to_string().contains("ignored propagation destination"));

    assert!(
        daemon
            .store
            .get_propagation_entry(transient_id.as_str())
            .expect("load propagation entry")
            .is_none(),
        "ignored destination payload must not be stored"
    );
    assert!(
        daemon
            .fetch_propagation_payloads_for_destination(
                &destination,
                &[Sha256::digest(&payload).to_vec()],
                None,
            )
            .is_empty(),
        "ignored destination payload must not be fetchable"
    );

    let status = daemon
        .handle_rpc(RpcRequest { id: 85, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(status["propagation"]["client_propagation_messages_received"].as_u64(), Some(0));
    assert_eq!(status["propagation"]["total_ingested"].as_u64(), Some(0));
}

#[test]
fn propagation_destination_fetch_combines_store_and_memory_payloads() {
    use sha2::{Digest, Sha256};

    let daemon = RpcDaemon::test_instance();
    let destination = [0x78_u8; 16];
    let mut stored_payload = destination.to_vec();
    stored_payload.extend_from_slice(b" stored propagation lxm");
    let mut cached_payload = destination.to_vec();
    cached_payload.extend_from_slice(b" cached propagation lxm");
    let stored_transient_id = Sha256::digest(&stored_payload).to_vec();
    let cached_transient_id = Sha256::digest(&cached_payload).to_vec();
    let cached_transient_hex = hex::encode(&cached_transient_id);

    daemon
        .ingest_propagation_payload_bytes_with_aliases(
            stored_payload.as_slice(),
            hex::encode(&stored_transient_id).as_str(),
            &[],
        )
        .expect("ingest stored payload");
    daemon
        .ingest_propagation_payload_bytes_with_aliases(
            cached_payload.as_slice(),
            cached_transient_hex.as_str(),
            &[],
        )
        .expect("ingest cached payload");
    daemon
        .store
        .purge_propagation_entries_for_destination(
            hex::encode(destination).as_str(),
            std::slice::from_ref(&cached_transient_hex),
        )
        .expect("remove cached payload from persistent store");

    let fetched = daemon.fetch_propagation_payloads_for_destination(
        &destination,
        &[stored_transient_id, cached_transient_id],
        None,
    );

    assert_eq!(fetched, vec![stored_payload, cached_payload]);
}

#[test]
fn propagation_ingest_rejects_mismatched_transient_id() {
    let daemon = RpcDaemon::test_instance();
    let err = daemon
        .handle_rpc(rpc_request(
            74,
            "propagation_ingest",
            json!({
                "transient_id": "deadbeef",
                "payload_hex": hex::encode(b"propagation-wire-payload"),
            }),
        ))
        .expect_err("mismatched transient_id must be rejected");
    assert!(err.to_string().contains("transient_id does not match propagation payload"));
}

#[test]
fn propagation_ingest_without_payload_does_not_increment_counts_or_store_payload() {
    let daemon = RpcDaemon::test_instance();

    let response = daemon
        .handle_rpc(rpc_request(
            75,
            "propagation_ingest",
            json!({
                "transient_id": "abcd",
            }),
        ))
        .expect("ingest without payload");
    let result = response.result.expect("ingest result");
    assert_eq!(result["ingested_count"].as_u64(), Some(0));
    assert_eq!(result["transient_id"].as_str(), Some("abcd"));

    let snapshot = daemon
        .handle_rpc(RpcRequest { id: 76, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(snapshot["propagation"]["last_ingest_count"].as_u64(), Some(0));
    assert_eq!(snapshot["propagation"]["total_ingested"].as_u64(), Some(0));
    assert_eq!(
        snapshot["propagation"]["client_propagation_messages_received"].as_u64(),
        Some(0)
    );

    let err = daemon
        .handle_rpc(rpc_request(
            77,
            "propagation_fetch",
            json!({
                "transient_id": "abcd",
            }),
        ))
        .expect_err("payload should not have been stored");
    assert!(err.to_string().contains("transient_id not found"));
}

#[test]
fn duplicate_propagation_ingest_does_not_double_count_received() {
    use sha2::{Digest, Sha256};

    let daemon = RpcDaemon::test_instance();
    let payload = b"duplicate-local-propagation-ingest";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));

    let mut second = JsonValue::Null;
    for request_id in [78, 79] {
        second = daemon
            .handle_rpc(rpc_request(
                request_id,
                "propagation_ingest",
                json!({
                    "transient_id": transient_id,
                    "payload_hex": payload_hex,
                }),
            ))
            .expect("propagation ingest")
            .result
            .expect("propagation ingest result");
    }
    assert_eq!(second["ingested_count"].as_u64(), Some(0));
    assert_eq!(second["duplicate_count"].as_u64(), Some(1));
    assert_eq!(second["payload_bytes"].as_u64(), Some(payload.len() as u64));
    assert_eq!(second["transferred_bytes"].as_u64(), Some(payload.len() as u64));

    let status = daemon
        .handle_rpc(RpcRequest { id: 80, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(status["propagation"]["client_propagation_messages_received"].as_u64(), Some(1));
    assert_eq!(status["propagation"]["total_ingested"].as_u64(), Some(1));
    assert_eq!(status["propagation"]["last_ingest_count"].as_u64(), Some(0));
}

#[test]
fn purged_propagation_ingest_does_not_recount_processed_transient() {
    use sha2::{Digest, Sha256};

    let daemon = RpcDaemon::test_instance();
    let destination = [0x9b_u8; 16];
    let mut payload = destination.to_vec();
    payload.extend_from_slice(b" purged local propagation idempotence");
    let payload_hex = hex::encode(&payload);
    let transient_id = Sha256::digest(&payload).to_vec();
    let transient_hex = hex::encode(&transient_id);

    let first = daemon
        .handle_rpc(rpc_request(
            83,
            "propagation_ingest",
            json!({
                "transient_id": transient_hex,
                "payload_hex": payload_hex,
            }),
        ))
        .expect("first propagation ingest")
        .result
        .expect("first propagation ingest result");
    assert_eq!(first["ingested_count"].as_u64(), Some(1));

    let purged = daemon
        .purge_propagation_payloads_for_destination(&destination, std::slice::from_ref(&transient_id));
    assert!(purged > 0);

    let second = daemon
        .handle_rpc(rpc_request(
            84,
            "propagation_ingest",
            json!({
                "transient_id": transient_hex,
                "payload_hex": payload_hex,
            }),
        ))
        .expect("second propagation ingest")
        .result
        .expect("second propagation ingest result");
    assert_eq!(second["ingested_count"].as_u64(), Some(0));
    assert_eq!(second["duplicate_count"].as_u64(), Some(1));

    let status = daemon
        .handle_rpc(RpcRequest { id: 85, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(status["propagation"]["client_propagation_messages_received"].as_u64(), Some(1));
    assert_eq!(status["propagation"]["total_ingested"].as_u64(), Some(1));
    assert_eq!(status["propagation"]["last_ingest_count"].as_u64(), Some(0));
}

#[test]
fn propagation_ingest_prunes_oldest_payload_when_storage_limit_is_exceeded() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            86,
            "propagation_enable",
            json!({
                "enabled": true,
                "message_storage_limit_mb": 1,
                "static_peers": ["peer-storage-prune-snapshot"],
            }),
        ))
        .expect("enable propagation storage limit");

    let destination = [0x5a_u8; 16];
    let mut first_payload = destination.to_vec();
    first_payload.extend(std::iter::repeat_n(0x11_u8, 600_000));
    let mut second_payload = destination.to_vec();
    second_payload.extend(std::iter::repeat_n(0x22_u8, 600_000));

    let first = daemon
        .handle_rpc(rpc_request(
            87,
            "propagation_ingest",
            json!({
                "payload_hex": hex::encode(&first_payload),
            }),
        ))
        .expect("first propagation ingest")
        .result
        .expect("first ingest result");
    let first_transient = first["transient_id"].as_str().expect("first transient id").to_string();
    assert!(daemon.has_propagation_payload(first_transient.as_str()));
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation_ids("peer-storage-prune-snapshot")
            .expect("first live unhandled ids"),
        vec![first_transient.clone()]
    );
    {
        let peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get("peer-storage-prune-snapshot").expect("stored peer");
        let serialized = serde_json::to_value(record).expect("serialize peer record");
        assert_eq!(
            serialized["unhandled_ids"].as_array().expect("serialized unhandled ids"),
            &[json!(first_transient.as_str())]
        );
    }

    let second = daemon
        .handle_rpc(rpc_request(
            88,
            "propagation_ingest",
            json!({
                "payload_hex": hex::encode(&second_payload),
            }),
        ))
        .expect("second propagation ingest")
        .result
        .expect("second ingest result");
    let second_transient = second["transient_id"].as_str().expect("second transient id").to_string();

    assert!(
        !daemon.has_propagation_payload(first_transient.as_str()),
        "oldest propagation payload should be pruned when storage limit is exceeded"
    );
    assert!(daemon.has_propagation_payload(second_transient.as_str()));
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation_ids("peer-storage-prune-snapshot")
            .expect("pruned live unhandled ids"),
        vec![second_transient.clone()]
    );
    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get("peer-storage-prune-snapshot").expect("stored peer");
    let serialized = serde_json::to_value(record).expect("serialize peer record");
    assert_eq!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids"),
        &[json!(second_transient.as_str())]
    );
    let stats = daemon.store.propagation_entry_stats().expect("propagation stats");
    assert!(stats.bytes <= 1_000_000, "stats after prune: {stats:?}");
}

fn stamped_propagation_payload(lxm_data: &[u8], target_cost: u32) -> Vec<u8> {
    use hkdf::Hkdf;
    use sha2::{Digest, Sha256};

    const PROPAGATION_STAMP_SIZE: usize = 32;
    const PROPAGATION_STAMP_ROUNDS: usize = 1000;

    let transient_id = Sha256::digest(lxm_data);
    let mut workblock = Vec::with_capacity(PROPAGATION_STAMP_ROUNDS * 256);
    for round in 0..PROPAGATION_STAMP_ROUNDS {
        let mut salt_data = Vec::with_capacity(transient_id.len() + 8);
        salt_data.extend_from_slice(transient_id.as_slice());
        let packed = rmp_serde::to_vec(&round).expect("msgpack encode propagation stamp round");
        salt_data.extend_from_slice(&packed);
        let salt_hash = Sha256::digest(&salt_data);
        let hk = Hkdf::<Sha256>::new(Some(salt_hash.as_slice()), transient_id.as_slice());
        let mut okm = [0u8; 256];
        hk.expand(&[], &mut okm).expect("hkdf expand propagation stamp workblock");
        workblock.extend_from_slice(&okm);
    }

    let mut stamp = vec![0u8; PROPAGATION_STAMP_SIZE];
    let mut nonce = 0u64;
    loop {
        stamp[..8].copy_from_slice(&nonce.to_le_bytes());
        let mut material = Vec::with_capacity(workblock.len() + stamp.len());
        material.extend_from_slice(&workblock);
        material.extend_from_slice(&stamp);
        let hash = Sha256::digest(&material);
        let mut value = 0u32;
        for byte in hash {
            if byte == 0 {
                value += 8;
            } else {
                value += byte.leading_zeros();
                break;
            }
        }
        if value >= target_cost {
            break;
        }
        nonce = nonce.wrapping_add(1);
    }

    let mut transient = lxm_data.to_vec();
    transient.extend_from_slice(&stamp);
    transient
}
