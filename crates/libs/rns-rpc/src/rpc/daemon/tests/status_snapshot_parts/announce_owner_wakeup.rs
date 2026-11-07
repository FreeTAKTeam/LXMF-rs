mod announce_owner_wakeup {
    use super::*;

    #[derive(Debug, PartialEq, Eq)]
    enum WakeEvent {
        Notify(String),
        Owns(String),
        Deliver(String),
    }

    struct OwnerBridge {
        events: Arc<Mutex<Vec<WakeEvent>>>,
        delivered: mpsc::Sender<String>,
    }

    impl OutboundBridge for OwnerBridge {
        fn notify_delivery_path(&self, destination: &str) {
            self.events.lock().expect("events").push(WakeEvent::Notify(destination.into()));
        }
        fn owns_delivery(&self, message_id: &str) -> bool {
            self.events.lock().expect("events").push(WakeEvent::Owns(message_id.into()));
            message_id.starts_with("owned-")
        }
        fn deliver(&self, record: &MessageRecord, _: &OutboundDeliveryOptions) -> Result<(), std::io::Error> {
            self.events.lock().expect("events").push(WakeEvent::Deliver(record.id.clone()));
            self.delivered.send(record.id.clone()).expect("observer");
            Ok(())
        }
    }

    fn announce(daemon: &RpcDaemon, destination: &str, aspect: &str) {
        let response = daemon.handle_rpc(rpc_request(1, "announce_received", json!({
            "peer": destination, "timestamp": 1_700_000_015i64, "aspect": aspect,
        }))).expect("announce RPC");
        assert!(response.error.is_none(), "{:?}", response.error);
    }

    #[test]
    fn propagation_notification_precedes_selected_node_filter() {
        for selected in [None, Some("different-node")] {
            let events = Arc::new(Mutex::new(Vec::new()));
            let (tx, rx) = mpsc::channel();
            let daemon = RpcDaemon::with_store_and_bridges(
                MessagesStore::in_memory().expect("store"), "owner-wake".into(),
                Some(Arc::new(OwnerBridge { events: events.clone(), delivered: tx })), None,
            );
            if let Some(selected) = selected {
                let response = daemon.handle_rpc(rpc_request(2, "set_outbound_propagation_node", json!({"peer": selected}))).expect("select");
                assert!(response.error.is_none());
            }
            announce(&daemon, "pinned-owner-node", "lxmf.propagation");
            assert_eq!(*events.lock().expect("events"), vec![WakeEvent::Notify("pinned-owner-node".into())]);
            assert!(rx.try_recv().is_err());
            daemon.shutdown_outbound_workers();
        }
    }

    #[test]
    fn owned_legacy_records_are_not_redispatched_or_mutated() {
        let store = MessagesStore::in_memory().expect("store");
        for (id, method) in [("owned-direct", "direct"), ("owned-opportunistic", "opportunistic"), ("unowned-control", "direct")] {
            store.insert_message(&MessageRecord {
                id: id.into(), source: "source".into(), destination: "owner-peer".into(),
                title: String::new(), content: "waiting".into(), timestamp: 1_700_000_000,
                direction: "out".into(), fields: Some(json!({"_lxmf": {"method": method}})),
                receipt_status: Some("queued: waiting for announce".into()),
            }).expect("record");
        }
        let events = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = mpsc::channel();
        let daemon = RpcDaemon::with_store_and_bridges(store, "owner-wake".into(),
            Some(Arc::new(OwnerBridge { events: events.clone(), delivered: tx })), None);
        announce(&daemon, "owner-peer", "lxmf.delivery");
        assert_eq!(rx.recv_timeout(std::time::Duration::from_secs(2)).expect("legacy control"), "unowned-control");
        announce(&daemon, "owner-peer", "lxmf.delivery");
        daemon.shutdown_outbound_workers();
        assert!(rx.try_recv().is_err(), "only the unowned control dispatches");
        let events = events.lock().expect("events");
        assert_eq!(events.first(), Some(&WakeEvent::Notify("owner-peer".into())));
        for id in ["owned-direct", "owned-opportunistic"] {
            assert!(events.contains(&WakeEvent::Owns(id.into())));
            assert!(!events.contains(&WakeEvent::Deliver(id.into())));
            assert_eq!(daemon.store.get_message(id).expect("lookup").expect("record").receipt_status.as_deref(), Some("queued: waiting for announce"));
        }
        assert_eq!(events.iter().filter(|event| matches!(event, WakeEvent::Notify(_))).count(), 2);
    }

    #[test]
    fn durable_filter_retains_prepared_payload_and_still_notifies_owner() {
        use crate::storage::broker::BrokerCommand;
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("owner.db");
        let store = MessagesStore::open(&path).expect("store");
        store.enable_durable_broker(32 * 1024 * 1024).expect("broker");
        let events = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = mpsc::channel();
        let daemon = RpcDaemon::with_store_and_bridges(store, "owner-wake".into(),
            Some(Arc::new(OwnerBridge { events: events.clone(), delivered: tx })), None);
        // Freeze the dispatch pump so this isolates announce behavior.
        daemon.shutdown_outbound_workers();
        let admitted = daemon.store.broker_command(BrokerCommand::Admit {
            owner: "test".into(), source: "source".into(), request: crate::broker::AdmitRequest {
                identity: "local".into(), operation_id: "owner-announce".into(),
                destination: "durable-peer".into(), title: String::new(), content: "hello".into(),
                fields: None, options: Default::default(),
            },
        }).expect("admit");
        let id = admitted["message_id"].as_str().expect("id");
        daemon.store.broker_command(BrokerCommand::DispatchState { message_id: id.into(), state: "scheduled".into() }).expect("claim");
        daemon.persist_durable_prepared_payload(id, json!({"lxmf_hex": "010203", "propagation": null})).expect("prepared");
        let before = daemon.durable_prepared_payload(id).expect("prepared lookup");
        announce(&daemon, "durable-peer", "lxmf.delivery");
        assert_eq!(*events.lock().expect("events"), vec![WakeEvent::Notify("durable-peer".into()), WakeEvent::Owns(id.into())]);
        assert!(rx.try_recv().is_err());
        assert_eq!(daemon.durable_prepared_payload(id).expect("prepared lookup"), before);
        let connection = rusqlite::Connection::open(path).expect("connection");
        let state: String = connection.query_row("SELECT state FROM broker_operations WHERE message_id=?1", [id], |row| row.get(0)).expect("state");
        assert_eq!(state, "scheduled");
    }
}
