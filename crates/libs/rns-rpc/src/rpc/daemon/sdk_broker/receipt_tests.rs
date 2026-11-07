use super::*;
use crate::broker::{AdmitRequest, PropagationCustody};

struct Fixture {
    directory: tempfile::TempDir,
    daemon: RpcDaemon,
    conn: rusqlite::Connection,
    id: String,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("receipt fixture directory");
        let path = directory.path().join("receipts.db");
        let store = MessagesStore::open(&path).expect("receipt store");
        store.enable_durable_broker(32 * 1024 * 1024).expect("enable broker");
        let result = store
            .broker_command(BrokerCommand::Admit {
                owner: "owner".into(),
                source: "source".into(),
                request: AdmitRequest {
                    identity: "source".into(),
                    operation_id: "custody".into(),
                    destination: "recipient".into(),
                    title: String::new(),
                    content: "body".into(),
                    fields: Some(json!({"_lxmf":{"include_ticket":true}})),
                    options: Default::default(),
                },
            })
            .expect("admit receipt operation");
        let id = result["message_id"].as_str().expect("operation message id").to_owned();
        store
            .broker_command(BrokerCommand::DispatchState {
                message_id: id.clone(),
                state: "scheduled".into(),
            })
            .expect("schedule operation");
        let daemon = RpcDaemon::with_store(store, "custody".into());
        let conn = rusqlite::Connection::open(&path).expect("inspection connection");
        Self { directory, daemon, conn, id }
    }

    fn state(&self) -> String {
        self.conn
            .query_row("SELECT state FROM broker_operations WHERE message_id=?1", [&self.id], |r| {
                r.get(0)
            })
            .expect("dispatch state")
    }

    fn trace_count(&self) -> usize {
        self.daemon.delivery_traces.lock().expect("trace lock").get(&self.id).map_or(0, Vec::len)
    }
}

#[test]
fn trusted_custody_preserves_metadata_and_ticket_then_survives_restart() {
    for representation in [PropagationCustody::Packet, PropagationCustody::Resource] {
        let f = Fixture::new();
        let mut events = f.daemon.subscribe_events();
        let metadata = json!({"message_id":"forged","status":"delivered","packet_hash":"packet", "resource_hash":"resource", "peer":"node", "method":"propagated", "delivery_kind":"native", "bytes":123,"link_id":"link","stage":"custody"}).as_object().unwrap().clone();
        let result =
            f.daemon.record_propagation_custody(&f.id, representation, metadata.clone()).unwrap();
        assert_eq!(result["message_id"], f.id);
        assert_eq!(result["status"], representation.status());
        assert_eq!(result["updated"], true);
        for key in [
            "packet_hash",
            "resource_hash",
            "peer",
            "method",
            "delivery_kind",
            "bytes",
            "link_id",
            "stage",
        ] {
            assert_eq!(result[key], metadata[key]);
        }
        assert_eq!(events.try_recv().unwrap().payload, result);
        assert_eq!(f.state(), "custody");
        assert_eq!(f.trace_count(), 1);
        assert_eq!(f.daemon.store.get_ticket_last_delivery("recipient").unwrap(), None);
        let duplicate =
            f.daemon.record_propagation_custody(&f.id, representation, JsonMap::new()).unwrap();
        assert_eq!(duplicate["updated"], false);
        assert_eq!(f.trace_count(), 1);
        assert_eq!(
            f.conn
                .query_row(
                    "SELECT COUNT(*) FROM broker_events WHERE event_type='receipt'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        let path = f.directory.path().join("receipts.db");
        drop(f.daemon);
        let store = MessagesStore::open(&path).unwrap();
        store.broker_recover_dispatch().unwrap();
        assert!(store.broker_pending_dispatch().unwrap().is_empty());
        let daemon = RpcDaemon::with_store(store, "reopened".into());
        assert_eq!(
            daemon.record_network_receipt(&f.id, "delivered", JsonMap::new()).unwrap()["updated"],
            true
        );
        assert!(daemon.store.get_ticket_last_delivery("recipient").unwrap().is_some());
    }
}

#[test]
fn trusted_custody_failure_publishes_neither_trace_nor_success_event() {
    let f = Fixture::new();
    let mut events = f.daemon.subscribe_events();
    f.conn.execute_batch("CREATE TRIGGER fail_custody BEFORE INSERT ON broker_events WHEN NEW.event_type='receipt' BEGIN SELECT RAISE(ABORT,'injected custody failure'); END;").unwrap();
    assert!(f
        .daemon
        .record_propagation_custody(&f.id, PropagationCustody::Resource, JsonMap::new())
        .unwrap_err()
        .to_string()
        .contains("injected custody failure"));
    assert_eq!(f.trace_count(), 0);
    assert!(events.try_recv().is_err());
    assert_eq!(f.state(), "scheduled");
    assert_eq!(f.daemon.message_receipt_status(&f.id).unwrap(), None);
    f.conn.execute_batch("DROP TRIGGER fail_custody;").unwrap();
    assert_eq!(
        f.daemon
            .record_propagation_custody(&f.id, PropagationCustody::Resource, JsonMap::new())
            .unwrap()["updated"],
        true
    );
    assert_eq!(f.trace_count(), 1);
    assert_eq!(events.try_recv().unwrap().payload["updated"], true);
    assert!(events.try_recv().is_err());
}

#[test]
fn trusted_custody_preserves_recipient_terminal_in_both_orders() {
    for terminal in ["delivered", "cancelled", "failed: transfer", "rejected"] {
        for custody_first in [false, true] {
            let f = Fixture::new();
            if custody_first {
                f.daemon
                    .record_propagation_custody(&f.id, PropagationCustody::Resource, JsonMap::new())
                    .unwrap();
            }
            f.daemon.record_network_receipt(&f.id, terminal, JsonMap::new()).unwrap();
            let traces = f.trace_count();
            let result = f
                .daemon
                .record_propagation_custody(&f.id, PropagationCustody::Resource, JsonMap::new())
                .unwrap();
            assert_eq!(result["updated"], false);
            assert_eq!(result["status"], terminal);
            assert_eq!(f.state(), "terminal");
            assert_eq!(f.trace_count(), traces);
        }
    }
}

#[test]
fn trusted_custody_keeps_legacy_nonbroker_receipts_working() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .store
        .insert_message(&MessageRecord {
            id: "legacy".into(),
            source: "source".into(),
            destination: "recipient".into(),
            title: String::new(),
            content: "body".into(),
            timestamp: 1,
            direction: "out".into(),
            fields: None,
            receipt_status: None,
        })
        .unwrap();
    let result = daemon
        .record_propagation_custody("legacy", PropagationCustody::Packet, JsonMap::new())
        .unwrap();
    assert_eq!(result["updated"], true);
    assert_eq!(
        daemon.message_receipt_status("legacy").unwrap().as_deref(),
        Some("sent: propagated")
    );
}

#[test]
fn missing_message_cannot_acknowledge_trusted_custody() {
    let daemon = RpcDaemon::test_instance();
    let mut events = daemon.subscribe_events();
    let error = daemon
        .record_propagation_custody("missing", PropagationCustody::Resource, JsonMap::new())
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    assert!(events.try_recv().is_err());
    assert!(daemon.delivery_traces.lock().unwrap().is_empty());
    // The existing ordinary receipt contract still permits missing records.
    assert_eq!(
        daemon.record_network_receipt("missing", "delivered", JsonMap::new()).unwrap()["updated"],
        false
    );
}
