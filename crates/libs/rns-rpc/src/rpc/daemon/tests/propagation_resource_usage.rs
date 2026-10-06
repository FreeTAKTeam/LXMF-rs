#[test]
fn propagation_payloads_survive_reopen_and_keep_aliases_without_memory_cache() {
    let temp = tempfile::tempdir().expect("database directory");
    let path = temp.path().join("messages.db");
    let daemon =
        RpcDaemon::with_store(MessagesStore::open(&path).expect("store"), "test-identity".into());
    let destination = [0x73; 16];
    let mut payload = destination.to_vec();
    payload.extend_from_slice(b"durable propagation payload");
    let id = hex::encode(Sha256::digest(&payload));
    let alias = "ab".repeat(32);
    daemon
        .ingest_propagation_payload_bytes_with_aliases(&payload, &id, std::slice::from_ref(&alias))
        .expect("ingest");
    let mut daemon = Some(daemon);
    for _ in 0..2 {
        let instance = daemon.take().unwrap_or_else(|| {
            RpcDaemon::with_store(
                MessagesStore::open(&path).expect("reopen"),
                "test-identity".into(),
            )
        });
        assert!(instance.has_propagation_payload(&id));
        let listed = instance.list_propagation_payloads_for_destination(&destination);
        assert_eq!(listed.len(), 2);
        for key in [&id, &alias] {
            let reply = instance
                .handle_rpc(rpc_request(1, "propagation_fetch", json!({"transient_id": key})))
                .expect("fetch");
            assert_eq!(reply.result.expect("result")["payload_hex"], hex::encode(&payload));
        }
        let stats = instance
            .handle_rpc(rpc_request(2, "router_stats", json!({})))
            .expect("stats")
            .result
            .expect("stats result");
        assert_eq!(stats["propagation_payloads"], 2);
    }
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "explicit persistent propagation memory qualification"]
fn profile_propagation_payload_retention() {
    fn rss_kib() -> u64 {
        std::fs::read_to_string("/proc/self/status")
            .expect("process status")
            .lines()
            .find_map(|line| line.strip_prefix("VmRSS:"))
            .expect("RSS")
            .split_whitespace()
            .next()
            .expect("RSS value")
            .parse()
            .expect("RSS number")
    }
    let temp = tempfile::tempdir().expect("database");
    let daemon = RpcDaemon::with_store(
        MessagesStore::open(&temp.path().join("messages.db")).expect("store"),
        "test-identity".into(),
    );
    let before = rss_kib();
    let start = std::time::Instant::now();
    let destination = [0x74; 16];
    for index in 0_u64..1000 {
        let mut payload = vec![0x42; 65_536];
        payload[..16].copy_from_slice(&destination);
        payload[16..24].copy_from_slice(&index.to_be_bytes());
        daemon.ingest_client_propagation_payload_bytes_at_cost(&payload, None, 0).expect("ingest");
    }
    let after_ingest = rss_kib();
    let list_start = std::time::Instant::now();
    let listed = daemon.list_propagation_payloads_for_destination(&destination);
    let list_ms = list_start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(listed.len(), 1000);
    let stats = daemon.store.propagation_entry_stats().expect("stats");
    assert_eq!(stats.entries, 1000);
    println!("rows={} payload_bytes={} rss_before_kib={before} rss_after_ingest_kib={after_ingest} rss_after_listing_kib={} ingest_ms={} list_ms={list_ms}", stats.entries, stats.bytes, rss_kib(), start.elapsed().as_secs_f64()*1000.0);
}
