    #[test]
    fn peer_queue_stats_merge_case_variant_marks_without_duplicate_counts_like_python() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let stored_peer = "Peer-Stats-Mixed";
        let request_peer = stored_peer.to_ascii_lowercase();
        let duplicated = PropagationEntryRecord {
            transient_id: "ae".repeat(32),
            destination: "33".repeat(16),
            payload_hex: "33".repeat(10),
            received_at: 100,
            size_bytes: 10,
            stamp_value: None,
        };
        let unhandled = PropagationEntryRecord {
            transient_id: "af".repeat(32),
            destination: "44".repeat(16),
            payload_hex: "44".repeat(20),
            received_at: 101,
            size_bytes: 20,
            stamp_value: None,
        };
        store.upsert_propagation_entry(&duplicated).expect("duplicated entry");
        store.upsert_propagation_entry(&unhandled).expect("unhandled entry");
        store
            .mark_peer_handled_propagation(stored_peer, duplicated.transient_id.as_str())
            .expect("mark stored handled");
        store
            .mark_peer_unhandled_propagation(
                request_peer.as_str(),
                duplicated.transient_id.as_str(),
            )
            .expect("mark case-variant duplicate unhandled");
        store
            .mark_peer_unhandled_propagation(request_peer.as_str(), unhandled.transient_id.as_str())
            .expect("mark case-variant unhandled");

        assert_eq!(
            store.peer_propagation_mark_stats(stored_peer).expect("mark stats"),
            PropagationEntryStats { entries: 2, bytes: 30 }
        );
        assert_eq!(
            store.peer_propagation_message_stats(stored_peer).expect("message stats"),
            PeerPropagationMessageStats {
                outgoing: 0,
                incoming: 0,
                offered: 1,
                unhandled: 1,
                offered_bytes: 10,
                unhandled_bytes: 20,
            }
        );
        assert_eq!(
            store.list_peer_handled_propagation_ids(stored_peer).expect("handled ids"),
            vec![duplicated.transient_id]
        );
        assert_eq!(
            store.list_peer_unhandled_propagation_ids(stored_peer).expect("unhandled ids"),
            vec![unhandled.transient_id]
        );
    }

    #[test]
    fn unhandled_peer_queue_selection_matches_peer_case_insensitively_like_python() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let stored_peer = "Peer-Select-Mixed";
        let request_peer = stored_peer.to_ascii_lowercase();
        let completed = PropagationEntryRecord {
            transient_id: "b1".repeat(32),
            destination: "55".repeat(16),
            payload_hex: "55".repeat(10),
            received_at: 100,
            size_bytes: 10,
            stamp_value: None,
        };
        let pending = PropagationEntryRecord {
            transient_id: "b2".repeat(32),
            destination: "66".repeat(16),
            payload_hex: "66".repeat(20),
            received_at: 101,
            size_bytes: 20,
            stamp_value: None,
        };
        store.upsert_propagation_entry(&completed).expect("completed entry");
        store.upsert_propagation_entry(&pending).expect("pending entry");
        store
            .mark_peer_handled_propagation(stored_peer, completed.transient_id.as_str())
            .expect("mark stored handled");
        store
            .mark_peer_unhandled_propagation(request_peer.as_str(), completed.transient_id.as_str())
            .expect("mark case-variant duplicate unhandled");
        store
            .mark_peer_unhandled_propagation(request_peer.as_str(), pending.transient_id.as_str())
            .expect("mark case-variant unhandled");

        assert_eq!(
            store.list_peer_unhandled_propagation(stored_peer).expect("unhandled entries"),
            vec![pending.clone()]
        );
        assert!(store
            .remove_peer_unhandled_propagation(stored_peer, pending.transient_id.as_str())
            .expect("remove case-variant unhandled"));
        assert!(store
            .list_peer_unhandled_propagation(request_peer.as_str())
            .expect("case-variant unhandled entries")
            .is_empty());
    }

    #[test]
    fn prospective_unhandled_queue_selection_matches_peer_case_insensitively_like_python() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let stored_peer = "Peer-Prospective-Mixed";
        let request_peer = stored_peer.to_ascii_lowercase();
        let completed = PropagationEntryRecord {
            transient_id: "b5".repeat(32),
            destination: "55".repeat(16),
            payload_hex: "55".repeat(10),
            received_at: 100,
            size_bytes: 10,
            stamp_value: None,
        };
        let pending = PropagationEntryRecord {
            transient_id: "b6".repeat(32),
            destination: "66".repeat(16),
            payload_hex: "66".repeat(20),
            received_at: 101,
            size_bytes: 20,
            stamp_value: None,
        };
        store.upsert_propagation_entry(&completed).expect("completed entry");
        store.upsert_propagation_entry(&pending).expect("pending entry");
        store
            .mark_peer_received_propagation(stored_peer, completed.transient_id.as_str())
            .expect("mark stored received");
        store
            .mark_peer_unhandled_propagation(request_peer.as_str(), completed.transient_id.as_str())
            .expect("mark case-variant duplicate unhandled");
        store
            .mark_peer_unhandled_propagation(request_peer.as_str(), pending.transient_id.as_str())
            .expect("mark case-variant pending unhandled");

        assert_eq!(
            store
                .list_peer_prospective_unhandled_propagation(stored_peer)
                .expect("prospective unhandled entries"),
            vec![pending]
        );
    }

    #[test]
    fn recent_unhandled_queue_limit_skips_existing_peer_marks_like_python() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let peer = "peer-prospective-limit";
        let existing = PropagationEntryRecord {
            transient_id: "b7".repeat(32),
            destination: "77".repeat(16),
            payload_hex: "77".repeat(10),
            received_at: 200,
            size_bytes: 10,
            stamp_value: None,
        };
        let pending = PropagationEntryRecord {
            transient_id: "b8".repeat(32),
            destination: "88".repeat(16),
            payload_hex: "88".repeat(20),
            received_at: 100,
            size_bytes: 20,
            stamp_value: None,
        };
        store.upsert_propagation_entry(&existing).expect("existing entry");
        store.upsert_propagation_entry(&pending).expect("pending entry");
        store
            .mark_peer_received_propagation(peer, existing.transient_id.as_str())
            .expect("mark existing received");

        store
            .mark_recent_propagation_unhandled_for_peer(peer, 1, u64::MAX)
            .expect("mark recent prospective entry");

        assert_eq!(
            store.list_peer_unhandled_propagation_ids(peer).expect("unhandled ids"),
            vec![pending.transient_id]
        );
    }

    #[test]
    fn terminal_peer_marks_clear_case_variant_unhandled_rows_like_python() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let stored_peer = "Peer-Terminal-Mixed";
        let request_peer = stored_peer.to_ascii_lowercase();
        let transferred = PropagationEntryRecord {
            transient_id: "b3".repeat(32),
            destination: "55".repeat(16),
            payload_hex: "55".repeat(10),
            received_at: 100,
            size_bytes: 10,
            stamp_value: None,
        };
        let transfer_limited = PropagationEntryRecord {
            transient_id: "b4".repeat(32),
            destination: "66".repeat(16),
            payload_hex: "66".repeat(20),
            received_at: 101,
            size_bytes: 20,
            stamp_value: None,
        };
        store.upsert_propagation_entry(&transferred).expect("transferred entry");
        store.upsert_propagation_entry(&transfer_limited).expect("transfer-limited entry");
        for entry in [&transferred, &transfer_limited] {
            store
                .mark_peer_unhandled_propagation(request_peer.as_str(), entry.transient_id.as_str())
                .expect("mark case-variant unhandled");
        }

        store
            .mark_peer_transferred_propagation(stored_peer, transferred.transient_id.as_str())
            .expect("mark stored transferred");
        store
            .mark_peer_transfer_limited_propagation(
                stored_peer,
                transfer_limited.transient_id.as_str(),
            )
            .expect("mark stored transfer limited");

        assert!(store
            .list_peer_unhandled_propagation(request_peer.as_str())
            .expect("case-variant unhandled rows")
            .is_empty());
        assert_eq!(
            store.list_peer_handled_propagation_ids(stored_peer).expect("handled ids"),
            vec![transferred.transient_id, transfer_limited.transient_id]
        );
    }

    #[test]
    fn queue_existing_propagation_preserves_transfer_limited_marks_like_python() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let transfer_limited = PropagationEntryRecord {
            transient_id: "a1".repeat(32),
            destination: "11".repeat(16),
            payload_hex: "11".repeat(8),
            received_at: 100,
            size_bytes: 8,
            stamp_value: None,
        };
        let handled = PropagationEntryRecord {
            transient_id: "a2".repeat(32),
            destination: "22".repeat(16),
            payload_hex: "22".repeat(8),
            received_at: 101,
            size_bytes: 8,
            stamp_value: None,
        };
        store.upsert_propagation_entry(&transfer_limited).expect("transfer-limited entry");
        store.upsert_propagation_entry(&handled).expect("handled entry");
        store
            .mark_peer_transfer_limited_propagation(
                "peer-reopen",
                transfer_limited.transient_id.as_str(),
            )
            .expect("mark transfer limited");
        store
            .mark_peer_handled_propagation("peer-reopen", handled.transient_id.as_str())
            .expect("mark handled");

        store.mark_all_propagation_unhandled_for_peer("peer-reopen").expect("queue existing");

        let pending = store.list_peer_unhandled_propagation("peer-reopen").expect("pending");
        assert!(pending.is_empty());
        let handled_ids =
            store.list_peer_handled_propagation_ids("peer-reopen").expect("handled ids");
        assert_eq!(handled_ids, vec![transfer_limited.transient_id, handled.transient_id]);
    }

    #[test]
    fn mark_peer_unhandled_preserves_transfer_limited_marks_like_python() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let transfer_limited = PropagationEntryRecord {
            transient_id: "b1".repeat(32),
            destination: "11".repeat(16),
            payload_hex: "11".repeat(8),
            received_at: 100,
            size_bytes: 8,
            stamp_value: None,
        };
        let handled = PropagationEntryRecord {
            transient_id: "b2".repeat(32),
            destination: "22".repeat(16),
            payload_hex: "22".repeat(8),
            received_at: 101,
            size_bytes: 8,
            stamp_value: None,
        };
        store.upsert_propagation_entry(&transfer_limited).expect("transfer-limited entry");
        store.upsert_propagation_entry(&handled).expect("handled entry");
        store
            .mark_peer_transfer_limited_propagation(
                "peer-direct-reopen",
                transfer_limited.transient_id.as_str(),
            )
            .expect("mark transfer limited");
        store
            .mark_peer_handled_propagation("peer-direct-reopen", handled.transient_id.as_str())
            .expect("mark handled");

        store
            .mark_peer_unhandled_propagation(
                "peer-direct-reopen",
                transfer_limited.transient_id.as_str(),
            )
            .expect("ignore transfer limited");
        store
            .mark_peer_unhandled_propagation("peer-direct-reopen", handled.transient_id.as_str())
            .expect("ignore handled");

        let pending = store.list_peer_unhandled_propagation("peer-direct-reopen").expect("pending");
        assert!(pending.is_empty());
        let handled_ids =
            store.list_peer_handled_propagation_ids("peer-direct-reopen").expect("handled ids");
        assert_eq!(handled_ids, vec![transfer_limited.transient_id, handled.transient_id]);
    }

    #[test]
    fn transfer_limited_does_not_downgrade_completed_peer_marks() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let transferred = PropagationEntryRecord {
            transient_id: "c1".repeat(32),
            destination: "11".repeat(16),
            payload_hex: "11".repeat(8),
            received_at: 100,
            size_bytes: 8,
            stamp_value: None,
        };
        let received = PropagationEntryRecord {
            transient_id: "c2".repeat(32),
            destination: "22".repeat(16),
            payload_hex: "22".repeat(12),
            received_at: 101,
            size_bytes: 12,
            stamp_value: None,
        };
        store.upsert_propagation_entry(&transferred).expect("transferred entry");
        store.upsert_propagation_entry(&received).expect("received entry");
        store
            .mark_peer_transferred_propagation("peer-completed", transferred.transient_id.as_str())
            .expect("mark transferred");
        store
            .mark_peer_received_propagation("peer-completed", received.transient_id.as_str())
            .expect("mark received");

        store
            .mark_peer_transfer_limited_propagation(
                "peer-completed",
                transferred.transient_id.as_str(),
            )
            .expect("ignore transferred downgrade");
        store
            .mark_peer_transfer_limited_propagation(
                "peer-completed",
                received.transient_id.as_str(),
            )
            .expect("ignore received downgrade");

        assert_eq!(
            store.peer_propagation_message_stats("peer-completed").expect("peer stats"),
            PeerPropagationMessageStats {
                outgoing: 1,
                incoming: 1,
                offered: 1,
                unhandled: 0,
                offered_bytes: 8,
                unhandled_bytes: 0,
            }
        );
    }

    #[test]
    fn received_report_does_not_downgrade_transferred_peer_mark() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let transferred = PropagationEntryRecord {
            transient_id: "c3".repeat(32),
            destination: "33".repeat(16),
            payload_hex: "33".repeat(16),
            received_at: 102,
            size_bytes: 16,
            stamp_value: None,
        };
        store.upsert_propagation_entry(&transferred).expect("transferred entry");
        store
            .mark_peer_transferred_propagation("peer-completed", transferred.transient_id.as_str())
            .expect("mark transferred");

        store
            .mark_peer_received_propagation("peer-completed", transferred.transient_id.as_str())
            .expect("ignore received downgrade");

        assert_eq!(
            store.peer_propagation_message_stats("peer-completed").expect("peer stats"),
            PeerPropagationMessageStats {
                outgoing: 1,
                incoming: 0,
                offered: 1,
                unhandled: 0,
                offered_bytes: 16,
                unhandled_bytes: 0,
            }
        );
    }

    #[test]
    fn transferred_report_does_not_downgrade_received_peer_mark() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let received = PropagationEntryRecord {
            transient_id: "c4".repeat(32),
            destination: "44".repeat(16),
            payload_hex: "44".repeat(20),
            received_at: 103,
            size_bytes: 20,
            stamp_value: None,
        };
        store.upsert_propagation_entry(&received).expect("received entry");
        store
            .mark_peer_received_propagation("peer-completed", received.transient_id.as_str())
            .expect("mark received");

        store
            .mark_peer_transferred_propagation("peer-completed", received.transient_id.as_str())
            .expect("ignore transferred downgrade");

        assert_eq!(
            store.peer_propagation_message_stats("peer-completed").expect("peer stats"),
            PeerPropagationMessageStats {
                outgoing: 0,
                incoming: 1,
                offered: 0,
                unhandled: 0,
                offered_bytes: 0,
                unhandled_bytes: 0,
            }
        );
    }

    #[test]
    fn peer_unhandled_entries_keep_their_order_without_a_payload_sort() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        // (id byte, received_at): insertion order differs from the listed order,
        // and two entries tie on `received_at`.
        let entries: Vec<_> = [("cc", 200), ("bb", 100), ("aa", 200), ("dd", 50)]
            .into_iter()
            .map(|(id, received_at)| PropagationEntryRecord {
                transient_id: id.repeat(32),
                destination: "33".repeat(16),
                payload_hex: id.repeat(8),
                received_at,
                size_bytes: 8,
                stamp_value: None,
            })
            .collect();
        for entry in &entries {
            store.upsert_propagation_entry(entry).expect("entry");
            store
                .mark_peer_unhandled_propagation("peer-order", entry.transient_id.as_str())
                .expect("mark unhandled");
        }

        let listed: Vec<_> = store
            .list_peer_unhandled_propagation("peer-order")
            .expect("unhandled entries")
            .into_iter()
            .map(|entry| entry.transient_id)
            .collect();
        assert_eq!(listed, ["dd", "bb", "aa", "cc"].map(|id| id.repeat(32)));

        // The metadata listing differs only by leaving every payload unread.
        let metadata =
            store.list_peer_unhandled_propagation_metadata("peer-order").expect("metadata");
        let metadata_ids: Vec<_> = metadata.iter().map(|entry| entry.transient_id.clone()).collect();
        assert_eq!(metadata_ids, listed);
        assert!(metadata.iter().all(|entry| entry.payload_hex.is_empty() && entry.size_bytes == 8));
        assert_eq!(
            store.propagation_payload_hex(listed[0].as_str()).expect("payload"),
            Some("dd".repeat(8))
        );
        assert_eq!(store.propagation_payload_hex("missing").expect("missing payload"), None);

        let plan = store
            .with_read_conn(|conn| {
                conn.prepare(&format!("EXPLAIN QUERY PLAN {PEER_UNHANDLED_ENTRIES_SQL}"))?
                    .query_map(params!["peer-order", false], |row| row.get::<_, String>(3))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .expect("explain unhandled entries")
            .join(" ");
        assert!(!plan.contains("TEMP B-TREE"), "{plan}");
    }
