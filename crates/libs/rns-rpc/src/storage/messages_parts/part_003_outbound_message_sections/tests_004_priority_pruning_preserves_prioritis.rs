    #[test]
    fn priority_pruning_preserves_prioritised_destination_payloads() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let prioritised = PropagationEntryRecord {
            transient_id: "30".repeat(32),
            destination: "66".repeat(16),
            payload_hex: "30".repeat(80),
            received_at: 1,
            size_bytes: 80,
            stamp_value: None,
        };
        let ordinary = PropagationEntryRecord {
            transient_id: "40".repeat(32),
            destination: "77".repeat(16),
            payload_hex: "40".repeat(40),
            received_at: 2,
            size_bytes: 40,
            stamp_value: None,
        };
        store.upsert_propagation_entry(&prioritised).expect("upsert prioritised");
        store.upsert_propagation_entry(&ordinary).expect("upsert ordinary");
        store
            .mark_peer_unhandled_propagation("peer-prune", prioritised.transient_id.as_str())
            .expect("mark prioritised unhandled");
        store
            .mark_peer_unhandled_propagation("peer-prune", ordinary.transient_id.as_str())
            .expect("mark ordinary unhandled");

        let pruned = store
            .prune_propagation_entries_to_limit_bytes_with_priorities(
                80,
                std::slice::from_ref(&prioritised.destination),
            )
            .expect("priority prune propagation entries");

        assert_eq!(pruned, vec![ordinary.transient_id.clone()]);
        assert!(store
            .get_propagation_entry(prioritised.transient_id.as_str())
            .expect("prioritised lookup")
            .is_some());
        assert!(store
            .get_propagation_entry(ordinary.transient_id.as_str())
            .expect("ordinary lookup")
            .is_none());
        assert_eq!(
            store.list_peer_unhandled_propagation_ids("peer-prune").expect("peer unhandled ids"),
            vec![prioritised.transient_id]
        );
    }

    #[test]
    fn local_propagation_processed_mark_is_idempotent() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let transient_id = "ac".repeat(32);

        assert!(!store
            .local_propagation_processed_mark_exists(transient_id.as_str())
            .expect("missing processed mark"));
        assert!(store
            .mark_local_propagation_processed(transient_id.as_str())
            .expect("insert processed mark"));
        assert!(!store
            .mark_local_propagation_processed(transient_id.as_str())
            .expect("repeat processed mark"));
        assert!(store
            .local_propagation_processed_mark_exists(transient_id.as_str())
            .expect("processed mark exists"));
    }

    #[test]
    fn resolve_receipt_status_updates_non_terminal_message() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store.insert_message(&outbound_message("msg-1", 1, None)).expect("insert message");

        let resolved =
            store.resolve_receipt_status("msg-1", "sent: direct").expect("resolve status");

        assert_eq!(resolved.as_deref(), Some("sent: direct"));
        assert_eq!(
            store
                .get_message("msg-1")
                .expect("load message")
                .expect("message exists")
                .receipt_status
                .as_deref(),
            Some("sent: direct")
        );
    }

    #[test]
    fn update_message_fields_preserves_receipt_status() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store
            .insert_message(&outbound_message("msg-1", 1, Some("sending")))
            .expect("insert message");

        store
            .update_message_fields("msg-1", Some(&json!({"_lxmf": {"transient_id": "abcd"}})))
            .expect("update fields");

        let message = store.get_message("msg-1").expect("load message").expect("message exists");
        assert_eq!(message.receipt_status.as_deref(), Some("sending"));
        assert_eq!(message.fields.expect("fields")["_lxmf"]["transient_id"], json!("abcd"));
    }

    #[test]
    fn receipt_and_field_updates_run_on_writer_lane_in_order() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store.insert_message(&outbound_message("msg-1", 1, None)).expect("insert message");

        store.update_receipt_status("msg-1", "sending").expect("update status");
        store
            .update_message_fields("msg-1", Some(&json!({"_lxmf": {"stage": "queued"}})))
            .expect("update fields");

        let message = store.get_message("msg-1").expect("load message").expect("message exists");
        assert_eq!(message.receipt_status.as_deref(), Some("sending"));
        assert_eq!(message.fields.expect("fields")["_lxmf"]["stage"], json!("queued"));
    }

    #[test]
    fn resolve_receipt_status_preserves_terminal_status() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store
            .insert_message(&outbound_message("msg-1", 1, Some("delivered")))
            .expect("insert delivered message");

        let resolved =
            store.resolve_receipt_status("msg-1", "sent: direct").expect("resolve status");

        assert_eq!(resolved.as_deref(), Some("delivered"));
        assert_eq!(
            store
                .get_message("msg-1")
                .expect("load message")
                .expect("message exists")
                .receipt_status
                .as_deref(),
            Some("delivered")
        );
    }

    #[test]
    fn resolve_receipt_status_preserves_sent_over_sending_regression() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store
            .insert_message(&outbound_message("msg-1", 1, Some("sent: propagated resource")))
            .expect("insert sent message");

        let resolved = store
            .resolve_receipt_status("msg-1", "sending: propagated resource")
            .expect("resolve status");

        assert_eq!(resolved.as_deref(), Some("sent: propagated resource"));
        assert_eq!(
            store
                .get_message("msg-1")
                .expect("load message")
                .expect("message exists")
                .receipt_status
                .as_deref(),
            Some("sent: propagated resource")
        );
    }
