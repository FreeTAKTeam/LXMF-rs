    #[test]
    fn received_report_does_not_downgrade_transfer_limited_peer_mark() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let transfer_limited = PropagationEntryRecord {
            transient_id: "c9".repeat(32),
            destination: "99".repeat(16),
            payload_hex: "99".repeat(24),
            received_at: 108,
            size_bytes: 24,
            stamp_value: None,
        };
        store.upsert_propagation_entry(&transfer_limited).expect("transfer limited entry");
        store
            .mark_peer_transfer_limited_propagation(
                "peer-completed",
                transfer_limited.transient_id.as_str(),
            )
            .expect("mark transfer limited");

        store
            .mark_peer_received_propagation(
                "peer-completed",
                transfer_limited.transient_id.as_str(),
            )
            .expect("ignore received downgrade");

        assert_eq!(
            store.peer_propagation_message_stats("peer-completed").expect("peer stats"),
            PeerPropagationMessageStats {
                outgoing: 0,
                incoming: 0,
                offered: 0,
                unhandled: 0,
                offered_bytes: 0,
                unhandled_bytes: 0,
            }
        );
        assert_eq!(
            store.list_peer_handled_propagation_ids("peer-completed").expect("handled ids"),
            vec![transfer_limited.transient_id]
        );
    }

    #[test]
    fn transferred_report_does_not_downgrade_transfer_limited_peer_mark() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let transfer_limited = PropagationEntryRecord {
            transient_id: "c8".repeat(32),
            destination: "88".repeat(16),
            payload_hex: "88".repeat(24),
            received_at: 107,
            size_bytes: 24,
            stamp_value: None,
        };
        store.upsert_propagation_entry(&transfer_limited).expect("transfer limited entry");
        store
            .mark_peer_transfer_limited_propagation(
                "peer-completed",
                transfer_limited.transient_id.as_str(),
            )
            .expect("mark transfer limited");

        store
            .mark_peer_transferred_propagation(
                "peer-completed",
                transfer_limited.transient_id.as_str(),
            )
            .expect("ignore transferred downgrade");

        assert_eq!(
            store.peer_propagation_message_stats("peer-completed").expect("peer stats"),
            PeerPropagationMessageStats {
                outgoing: 0,
                incoming: 0,
                offered: 0,
                unhandled: 0,
                offered_bytes: 0,
                unhandled_bytes: 0,
            }
        );
        assert_eq!(
            store.list_peer_handled_propagation_ids("peer-completed").expect("handled ids"),
            vec![transfer_limited.transient_id]
        );
    }

    #[test]
    fn handled_report_does_not_downgrade_completed_peer_marks() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let transferred = PropagationEntryRecord {
            transient_id: "c5".repeat(32),
            destination: "55".repeat(16),
            payload_hex: "55".repeat(24),
            received_at: 104,
            size_bytes: 24,
            stamp_value: None,
        };
        let received = PropagationEntryRecord {
            transient_id: "c6".repeat(32),
            destination: "66".repeat(16),
            payload_hex: "66".repeat(28),
            received_at: 105,
            size_bytes: 28,
            stamp_value: None,
        };
        let transfer_limited = PropagationEntryRecord {
            transient_id: "c7".repeat(32),
            destination: "77".repeat(16),
            payload_hex: "77".repeat(32),
            received_at: 106,
            size_bytes: 32,
            stamp_value: None,
        };
        store.upsert_propagation_entry(&transferred).expect("transferred entry");
        store.upsert_propagation_entry(&received).expect("received entry");
        store.upsert_propagation_entry(&transfer_limited).expect("transfer limited entry");
        store
            .mark_peer_transferred_propagation("peer-completed", transferred.transient_id.as_str())
            .expect("mark transferred");
        store
            .mark_peer_received_propagation("peer-completed", received.transient_id.as_str())
            .expect("mark received");
        store
            .mark_peer_transfer_limited_propagation(
                "peer-completed",
                transfer_limited.transient_id.as_str(),
            )
            .expect("mark transfer limited");

        store
            .mark_peer_handled_propagation("peer-completed", transferred.transient_id.as_str())
            .expect("ignore transferred downgrade");
        store
            .mark_peer_handled_propagation("peer-completed", received.transient_id.as_str())
            .expect("ignore received downgrade");
        store
            .mark_peer_handled_propagation("peer-completed", transfer_limited.transient_id.as_str())
            .expect("ignore transfer-limited downgrade");

        assert_eq!(
            store.peer_propagation_message_stats("peer-completed").expect("peer stats"),
            PeerPropagationMessageStats {
                outgoing: 1,
                incoming: 1,
                offered: 1,
                unhandled: 0,
                offered_bytes: 24,
                unhandled_bytes: 0,
            }
        );
    }

    #[test]
    fn peer_propagation_message_stats_counts_offered_and_unhandled_marks() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let handled = PropagationEntryRecord {
            transient_id: "aa".repeat(32),
            destination: "11".repeat(16),
            payload_hex: "aa".repeat(12),
            received_at: 1_700_000_001,
            size_bytes: 12,
            stamp_value: None,
        };
        let unhandled = PropagationEntryRecord {
            transient_id: "bb".repeat(32),
            destination: "11".repeat(16),
            payload_hex: "bb".repeat(24),
            received_at: 1_700_000_002,
            size_bytes: 24,
            stamp_value: None,
        };
        let other = PropagationEntryRecord {
            transient_id: "cc".repeat(32),
            destination: "11".repeat(16),
            payload_hex: "cc".repeat(36),
            received_at: 1_700_000_003,
            size_bytes: 36,
            stamp_value: None,
        };
        let received = PropagationEntryRecord {
            transient_id: "ee".repeat(32),
            destination: "11".repeat(16),
            payload_hex: "ee".repeat(48),
            received_at: 1_700_000_004,
            size_bytes: 48,
            stamp_value: None,
        };
        for entry in [&handled, &unhandled, &other, &received] {
            store.upsert_propagation_entry(entry).expect("upsert entry");
        }
        store
            .mark_peer_handled_propagation("peer-a", handled.transient_id.as_str())
            .expect("mark handled");
        store
            .mark_peer_transferred_propagation("peer-a", handled.transient_id.as_str())
            .expect("mark transferred");
        store
            .mark_peer_transfer_limited_propagation("peer-a", other.transient_id.as_str())
            .expect("mark transfer limited");
        store
            .mark_peer_received_propagation("peer-a", received.transient_id.as_str())
            .expect("mark received");
        store
            .mark_peer_unhandled_propagation("peer-a", unhandled.transient_id.as_str())
            .expect("mark unhandled");
        store
            .mark_peer_handled_propagation("peer-a", "dd".repeat(32).as_str())
            .expect("mark stale handled");
        store
            .mark_peer_unhandled_propagation("peer-b", other.transient_id.as_str())
            .expect("mark other peer unhandled");

        assert_eq!(
            store.peer_propagation_message_stats("peer-a").expect("peer-a stats"),
            PeerPropagationMessageStats {
                outgoing: 1,
                incoming: 1,
                offered: 1,
                unhandled: 1,
                offered_bytes: 12,
                unhandled_bytes: 24,
            }
        );
        assert_eq!(
            store.peer_propagation_message_stats("peer-b").expect("peer-b stats"),
            PeerPropagationMessageStats {
                outgoing: 0,
                incoming: 0,
                offered: 0,
                unhandled: 1,
                offered_bytes: 0,
                unhandled_bytes: 36,
            }
        );
    }

    #[test]
    fn clear_all_peer_propagation_marks_removes_every_peer_queue_mark() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let entry_a = PropagationEntryRecord {
            transient_id: "ab".repeat(32),
            destination: "11".repeat(16),
            payload_hex: "11".repeat(8),
            received_at: 1_700_000_010,
            size_bytes: 8,
            stamp_value: None,
        };
        let entry_b = PropagationEntryRecord {
            transient_id: "bc".repeat(32),
            destination: "22".repeat(16),
            payload_hex: "22".repeat(8),
            received_at: 1_700_000_011,
            size_bytes: 8,
            stamp_value: None,
        };
        store.upsert_propagation_entry(&entry_a).expect("upsert entry a");
        store.upsert_propagation_entry(&entry_b).expect("upsert entry b");
        store
            .mark_peer_unhandled_propagation("peer-a", entry_a.transient_id.as_str())
            .expect("mark peer-a unhandled");
        store
            .mark_peer_handled_propagation("peer-b", entry_b.transient_id.as_str())
            .expect("mark peer-b handled");

        assert_eq!(store.clear_all_peer_propagation_marks().expect("clear marks"), 2);

        assert!(store.list_peer_unhandled_propagation("peer-a").expect("peer-a marks").is_empty());
        assert!(store
            .list_peer_handled_propagation_ids("peer-b")
            .expect("peer-b marks")
            .is_empty());
    }

    #[test]
    fn propagation_entries_for_destination_apply_python_sync_budget_order() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let destination = "22".repeat(16);
        let large = PropagationEntryRecord {
            transient_id: "cc".repeat(32),
            destination: destination.clone(),
            payload_hex: "aa".repeat(100),
            received_at: 1,
            size_bytes: 100,
            stamp_value: Some(2),
        };
        let small = PropagationEntryRecord {
            transient_id: "dd".repeat(32),
            destination: destination.clone(),
            payload_hex: "bb".repeat(20),
            received_at: 2,
            size_bytes: 20,
            stamp_value: Some(3),
        };
        let other_destination = PropagationEntryRecord {
            transient_id: "ee".repeat(32),
            destination: "33".repeat(16),
            payload_hex: "cc".repeat(8),
            received_at: 3,
            size_bytes: 8,
            stamp_value: Some(4),
        };
        store.upsert_propagation_entry(&large).expect("upsert large");
        store.upsert_propagation_entry(&small).expect("upsert small");
        store.upsert_propagation_entry(&other_destination).expect("upsert other");

        let entries = store
            .list_propagation_entries_for_destination(destination.as_str())
            .expect("list destination entries");
        assert_eq!(
            entries.iter().map(|entry| entry.transient_id.as_str()).collect::<Vec<_>>(),
            vec![small.transient_id.as_str(), large.transient_id.as_str()]
        );

        let fetched = store
            .fetch_propagation_payloads_for_destination(
                destination.as_str(),
                &[small.transient_id.clone(), large.transient_id.clone()],
                Some(24 + 20 + 32 + 16),
            )
            .expect("fetch payloads under budget");
        assert_eq!(fetched, vec![hex::decode(small.payload_hex).expect("small payload hex")]);
    }

    #[test]
    fn purge_propagation_entries_removes_unhandled_marks_but_preserves_completed_state() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let entry = PropagationEntryRecord {
            transient_id: "af".repeat(32),
            destination: "44".repeat(16),
            payload_hex: "44".repeat(16),
            received_at: 1,
            size_bytes: 16,
            stamp_value: None,
        };
        store.upsert_propagation_entry(&entry).expect("upsert propagation entry");
        store
            .mark_peer_handled_propagation("peer-cleanup", entry.transient_id.as_str())
            .expect("mark handled");
        store
            .mark_peer_unhandled_propagation("peer-retry", entry.transient_id.as_str())
            .expect("mark unhandled");

        let purged = store
            .purge_propagation_entries_for_destination(
                entry.destination.as_str(),
                std::slice::from_ref(&entry.transient_id),
            )
            .expect("purge propagation entry");

        assert_eq!(purged, 1);
        assert!(store
            .list_peer_handled_propagation_ids("peer-cleanup")
            .expect("handled ids")
            .is_empty());
        assert!(
            store
                .peer_completed_propagation_mark_exists("peer-cleanup", entry.transient_id.as_str())
                .expect("completed mark"),
            "completed peer accounting survives payload purge for future reingest"
        );
        assert!(
            store
                .list_peer_unhandled_propagation_ids("peer-retry")
                .expect("unhandled ids")
                .is_empty(),
            "retryable marks for the deleted payload are stale and should be removed"
        );
    }

    #[test]
    fn prune_propagation_entries_to_limit_bytes_removes_oldest_payloads() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let old = PropagationEntryRecord {
            transient_id: "10".repeat(32),
            destination: "44".repeat(16),
            payload_hex: "10".repeat(80),
            received_at: 1,
            size_bytes: 80,
            stamp_value: None,
        };
        let new = PropagationEntryRecord {
            transient_id: "20".repeat(32),
            destination: "55".repeat(16),
            payload_hex: "20".repeat(40),
            received_at: 2,
            size_bytes: 40,
            stamp_value: None,
        };
        store.upsert_propagation_entry(&old).expect("upsert old");
        store.upsert_propagation_entry(&new).expect("upsert new");
        store
            .mark_peer_unhandled_propagation("peer-prune", old.transient_id.as_str())
            .expect("mark old unhandled");
        store
            .mark_peer_unhandled_propagation("peer-prune", new.transient_id.as_str())
            .expect("mark new unhandled");
        store
            .mark_peer_handled_propagation("peer-done", old.transient_id.as_str())
            .expect("mark old handled");

        let pruned =
            store.prune_propagation_entries_to_limit_bytes(64).expect("prune propagation entries");

        assert_eq!(pruned, vec![old.transient_id.clone()]);
        assert!(store
            .get_propagation_entry(old.transient_id.as_str())
            .expect("old lookup")
            .is_none());
        assert!(store
            .get_propagation_entry(new.transient_id.as_str())
            .expect("new lookup")
            .is_some());
        assert_eq!(
            store.list_peer_unhandled_propagation_ids("peer-prune").expect("peer unhandled ids"),
            vec![new.transient_id]
        );
        assert!(
            store
                .peer_completed_propagation_mark_exists("peer-done", old.transient_id.as_str())
                .expect("completed mark"),
            "completed peer accounting should survive propagation storage pruning"
        );
    }
