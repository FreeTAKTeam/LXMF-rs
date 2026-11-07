use super::*;

fn record(id: usize, peer: &str, timestamp: i64) -> AnnounceRecord {
    AnnounceRecord {
        id: format!("{id:08}"),
        peer: peer.into(),
        timestamp,
        name: Some(format!("name-{id}")),
        name_source: None,
        first_seen: 0,
        seen_count: 1,
        app_data_hex: None,
        capabilities: vec![],
        rssi: None,
        snr: None,
        q: None,
        stamp_cost: None,
        stamp_cost_flexibility: None,
        peering_cost: None,
    }
}

#[test]
fn pages_reconcile_quiet_peers_bursts_ties_and_updates_behind_cursor() {
    let store = MessagesStore::in_memory().unwrap();
    for i in 0..70 {
        store.insert_announce(&record(i, &format!("peer-{i:04}"), 10)).unwrap();
    }
    for i in 70..170 {
        store.insert_announce(&record(i, "peer-0000", 20)).unwrap();
    }
    let first = store.broker_announce_peer_page(None).unwrap();
    assert_eq!(first.len(), 32);
    assert_eq!(first[0].id, "00000169"); // deterministic timestamp tie
    assert_eq!(first[31].peer, "peer-0031");
    store.insert_announce(&record(170, "peer-0000", 30)).unwrap();
    store.insert_announce(&record(171, "peer-0069", 30)).unwrap();
    let second = store.broker_announce_peer_page(Some(&first[31].peer)).unwrap();
    assert_eq!(second.len(), 32);
    let last = store.broker_announce_peer_page(Some(&second[31].peer)).unwrap();
    assert_eq!(last.len(), 6);
    assert_eq!(last[5].id, "00000171");
    // Complete sweeps reconcile changes behind the cursor without timestamp gaps.
    assert_eq!(store.broker_announce_peer_page(None).unwrap()[0].id, "00000170");
    assert!(store.broker_announce_peer_page(Some("zzzz")).unwrap().is_empty());
    while store.prune_announce_history_batch(0).unwrap() != 0 {}
    assert_eq!(store.broker_announce_peer_page(None).unwrap().len(), 32);
    assert_eq!(store.broker_announce_peer_page(Some("peer-0063")).unwrap().len(), 6);
}

#[test]
fn projection_rejects_oversized_records_before_decoding_and_replays_after_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("announces.db");
    {
        let store = MessagesStore::open(&path).unwrap();
        store.insert_announce(&record(1, "quiet", 1)).unwrap();
    }
    let store = MessagesStore::open(&path).unwrap();
    assert_eq!(store.broker_announce_peer_page(None).unwrap()[0].peer, "quiet");
    let mut huge = record(2, "large", 2);
    huge.app_data_hex = Some("a".repeat(128 * 1024));
    store.insert_announce(&huge).unwrap();
    assert!(store.broker_announce_peer_page(None).unwrap_err().to_string().contains("128 KiB"));
}
