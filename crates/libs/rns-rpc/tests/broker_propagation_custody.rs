use rns_rpc::broker::AdmitRequest;
use rns_rpc::broker::PropagationCustody;
use rns_rpc::{BrokerCommand, MessagesStore};

fn admitted(store: &MessagesStore) -> String {
    let receipt = store
        .broker_command(BrokerCommand::Admit {
            owner: "owner".into(),
            source: "source".into(),
            request: AdmitRequest {
                identity: "identity".into(),
                operation_id: "custody-test".into(),
                destination: "recipient".into(),
                title: String::new(),
                content: "custody payload".into(),
                fields: None,
                options: Default::default(),
            },
        })
        .expect("admit operation");
    let id = receipt["message_id"].as_str().expect("message id").to_owned();
    store
        .broker_command(BrokerCommand::Prepared {
            message_id: id.clone(),
            payload: serde_json::json!({"lxmf_hex":"1234","propagation":null}),
        })
        .expect("prepare immutable bytes");
    store
        .broker_command(BrokerCommand::DispatchState {
            message_id: id.clone(),
            state: "scheduled".into(),
        })
        .expect("claim dispatch");
    id
}

#[test]
fn committed_propagation_custody_survives_restart_without_recipient_delivery_or_redispatch() {
    let directory = tempfile::tempdir().expect("temporary store");
    let path = directory.path().join("custody.db");
    let store = MessagesStore::open(&path).expect("open store");
    store.enable_durable_broker(32 * 1024 * 1024).expect("enable broker");
    let id = admitted(&store);
    store
        .complete_propagation_custody(&id, rns_rpc::broker::PropagationCustody::Resource)
        .expect("record custody");
    drop(store);
    let store = MessagesStore::open(&path).expect("reopen store");
    store.broker_recover_dispatch().expect("recover dispatch");
    assert_eq!(
        store.get_message(&id).unwrap().unwrap().receipt_status.as_deref(),
        Some("sent: propagated resource")
    );
    assert!(
        store.broker_pending_dispatch().expect("pending dispatch").is_empty(),
        "committed propagation custody must not upload again after restart"
    );
    store.update_receipt_status(&id, "delivered").expect("later recipient proof");
    assert_eq!(
        store.get_message(&id).unwrap().unwrap().receipt_status.as_deref(),
        Some("delivered")
    );
}

struct Fixture {
    _directory: tempfile::TempDir,
    store: MessagesStore,
    conn: rusqlite::Connection,
    id: String,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("temporary custody directory");
        let path = directory.path().join("custody.db");
        let store = MessagesStore::open(&path).expect("open custody store");
        store.enable_durable_broker(32 * 1024 * 1024).expect("enable custody broker");
        let id = admitted(&store);
        let conn = rusqlite::Connection::open(&path).expect("open custody inspection connection");
        Self { _directory: directory, store, conn, id }
    }

    fn snapshot(&self) -> (String, Option<String>, i64, i64, i64, i64) {
        self.conn.query_row("SELECT o.state,m.receipt_status,o.prepared_reserve,c.bytes,(SELECT used_bytes FROM broker_meta),(SELECT COUNT(*) FROM broker_events) FROM broker_operations o JOIN messages m ON m.id=o.message_id JOIN broker_completions c ON c.message_id=m.id WHERE m.id=?1", [&self.id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).expect("read custody accounting snapshot")
    }

    fn commit(&self, representation: PropagationCustody) -> rusqlite::Result<serde_json::Value> {
        self.store.complete_propagation_custody(&self.id, representation)
    }
}

fn actual_custody_event_cost(representation: PropagationCustody) -> i64 {
    let fixture = Fixture::new();
    fixture.commit(representation).expect("commit reference custody event");
    fixture
        .conn
        .query_row("SELECT bytes FROM broker_events WHERE event_type='receipt'", [], |r| r.get(0))
        .expect("read actual custody journal cost")
}

#[test]
fn custody_spends_exact_prepared_capacity_at_full_budget_and_keeps_recipient_terminal_capacity() {
    for representation in [PropagationCustody::Packet, PropagationCustody::Resource] {
        let cost = actual_custody_event_cost(representation);
        let f = Fixture::new();
        let terminal_reserve = f.snapshot().3;
        f.conn.execute("UPDATE broker_operations SET prepared_reserve=?1", [cost]).unwrap();
        // Inject logical-budget saturation; physical/WAL admission remains real.
        f.conn.execute("UPDATE broker_meta SET used_bytes=budget", []).unwrap();
        assert_eq!(f.commit(representation).unwrap()["committed"], true);
        let committed = f.snapshot();
        assert_eq!(committed.0, "custody");
        assert_eq!(committed.1.as_deref(), Some(representation.status()));
        assert_eq!(committed.2, 0);
        assert_eq!(committed.3, terminal_reserve);
        assert_eq!(committed.4, 32 * 1024 * 1024);
        assert_eq!(f.commit(representation).unwrap()["committed"], false);
        assert_eq!(f.snapshot(), committed, "duplicate custody must spend nothing");
        f.store.update_receipt_status(&f.id, "delivered").unwrap();
        assert_eq!(
            f.store.get_message(&f.id).unwrap().unwrap().receipt_status.as_deref(),
            Some("delivered")
        );
        assert_eq!(
            f.conn
                .query_row("SELECT COUNT(*) FROM broker_completions", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            f.conn
                .query_row(
                    "SELECT COUNT(*) FROM broker_events WHERE event_type='receipt'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            2
        );
    }
}

#[test]
fn insufficient_custody_capacity_and_journal_failure_leave_no_partial_commit() {
    let cost = actual_custody_event_cost(PropagationCustody::Resource);
    let f = Fixture::new();
    f.conn.execute("UPDATE broker_operations SET prepared_reserve=?1", [cost - 1]).unwrap();
    f.conn.execute("UPDATE broker_meta SET used_bytes=budget", []).unwrap();
    let before = f.snapshot();
    assert!(f
        .commit(PropagationCustody::Resource)
        .unwrap_err()
        .to_string()
        .contains("BROKER_FULL"));
    assert_eq!(f.snapshot(), before);
    assert!(
        f.store.broker_prepared(&f.id).unwrap_err().to_string().contains("BROKER_FULL"),
        "restored bytes cannot bypass custody capacity validation"
    );
    assert!(
        f.store
            .broker_command(BrokerCommand::Prepared {
                message_id: f.id.clone(),
                payload: serde_json::json!({"lxmf_hex":"1234","propagation":null}),
            })
            .unwrap_err()
            .to_string()
            .contains("BROKER_FULL"),
        "identical preparation must validate capacity"
    );
    assert_eq!(f.snapshot(), before);

    // Free ordinary capacity. A journal failure must roll back the reserve repair too.
    f.conn.execute("UPDATE broker_meta SET used_bytes=budget-1024", []).unwrap();
    f.conn.execute_batch("CREATE TRIGGER reject_custody_journal BEFORE INSERT ON broker_events WHEN NEW.event_type='receipt' BEGIN SELECT RAISE(ABORT, 'injected journal failure'); END;").unwrap();
    let before = f.snapshot();
    assert!(f
        .commit(PropagationCustody::Resource)
        .unwrap_err()
        .to_string()
        .contains("injected journal failure"));
    assert_eq!(f.snapshot(), before, "receipt, dispatch intent and budget must roll back together");
    f.conn.execute_batch("DROP TRIGGER reject_custody_journal;").unwrap();
    assert_eq!(f.commit(PropagationCustody::Resource).unwrap()["committed"], true);
}

#[test]
fn older_preparations_repair_only_the_shortfall_after_restart_and_capacity_recovery() {
    for identical_preparation in [false, true] {
        let mut f = Fixture::new();
        let prepared = f.store.broker_prepared(&f.id).unwrap().unwrap().unwrap();
        let cost = actual_custody_event_cost(PropagationCustody::Resource);
        f.conn.execute("UPDATE broker_operations SET prepared_reserve=?1", [cost - 1]).unwrap();
        f.conn.execute("UPDATE broker_meta SET used_bytes=budget", []).unwrap();
        let path = f._directory.path().join("custody.db");
        drop(f.store);
        f.store = MessagesStore::open(&path).unwrap();
        f.store.broker_recover_dispatch().unwrap();
        let before = f.snapshot();
        assert!(f.store.broker_prepared(&f.id).unwrap_err().to_string().contains("BROKER_FULL"));
        assert_eq!(f.snapshot(), before, "restored payload stays hidden while storage is full");
        f.conn.execute("UPDATE broker_meta SET used_bytes=budget-1024", []).unwrap();
        let before = f.snapshot();
        if identical_preparation {
            f.store
                .broker_command(BrokerCommand::Prepared {
                    message_id: f.id.clone(),
                    payload: prepared.clone(),
                })
                .unwrap();
            assert_eq!(f.snapshot().2, cost, "identical preparation must commit its repair");
        }
        assert_eq!(f.store.broker_prepared(&f.id).unwrap(), Some(Some(prepared.clone())));
        let after = f.snapshot();
        assert_eq!(after.0, "queued");
        assert_eq!(after.1, before.1);
        assert_eq!(after.2, cost);
        assert_eq!(after.3, before.3, "recipient-terminal capacity is untouched");
        assert_eq!(after.4, before.4 + 1, "charge only the one missing byte");
        assert_eq!(after.5, before.5);
        assert_eq!(f.store.broker_prepared(&f.id).unwrap(), Some(Some(prepared)));
        assert_eq!(f.snapshot(), after, "repeated reads cannot charge again");
    }
}

#[test]
fn ordinary_sent_status_does_not_establish_trusted_propagation_custody() {
    let f = Fixture::new();
    let before = f.snapshot();
    f.store.update_receipt_status(&f.id, PropagationCustody::Resource.status()).unwrap();
    let after = f.snapshot();
    assert_eq!(after.0, "scheduled");
    assert_eq!(after.2, before.2);
    assert_eq!(after.3, before.3);
    f.store.broker_recover_dispatch().unwrap();
    assert_eq!(f.store.broker_pending_dispatch().unwrap().len(), 1);
    assert_eq!(f.commit(PropagationCustody::Resource).unwrap()["committed"], true);
}

#[test]
fn fallback_preparation_cannot_spend_the_last_custody_byte() {
    let f = Fixture::new();
    let cost = actual_custody_event_cost(PropagationCustody::Resource);
    let old = f.store.broker_prepared(&f.id).unwrap().unwrap().unwrap();
    let available = serde_json::to_string(&old).unwrap().len() as i64 + f.snapshot().2;
    let mut fallback = old.clone();
    fallback["propagation"] = serde_json::json!({"bytes_hex":"aa","node":"node","padding":""});
    let base = serde_json::to_string(&fallback).unwrap().len();
    let padding = (available - cost + 1) as usize - base;
    fallback["propagation"]["padding"] = serde_json::json!("x".repeat(padding));
    let before = f.snapshot();
    assert!(f
        .store
        .broker_command(BrokerCommand::Prepared {
            message_id: f.id.clone(),
            payload: fallback.clone()
        })
        .unwrap_err()
        .to_string()
        .contains("custody capacity"));
    assert_eq!(f.snapshot(), before);
    assert_eq!(f.store.broker_prepared(&f.id).unwrap(), Some(Some(old)));
    fallback["propagation"]["padding"] = serde_json::json!("x".repeat(padding - 1));
    f.store
        .broker_command(BrokerCommand::Prepared {
            message_id: f.id.clone(),
            payload: fallback.clone(),
        })
        .unwrap();
    assert_eq!(f.snapshot().2, cost);
    assert_eq!(f.store.broker_prepared(&f.id).unwrap(), Some(Some(fallback)));
    assert_eq!(f.commit(PropagationCustody::Resource).unwrap()["committed"], true);
}

#[test]
fn recipient_terminal_wins_both_before_and_after_custody() {
    for terminal in ["cancelled", "delivered", "rejected", "failed: native transfer"] {
        for custody_first in [false, true] {
            let f = Fixture::new();
            if custody_first {
                f.commit(PropagationCustody::Resource).unwrap();
            }
            f.store.update_receipt_status(&f.id, terminal).unwrap();
            let events: i64 =
                f.conn.query_row("SELECT COUNT(*) FROM broker_events", [], |r| r.get(0)).unwrap();
            let used: i64 =
                f.conn.query_row("SELECT used_bytes FROM broker_meta", [], |r| r.get(0)).unwrap();
            assert_eq!(f.commit(PropagationCustody::Resource).unwrap()["committed"], false);
            assert_eq!(
                f.store.get_message(&f.id).unwrap().unwrap().receipt_status.as_deref(),
                Some(terminal)
            );
            assert_eq!(
                f.conn
                    .query_row("SELECT COUNT(*) FROM broker_events", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                events
            );
            assert_eq!(
                f.conn
                    .query_row("SELECT used_bytes FROM broker_meta", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                used
            );
            assert_eq!(
                f.conn
                    .query_row("SELECT state FROM broker_operations", [], |r| r.get::<_, String>(0))
                    .unwrap(),
                "terminal"
            );
        }
    }
}
