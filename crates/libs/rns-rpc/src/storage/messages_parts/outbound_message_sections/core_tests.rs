    use super::*;

    use serde_json::json;

    fn outbound_message(id: &str, timestamp: i64, receipt_status: Option<&str>) -> MessageRecord {
        MessageRecord {
            id: id.to_string(),
            source: "src".to_string(),
            destination: "dst".to_string(),
            title: "title".to_string(),
            content: "body".to_string(),
            timestamp,
            direction: "out".to_string(),
            fields: None,
            receipt_status: receipt_status.map(ToString::to_string),
        }
    }

    #[test]
    fn sdk_domain_snapshot_roundtrip() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let initial = store.get_sdk_domain_snapshot().expect("query snapshot");
        assert!(initial.is_none(), "snapshot should be absent before first write");

        let snapshot = json!({
            "topics": [{ "topic_id": "topic-1" }],
            "attachments": [],
            "markers": [],
        });
        store.put_sdk_domain_snapshot(&snapshot).expect("persist snapshot");

        let loaded = store.get_sdk_domain_snapshot().expect("load snapshot");
        assert_eq!(loaded, Some(snapshot));
    }

    #[test]
    fn sdk_domain_snapshot_clear_removes_record() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store
            .put_sdk_domain_snapshot(&json!({ "voice_sessions": [{ "session_id": "voice-1" }] }))
            .expect("persist snapshot");
        store.clear_sdk_domain_snapshot().expect("clear snapshot");
        let loaded = store.get_sdk_domain_snapshot().expect("load snapshot");
        assert!(loaded.is_none(), "snapshot should be removed after clear");
    }

    #[test]
    fn message_count_uses_direct_count() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store.insert_message(&outbound_message("msg-1", 1, None)).expect("insert msg-1");
        store
            .insert_message(&outbound_message("msg-2", 2, Some("delivered")))
            .expect("insert msg-2");

        assert_eq!(store.message_count().expect("count messages"), 2);
    }

    #[test]
    fn message_count_cache_ignores_replace_for_existing_id() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store.insert_message(&outbound_message("msg-1", 1, None)).expect("insert original");
        store
            .insert_message(&outbound_message("msg-1", 2, Some("delivered")))
            .expect("replace existing");

        assert_eq!(store.message_count().expect("count messages"), 1);
    }

    #[test]
    fn configure_connection_sets_busy_timeout() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let busy_timeout_ms = store.busy_timeout_ms().expect("query busy_timeout");
        assert_eq!(busy_timeout_ms, 5_000);
    }

    #[test]
    fn prune_expired_tickets_matches_python_available_ticket_cleanup() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store.upsert_outbound_ticket("expired-outbound", "00", 90).expect("outbound expired");
        store.upsert_outbound_ticket("valid-outbound", "11", 110).expect("outbound valid");
        store.upsert_ticket("inbound-grace", "22", 90).expect("inbound grace");
        store.upsert_ticket("inbound-expired", "33", 89).expect("inbound expired");

        store.prune_expired_tickets(100, 10).expect("prune tickets");

        assert!(store.get_outbound_ticket("expired-outbound").expect("expired outbound").is_none());
        assert!(store.get_outbound_ticket("valid-outbound").expect("valid outbound").is_some());
        assert!(store.get_ticket("inbound-grace").expect("inbound grace").is_some());
        assert!(store.get_ticket("inbound-expired").expect("inbound expired").is_none());
    }

    #[test]
    fn announce_and_ticket_writes_run_on_writer_lane() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store
            .insert_announce(&AnnounceRecord {
                id: "ann-1".to_string(),
                peer: "peer-a".to_string(),
                timestamp: 100,
                name: Some("Peer A".to_string()),
                name_source: Some("app_data".to_string()),
                first_seen: 90,
                seen_count: 2,
                app_data_hex: Some("0102".to_string()),
                capabilities: vec!["lxmf.delivery".to_string()],
                rssi: Some(-42.0),
                snr: Some(7.0),
                q: Some(0.9),
                stamp_cost: Some(4),
                stamp_cost_flexibility: Some(1),
                peering_cost: Some(2),
            })
            .expect("insert announce");
        store.upsert_ticket("peer-a", "22", 200).expect("upsert inbound ticket");
        store.upsert_outbound_ticket("peer-a", "33", 210).expect("upsert outbound ticket");
        store.upsert_ticket_last_delivery("peer-a", 110).expect("upsert last delivery");

        let announces = store.list_announces(10, None, None).expect("list announces");
        assert_eq!(announces.len(), 1);
        assert_eq!(announces[0].peer, "peer-a");
        assert_eq!(announces[0].capabilities, vec!["lxmf.delivery".to_string()]);
        assert_eq!(store.get_ticket("peer-a").expect("inbound ticket"), Some(("22".into(), 200)));
        assert_eq!(
            store.get_outbound_ticket("peer-a").expect("outbound ticket"),
            Some(("33".into(), 210))
        );
        assert_eq!(store.get_ticket_last_delivery("peer-a").expect("last delivery"), Some(110));
    }

    #[test]
    fn inbound_tickets_keep_multiple_generated_tickets_per_destination() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store.upsert_ticket("peer", "22", 200).expect("insert first ticket");
        store.upsert_ticket("peer", "33", 300).expect("insert second ticket");

        let tickets = store.get_tickets_for_destination("peer").expect("load tickets");

        assert_eq!(tickets, vec![("33".to_string(), 300), ("22".to_string(), 200)]);
        assert_eq!(store.get_ticket("peer").expect("load latest"), Some(("33".to_string(), 300)));
    }

    #[test]
    fn opening_old_single_ticket_schema_migrates_to_multi_ticket_schema() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("single-ticket-schema.sqlite");
        {
            let conn = Connection::open(db_path.as_path()).expect("open raw sqlite");
            conn.execute_batch(
                "CREATE TABLE tickets (
                    destination TEXT PRIMARY KEY,
                    ticket TEXT NOT NULL,
                    expires_at INTEGER NOT NULL
                );
                INSERT INTO tickets (destination, ticket, expires_at)
                    VALUES ('peer', '22', 200);",
            )
            .expect("seed old schema");
        }

        let store = MessagesStore::open(db_path.as_path()).expect("open migrated store");
        store.upsert_ticket("peer", "33", 300).expect("insert second ticket");

        let tickets = store.get_tickets_for_destination("peer").expect("load tickets");
        assert_eq!(tickets, vec![("33".to_string(), 300), ("22".to_string(), 200)]);
    }

    #[test]
    fn expire_outbound_messages_marks_non_terminal_records() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store
            .insert_message(&outbound_message("out-non-terminal", 10, None))
            .expect("insert non-terminal");
        store
            .insert_message(&outbound_message("out-terminal", 10, Some("delivered")))
            .expect("insert terminal");
        let expired = store.expire_outbound_messages_before(11).expect("expire outbound");
        assert_eq!(expired, vec!["out-non-terminal".to_string()]);
        let non_terminal = store
            .get_message("out-non-terminal")
            .expect("load non-terminal")
            .expect("non-terminal exists");
        assert_eq!(non_terminal.receipt_status.as_deref(), Some("expired"));
        let terminal =
            store.get_message("out-terminal").expect("load terminal").expect("terminal exists");
        assert_eq!(terminal.receipt_status.as_deref(), Some("delivered"));
    }

    #[test]
    fn detailed_failed_status_is_terminal_for_expiry_and_buckets() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store
            .insert_message(&outbound_message("out-failed-detail", 10, Some("failed: no path")))
            .expect("insert detailed failure");
        store
            .insert_message(&outbound_message("out-sending", 11, Some("sending")))
            .expect("insert sending");

        let expired = store.expire_outbound_messages_before(12).expect("expire outbound");
        assert_eq!(expired, vec!["out-sending".to_string()]);
        let failed =
            store.get_message("out-failed-detail").expect("load failed").expect("failed exists");
        assert_eq!(failed.receipt_status.as_deref(), Some("failed: no path"));

        let (queued, in_flight) = store.count_message_buckets().expect("message buckets");
        assert_eq!(queued, 0);
        assert_eq!(in_flight, 0);
    }

    #[test]
    fn prune_outbound_messages_terminal_first_prefers_terminal_records() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store
            .insert_message(&outbound_message("msg-terminal-old", 1, Some("sent: direct")))
            .expect("insert terminal old");
        store
            .insert_message(&outbound_message("msg-non-terminal", 2, None))
            .expect("insert non-terminal");
        store
            .insert_message(&outbound_message("msg-terminal-new", 3, Some("delivered")))
            .expect("insert terminal new");

        let pruned = store.prune_outbound_messages(2, "terminal_first").expect("prune outbound");
        assert_eq!(pruned.len(), 2);
        assert!(pruned.iter().any(|id| id == "msg-terminal-old"));
        assert!(pruned.iter().any(|id| id == "msg-terminal-new"));
        assert!(
            store.get_message("msg-non-terminal").expect("load non-terminal").is_some(),
            "non-terminal record should remain when terminal records satisfy prune count"
        );
        assert_eq!(store.message_count().expect("count after prune"), 1);
    }

    #[test]
    fn prune_outbound_messages_terminal_first_treats_detailed_failed_status_as_terminal() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store
            .insert_message(&outbound_message("msg-failed-detail", 1, Some("failed: no path")))
            .expect("insert detailed failure");
        store
            .insert_message(&outbound_message("msg-sending", 2, Some("sending")))
            .expect("insert sending");

        let pruned = store.prune_outbound_messages(1, "terminal_first").expect("prune outbound");
        assert_eq!(pruned, vec!["msg-failed-detail".to_string()]);
        assert!(
            store.get_message("msg-sending").expect("load sending").is_some(),
            "sending record should remain when detailed failure satisfies prune count"
        );
    }

    #[test]
    fn peer_message_stats_treats_detailed_failed_status_as_terminal() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let mut failed = outbound_message("peer-failed-detail", 1, Some("failed: no path"));
        failed.destination = "peer-a".to_string();
        let mut sending = outbound_message("peer-sending", 2, Some("sending"));
        sending.destination = "peer-a".to_string();
        store.insert_message(&failed).expect("insert detailed failure");
        store.insert_message(&sending).expect("insert sending");

        let stats = store.peer_message_stats("peer-a").expect("peer stats");
        assert_eq!(stats.outgoing, 2);
        assert_eq!(stats.offered, 1);
    }

    #[test]
    fn clear_messages_resets_message_count_cache() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store.insert_message(&outbound_message("msg-1", 1, None)).expect("insert msg-1");
        store
            .insert_message(&outbound_message("msg-2", 2, Some("delivered")))
            .expect("insert msg-2");

        store.clear_messages().expect("clear messages");

        assert_eq!(store.message_count().expect("count after clear"), 0);
    }

    #[test]
    fn prune_messages_to_limit_bytes_removes_oldest_messages() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let mut first = outbound_message("msg-1", 1, None);
        first.content = "a".repeat(128);
        let mut second = outbound_message("msg-2", 2, None);
        second.content = "b".repeat(128);
        store.insert_message(&first).expect("insert first");
        store.insert_message(&second).expect("insert second");

        let before = store.message_storage_stats().expect("stats before");
        let pruned =
            store.prune_messages_to_limit_bytes(before.bytes.saturating_sub(64)).expect("prune");

        assert_eq!(pruned, vec!["msg-1".to_string()]);
        let remaining = store.list_messages(10, None).expect("remaining");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, "msg-2");
    }

    #[test]
    fn scheduled_prune_messages_to_limit_bytes_runs_on_writer_lane() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let mut first = outbound_message("msg-1", 1, None);
        first.content = "a".repeat(128);
        let mut second = outbound_message("msg-2", 2, None);
        second.content = "b".repeat(128);
        store.insert_message(&first).expect("insert first");
        store.insert_message(&second).expect("insert second");

        let before = store.message_storage_stats().expect("stats before");
        store
            .schedule_prune_messages_to_limit_bytes(before.bytes.saturating_sub(64))
            .expect("schedule prune");
        store
            .insert_message(&outbound_message("flush-after-scheduled-prune", 3, Some("sent")))
            .expect("flush writer lane");

        let remaining = store.list_messages(10, None).expect("remaining");
        assert_eq!(remaining.len(), 2);
        assert!(remaining.iter().any(|record| record.id == "msg-2"));
        assert!(remaining.iter().any(|record| record.id == "flush-after-scheduled-prune"));
        assert!(
            remaining.iter().all(|record| record.id != "msg-1"),
            "scheduled prune should remove the oldest oversized record before later writes"
        );
    }

    #[test]
    fn peer_message_stats_reports_incoming_and_outgoing_counts() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let mut outbound = outbound_message("msg-out", 1, None);
        outbound.destination = "peer-a".to_string();
        let inbound = MessageRecord {
            id: "msg-in".to_string(),
            source: "peer-a".to_string(),
            destination: "local".to_string(),
            title: "title".to_string(),
            content: "body".to_string(),
            timestamp: 2,
            direction: "in".to_string(),
            fields: None,
            receipt_status: None,
        };
        store.insert_message(&outbound).expect("insert outbound");
        store.insert_message(&inbound).expect("insert inbound");

        let stats = store.peer_message_stats("peer-a").expect("peer stats");
        assert_eq!(stats.outgoing, 1);
        assert_eq!(stats.incoming, 1);
        assert_eq!(stats.offered, 1);
        assert_eq!(stats.unhandled, 1);
    }

    #[test]
    fn propagation_entry_roundtrip_persists_payload_metadata() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let record = PropagationEntryRecord {
            transient_id: "aa".repeat(32),
            destination: "11".repeat(16),
            payload_hex: "deadbeef".to_string(),
            received_at: 1_770_000_000,
            size_bytes: 4,
            stamp_value: Some(13),
        };

        store.upsert_propagation_entry(&record).expect("upsert propagation entry");

        let loaded = store
            .get_propagation_entry(record.transient_id.as_str())
            .expect("load propagation entry")
            .expect("entry exists");
        assert_eq!(loaded, record);
        let stats = store.propagation_entry_stats().expect("propagation stats");
        assert_eq!(stats.entries, 1);
        assert_eq!(stats.bytes, 4);
    }

    #[test]
    fn propagation_peer_marks_track_python_handled_and_unhandled_lists() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let first = PropagationEntryRecord {
            transient_id: "aa".repeat(32),
            destination: "11".repeat(16),
            payload_hex: "aaaa".to_string(),
            received_at: 1,
            size_bytes: 2,
            stamp_value: Some(7),
        };
        let second = PropagationEntryRecord {
            transient_id: "bb".repeat(32),
            destination: "11".repeat(16),
            payload_hex: "bbbbbb".to_string(),
            received_at: 2,
            size_bytes: 3,
            stamp_value: Some(9),
        };
        store.upsert_propagation_entry(&first).expect("upsert first");
        store.upsert_propagation_entry(&second).expect("upsert second");

        store
            .mark_peer_unhandled_propagation("peer-a", first.transient_id.as_str())
            .expect("mark first unhandled");
        store
            .mark_peer_unhandled_propagation("peer-a", second.transient_id.as_str())
            .expect("mark second unhandled");
        store
            .mark_peer_handled_propagation("peer-a", first.transient_id.as_str())
            .expect("mark first handled");

        let pending = store.list_peer_unhandled_propagation("peer-a").expect("list peer unhandled");
        assert_eq!(pending, vec![second.clone()]);

        let handled =
            store.list_peer_handled_propagation_ids("peer-a").expect("list peer handled ids");
        assert_eq!(handled, vec![first.transient_id]);
    }

    #[test]
    fn stale_peer_mark_cleanup_matches_peer_case_insensitively_like_python() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let stored_peer = "Peer-Stale-Mixed";
        let request_peer = stored_peer.to_ascii_lowercase();
        let unhandled_id = "ac".repeat(32);
        let handled_id = "ad".repeat(32);

        store
            .mark_peer_unhandled_propagation(stored_peer, unhandled_id.as_str())
            .expect("mark stale unhandled");
        store
            .mark_peer_handled_propagation(stored_peer, handled_id.as_str())
            .expect("mark stale handled");

        assert_eq!(
            store
                .remove_stale_peer_unhandled_propagation_ids(request_peer.as_str())
                .expect("remove stale unhandled"),
            vec![unhandled_id]
        );
        assert_eq!(
            store
                .remove_stale_peer_completed_propagation_ids(request_peer.as_str())
                .expect("remove stale completed"),
            vec![handled_id]
        );
        assert!(store
            .remove_stale_peer_unhandled_propagation_ids(stored_peer)
            .expect("stored-case stale unhandled")
            .is_empty());
        assert!(store
            .remove_stale_peer_completed_propagation_ids(stored_peer)
            .expect("stored-case stale completed")
            .is_empty());
    }
