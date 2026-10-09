use rns_rpc::broker::*;
use rns_rpc::BrokerCommand;
use rns_rpc::{MessageRecord, MessagesStore};
use serde_json::Value;
use sha2::Digest;
fn message(id: &str, destination: &str) -> MessageRecord {
    MessageRecord {
        id: id.into(),
        source: "remote".into(),
        destination: destination.into(),
        title: "".into(),
        content: "Test1234".into(),
        timestamp: 17,
        direction: "in".into(),
        fields: None,
        receipt_status: None,
    }
}
fn resume(
    store: &MessagesStore,
    owner: &str,
    dest: &str,
    journal: Option<JournalId>,
    stored: u64,
) -> Result<Value, rusqlite::Error> {
    store.broker_command(BrokerCommand::Resume {
        owner: owner.into(),
        destination: dest.into(),
        request: ResumeRequest {
            consumer_id: ConsumerId("rch".into()),
            identity: "identity".into(),
            journal_id: journal,
            stored: EventPosition(stored),
        },
    })
}
fn fetch(store: &MessagesStore, count: usize, bytes: usize) -> BrokerBatch {
    serde_json::from_value(
        store
            .broker_command(BrokerCommand::Fetch {
                owner: "local".into(),
                destination: "rch".into(),
                request: FetchRequest {
                    consumer_id: ConsumerId("rch".into()),
                    identity: "identity".into(),
                    max_events: count,
                    max_bytes: bytes,
                },
            })
            .expect("durable fetch succeeds"),
    )
    .expect("durable fetch response is a batch")
}
fn ack(store: &MessagesStore, batch: &BrokerBatch) -> Result<Value, rusqlite::Error> {
    store.broker_command(BrokerCommand::Ack {
        owner: "local".into(),
        destination: "rch".into(),
        request: AckStoredRequest {
            consumer_id: batch.consumer_id.clone(),
            identity: "identity".into(),
            journal_id: batch.journal_id.clone(),
            end: batch.end,
            receipt: batch.receipt.clone(),
        },
    })
}
#[test]
fn restart_replays_issued_batch_and_old_ack_without_position_reuse() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("daemon.db");
    let store = MessagesStore::open(&path).unwrap();
    store.enable_durable_broker(32 * 1024 * 1024).unwrap();
    let cp: BrokerCheckpoint =
        serde_json::from_value(resume(&store, "local", "rch", None, 0).unwrap()).unwrap();
    for (id, dest) in [("one", "rch"), ("hidden", "other"), ("two", "rch")] {
        store.insert_message(&message(id, dest)).unwrap();
    }
    let first = fetch(&store, 1, 4096);
    assert_eq!(first.events.len(), 1);
    assert_eq!(first.events[0].payload["message"]["id"], "one");
    drop(store);
    let store = MessagesStore::open(&path).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row("SELECT sqlite_version()", [], |r| r.get::<_, String>(0)).unwrap(),
        "3.53.2"
    );
    assert_eq!(fetch(&store, 128, MAX_BATCH_BYTES), first);
    ack(&store, &first).unwrap();
    let second = fetch(&store, 128, MAX_BATCH_BYTES);
    assert_eq!(second.start, first.end);
    assert_eq!(second.events.len(), 1);
    assert_eq!(second.end, EventPosition(3));
    ack(&store, &second).unwrap();
    assert_eq!(ack(&store, &first).unwrap()["stored"], 3);
    assert!(resume(&store, "local", "rch", Some(cp.journal_id.clone()), 1)
        .unwrap_err()
        .to_string()
        .contains("RECOVERY_REQUIRED"));
    assert!(resume(&store, "intruder", "rch", Some(cp.journal_id.clone()), 3)
        .unwrap_err()
        .to_string()
        .contains("FORBIDDEN"));
    assert!(resume(&store, "local", "other", Some(cp.journal_id.clone()), 3).is_err());
    store.insert_message(&message("three", "rch")).unwrap();
    let third = fetch(&store, 128, MAX_BATCH_BYTES);
    assert_eq!(third.journal_id, cp.journal_id);
    assert_eq!(third.end, EventPosition(4));
    let mut forged = third.clone();
    forged.end = EventPosition(99);
    assert!(ack(&store, &forged).is_err());
    assert_eq!(fetch(&store, 1, 4096), third);
}
#[test]
fn no_consumer_retains_and_message_and_event_rollback_together() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("daemon.db");
    let store = MessagesStore::open(&path).unwrap();
    store.enable_durable_broker(32 * 1024 * 1024).unwrap();
    store.insert_message(&message("one", "rch")).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM broker_events", [], |r| r.get::<_, i64>(0)).unwrap(),
        1
    );
    conn.execute("UPDATE broker_meta SET used_bytes=budget", []).unwrap();
    assert!(store
        .insert_message(&message("two", "rch"))
        .unwrap_err()
        .to_string()
        .contains("BROKER_FULL"));
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM messages WHERE id='two'", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row("SELECT next_position FROM broker_meta", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
}
#[test]
fn status_events_are_scoped_and_terminal_updates_do_not_duplicate() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("daemon.db");
    let store = MessagesStore::open(&path).unwrap();
    store.enable_durable_broker(32 * 1024 * 1024).unwrap();
    resume(&store, "local", "rch", None, 0).unwrap();
    let mut record = message("send", "remote");
    record.direction = "out".into();
    record.source = "rch".into();
    store.insert_message(&record).unwrap();
    store.update_receipt_status("send", "delivered").unwrap();
    store.update_receipt_status("send", "sending").unwrap();
    let batch = fetch(&store, 128, MAX_BATCH_BYTES);
    assert_eq!(batch.events.len(), 2);
    assert_eq!(batch.events[1].event_type, "receipt");
    assert_eq!(batch.events[1].payload["status"], "delivered");
}

#[test]
fn real_inbound_command_updates_cache_and_migration_detects_field_conflicts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("daemon.db");
    let store = MessagesStore::open(&path).unwrap();
    let mut record = message("migrated", "rch");
    record.fields = Some(serde_json::json!({"commands":["join"]}));
    store.insert_message(&record).unwrap();
    store.enable_durable_broker(32 * 1024 * 1024).unwrap();
    resume(&store, "local", "rch", None, 0).unwrap();
    record.fields = Some(serde_json::json!({"commands":["leave"]}));
    assert!(store
        .broker_command(BrokerCommand::Inbound { record, raw_hex: None })
        .unwrap_err()
        .to_string()
        .contains("CONFLICT"));
    let result = store
        .broker_command(BrokerCommand::Inbound {
            record: message("actual-ingress", "rch"),
            raw_hex: Some("0001".into()),
        })
        .unwrap();
    assert_eq!(result["inserted"], true);
    let conn = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get::<_, i64>(0)).unwrap(),
        2
    );
    let events = fetch(&store, 128, MAX_BATCH_BYTES);
    assert_eq!(events.events.len(), 2);
    assert_eq!(events.events[0].event_type, "bootstrap_message");
    assert_eq!(events.events[1].payload["lxmf_bytes_hex"], "0001");
}

#[test]
fn pending_messages_and_completion_capacity_survive_full_admission() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("daemon.db");
    let store = MessagesStore::open(&path).unwrap();
    store.enable_durable_broker(32 * 1024 * 1024).unwrap();
    resume(&store, "local", "rch", None, 0).unwrap();
    let mut record = message("pending", "remote");
    record.direction = "out".into();
    record.source = "rch".into();
    store.insert_message(&record).unwrap();
    assert!(store
        .delete_meshchat_message("pending")
        .unwrap_err()
        .to_string()
        .contains("REFERENCED"));
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute("UPDATE broker_meta SET used_bytes=budget", []).unwrap();
    store.update_receipt_status("pending", "delivered").unwrap();
    let events = fetch(&store, 128, MAX_BATCH_BYTES);
    assert_eq!(events.events.last().unwrap().payload["status"], "delivered");
    ack(&store, &events).unwrap();
    assert_eq!(store.delete_meshchat_message("pending").unwrap(), 1);
}

#[test]
fn operation_and_prepared_bytes_survive_lost_reply_and_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("daemon.db");
    let store = MessagesStore::open(&path).unwrap();
    store.enable_durable_broker(32 * 1024 * 1024).unwrap();
    let request = AdmitRequest {
        identity: "handle".into(),
        operation_id: "op1234".into(),
        destination: "remote".into(),
        title: "".into(),
        content: "Test1234".into(),
        fields: None,
        options: Default::default(),
    };
    let admit = |store: &MessagesStore, request: AdmitRequest| {
        store.broker_command(BrokerCommand::Admit {
            owner: "alice".into(),
            source: "rch".into(),
            request,
        })
    };
    assert_eq!(store.message_count().unwrap(), 0);
    let original = admit(&store, request.clone()).unwrap();
    assert_eq!(store.message_count().unwrap(), 1);
    assert_eq!(admit(&store, request.clone()).unwrap(), original);
    assert_eq!(store.message_count().unwrap(), 1);
    let id = original["message_id"].as_str().unwrap().to_string();
    let prepared = serde_json::json!({"lxmf_hex":"1234","propagation":null});
    store
        .broker_command(BrokerCommand::Prepared {
            message_id: id.clone(),
            payload: prepared.clone(),
        })
        .unwrap();
    assert!(store
        .broker_command(BrokerCommand::DispatchState {
            message_id: id.clone(),
            state: "scheduled".into()
        })
        .unwrap()["claimed"]
        .as_bool()
        .unwrap());
    drop(store);
    let store = MessagesStore::open(&path).unwrap();
    store.broker_recover_dispatch().unwrap();
    assert_eq!(admit(&store, request.clone()).unwrap()["message_id"], id);
    assert_eq!(
        store.broker_prepared(&id).unwrap(),
        Some(Some({
            let mut p = prepared.clone();
            p["wire_hash"] = serde_json::json!(hex::encode(sha2::Sha256::digest([0x12, 0x34])));
            p
        }))
    );
    assert_eq!(store.broker_pending_dispatch().unwrap()[0].0.id, id);
    let mut conflicting = request.clone();
    conflicting.content = "different".into();
    assert!(admit(&store, conflicting).unwrap_err().to_string().contains("OPERATION_CONFLICT"));
    let other = store
        .broker_command(BrokerCommand::Reconcile {
            owner: "mallory".into(),
            source: "rch".into(),
            request: ReconcileRequest {
                identity: "handle".into(),
                operation_id: request.operation_id,
            },
        })
        .unwrap();
    assert!(other.is_null());
    assert!(store
        .broker_command(BrokerCommand::Prepared {
            message_id: id.clone(),
            payload: serde_json::json!({"lxmf_hex":"changed","propagation":null})
        })
        .is_err());
    store.broker_command(BrokerCommand::Prepared {message_id:id.clone(),payload:serde_json::json!({"lxmf_hex":"1234","propagation":{"bytes_hex":"aa","node":"node"}})}).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get::<_, i64>(0)).unwrap(),
        1
    );
    store.update_receipt_status(&id, "delivered").unwrap();
    assert_eq!(
        store
            .broker_command(BrokerCommand::DispatchState {
                message_id: id,
                state: "scheduled".into()
            })
            .unwrap()["claimed"],
        false
    );
}

#[test]
fn crash_after_commit_child() {
    let Some(path) = std::env::var_os("LXMF_BROKER_CRASH_DB") else {
        return;
    };
    let store = MessagesStore::open(std::path::Path::new(&path)).unwrap();
    store.enable_durable_broker(32 * 1024 * 1024).unwrap();
    let request = AdmitRequest {
        identity: "handle".into(),
        operation_id: "crash-op".into(),
        destination: "remote".into(),
        title: "".into(),
        content: "Test1234".into(),
        fields: None,
        options: Default::default(),
    };
    let receipt = store
        .broker_command(BrokerCommand::Admit {
            owner: "alice".into(),
            source: "rch".into(),
            request,
        })
        .unwrap();
    store
        .broker_command(BrokerCommand::Prepared {
            message_id: receipt["message_id"].as_str().unwrap().into(),
            payload: serde_json::json!({"lxmf_hex":"1234","propagation":null}),
        })
        .unwrap();
    std::fs::write(
        std::path::PathBuf::from(&path).with_extension("ready"),
        serde_json::to_vec(&receipt).unwrap(),
    )
    .unwrap();
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
#[test]
fn real_kill_after_commit_recovers_original_admission_and_prepared_wire() {
    use std::time::{Duration, Instant};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("daemon.db");
    let ready = path.with_extension("ready");
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "crash_after_commit_child", "--nocapture"])
        .env("LXMF_BROKER_CRASH_DB", &path)
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while !ready.exists() {
        assert!(child.try_wait().unwrap().is_none());
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let original: Value = serde_json::from_slice(&std::fs::read(&ready).unwrap()).unwrap();
    child.kill().unwrap();
    assert!(!child.wait().unwrap().success());
    let store = MessagesStore::open(&path).unwrap();
    let receipt = store
        .broker_command(BrokerCommand::Reconcile {
            owner: "alice".into(),
            source: "rch".into(),
            request: ReconcileRequest {
                identity: "handle".into(),
                operation_id: "crash-op".into(),
            },
        })
        .unwrap();
    assert_eq!(receipt, original);
    let prepared =
        store.broker_prepared(receipt["message_id"].as_str().unwrap()).unwrap().unwrap().unwrap();
    assert_eq!(prepared["lxmf_hex"], "1234");
    resume(&store, "local", "rch", None, 0).unwrap();
    let events = fetch(&store, 128, MAX_BATCH_BYTES);
    assert_eq!(events.events.len(), 1);
    assert_eq!(events.events[0].payload["message"]["content"], "Test1234");
}

#[test]
fn maintenance_preserves_pending_custody_and_journals_expiry_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("daemon.db");
    let store = MessagesStore::open(&path).unwrap();
    store.enable_durable_broker(32 * 1024 * 1024).unwrap();
    resume(&store, "local", "rch", None, 0).unwrap();
    let mut pending = message("pending", "remote");
    pending.direction = "out".into();
    pending.source = "rch".into();
    store.insert_message(&pending).unwrap();
    let ordinary = message("ordinary", "rch");
    store.insert_message(&ordinary).unwrap();
    assert!(store.prune_outbound_messages(10, "oldest").unwrap().is_empty());
    assert_eq!(store.prune_messages_to_limit_bytes(0).unwrap(), vec!["ordinary"]);
    assert_eq!(store.expire_outbound_messages_before(18).unwrap(), vec!["pending"]);
    let batch = fetch(&store, 128, MAX_BATCH_BYTES);
    assert_eq!(batch.events.len(), 3);
    assert_eq!(batch.events[2].event_type, "receipt");
    assert_eq!(batch.events[2].payload["status"], "expired");
    assert!(store.expire_outbound_messages_before(18).unwrap().is_empty());
    assert_eq!(store.prune_outbound_messages(10, "oldest").unwrap(), vec!["pending"]);
    // The independently retained journal still carries both deleted messages.
    assert_eq!(fetch(&store, 128, MAX_BATCH_BYTES), batch);
}

#[test]
fn persisted_budget_stays_coherent_and_wal_recovers_after_pinned_reader() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("daemon.db");
    let store = MessagesStore::open(&path).unwrap();
    store.enable_durable_broker(32 * 1024 * 1024).unwrap();
    resume(&store, "local", "rch", None, 0).unwrap();
    store.insert_message(&message("before", "rch")).unwrap();
    let batch = fetch(&store, 128, MAX_BATCH_BYTES);
    store.enable_durable_broker(512 * 1024 * 1024).unwrap();
    let writer = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        writer.query_row("SELECT budget FROM broker_meta", [], |r| r.get::<_, i64>(0)).unwrap(),
        32 * 1024 * 1024
    );
    let admitted = store
        .broker_command(BrokerCommand::Admit {
            owner: "local".into(),
            source: "rch".into(),
            request: AdmitRequest {
                identity: "handle".into(),
                operation_id: "prepared-pressure".into(),
                destination: "remote".into(),
                title: "".into(),
                content: "Test1234".into(),
                fields: None,
                options: Default::default(),
            },
        })
        .unwrap();
    let prepared = || BrokerCommand::Prepared {
        message_id: admitted["message_id"].as_str().unwrap().into(),
        payload: serde_json::json!({"lxmf_hex":"1234","propagation":null}),
    };
    store.broker_command(prepared()).unwrap();
    writer.execute_batch("PRAGMA wal_autocheckpoint=0;CREATE TABLE pressure(value BLOB);INSERT INTO pressure VALUES(zeroblob(8000000));").unwrap();
    let reader = rusqlite::Connection::open(&path).unwrap();
    reader.execute_batch("BEGIN;").unwrap();
    let _: i64 = reader.query_row("SELECT COUNT(*) FROM pressure", [], |r| r.get(0)).unwrap();
    for _ in 0..5 {
        writer.execute("UPDATE pressure SET value=randomblob(8000000)", []).unwrap();
    }
    assert!(store
        .insert_message(&message("blocked", "rch"))
        .unwrap_err()
        .to_string()
        .contains("BROKER_FULL"));
    // Identical prepared bytes remain a successful no-op under physical backpressure.
    assert_eq!(store.broker_command(prepared()).unwrap(), serde_json::json!({"prepared":true}));
    reader.execute_batch("ROLLBACK").unwrap();
    writer.execute("DELETE FROM pressure", []).unwrap();
    ack(&store, &batch).unwrap();
    store.insert_message(&message("after", "rch")).unwrap();
    assert!(fetch(&store, 128, MAX_BATCH_BYTES)
        .events
        .iter()
        .any(|event| event.payload["message"]["id"] == "after"));
    assert!(store.get_message("blocked").unwrap().is_none());
}

#[test]
fn declared_consistent_backup_restore_invalidates_incarnation_and_old_receipts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("daemon.db");
    let backup = dir.path().join("backup.db");
    let store = MessagesStore::open(&path).unwrap();
    store.enable_durable_broker(32 * 1024 * 1024).unwrap();
    resume(&store, "local", "rch", None, 0).unwrap();
    store.insert_message(&message("before-backup", "rch")).unwrap();
    let batch = fetch(&store, 128, MAX_BATCH_BYTES);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute("VACUUM INTO ?1", [backup.to_str().unwrap()]).unwrap();
    store.insert_message(&message("after-backup", "rch")).unwrap();
    ack(&store, &batch).unwrap();
    drop(store);
    drop(conn);
    let restored = MessagesStore::open(&backup).unwrap();
    restored.declare_broker_backup_restore().unwrap();
    assert!(resume(&restored, "local", "rch", Some(batch.journal_id.clone()), 1)
        .unwrap_err()
        .to_string()
        .contains("RECOVERY_REQUIRED"));
    assert!(ack(&restored, &batch).is_err());
    let conn = rusqlite::Connection::open(&backup).unwrap();
    let journal: String =
        conn.query_row("SELECT journal_id FROM broker_meta", [], |r| r.get(0)).unwrap();
    assert_ne!(journal, batch.journal_id.0);
    assert!(restored.get_message("before-backup").unwrap().is_some());
    assert!(restored.get_message("after-backup").unwrap().is_none());
}
#[test]
fn migration_serializes_arrivals_and_bootstrap_has_one_record_per_logical_input() {
    let dir = tempfile::tempdir().unwrap();
    let store = std::sync::Arc::new(MessagesStore::open(&dir.path().join("migration.db")).unwrap());
    store.insert_message(&message("before", "rch")).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let incoming = store.clone();
    let gate = barrier.clone();
    let arrivals = std::thread::spawn(move || {
        gate.wait();
        incoming.insert_message(&message("during", "rch")).unwrap();
    });
    barrier.wait();
    store.enable_durable_broker(32 * 1024 * 1024).unwrap();
    arrivals.join().unwrap();
    store.insert_message(&message("after", "rch")).unwrap();
    resume(&store, "local", "rch", None, 0).unwrap();
    let batch = fetch(&store, 128, MAX_BATCH_BYTES);
    let ids = batch
        .events
        .iter()
        .map(|event| event.payload["message"]["id"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids, std::collections::BTreeSet::from(["before", "during", "after"]));
    assert_eq!(batch.events.len(), 3);
    assert_eq!(
        batch
            .events
            .iter()
            .find(|event| event.payload["message"]["id"] == "before")
            .unwrap()
            .event_type,
        "bootstrap_message"
    );
    assert_eq!(
        batch
            .events
            .iter()
            .find(|event| event.payload["message"]["id"] == "after")
            .unwrap()
            .event_type,
        "inbound"
    );
}

#[test]
fn announce_projection_uses_same_timestamp_order_and_native_utf8_byte_limit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("announces.db");
    let store = MessagesStore::open(&path).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    for index in 0..40 {
        conn.execute("INSERT INTO announces(id,peer,timestamp,name_source,first_seen,seen_count) VALUES(?1,'peer',1,'source',1,1)",[format!("{index:03}")]).unwrap();
    }
    let records = store.broker_announce_projection().unwrap();
    assert_eq!(records.len(), 32);
    assert_eq!(records.first().unwrap().id, "039");
    assert_eq!(records.last().unwrap().id, "008");
    conn.execute("UPDATE announces SET name_source=?1 WHERE id='039'", ["é".repeat(70_000)])
        .unwrap();
    assert!(store
        .broker_announce_projection()
        .unwrap_err()
        .to_string()
        .contains("EVENT_TOO_LARGE"));
}
#[test]
fn shared_metadata_writes_backpressure_without_rejecting_idempotent_custody_reads() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("shared-pressure.db");
    let store = MessagesStore::open(&path).unwrap();
    let budget = 32 * 1024 * 1024;
    store.enable_durable_broker(budget).unwrap();
    resume(&store, "local", "rch", None, 0).unwrap();
    store.insert_message(&message("shared", "rch")).unwrap();
    let batch = fetch(&store, 128, MAX_BATCH_BYTES);
    let reader = rusqlite::Connection::open(&path).unwrap();
    reader.execute_batch("BEGIN;SELECT * FROM messages;").unwrap();
    let mut rejected = false;
    for sequence in 0..100 {
        let fields = serde_json::json!({"sequence":sequence,"metadata":sequence.to_string().repeat(500_000/sequence.to_string().len())});
        match store.update_message_fields("shared", Some(&fields)) {
            Ok(()) => {}
            Err(error) => {
                assert!(error.to_string().contains("BROKER_FULL"));
                rejected = true;
                break;
            }
        }
        let wal = std::fs::metadata(format!("{}-wal", path.display())).unwrap().len();
        assert!(std::fs::metadata(&path).unwrap().len() + wal <= budget as u64);
    }
    assert!(rejected);
    assert_eq!(fetch(&store, 128, MAX_BATCH_BYTES), batch);
    resume(&store, "local", "rch", Some(batch.journal_id.clone()), 0).unwrap();
    // Control headroom admits the stored ACK while ordinary metadata is full.
    ack(&store, &batch).unwrap();
    ack(&store, &batch).unwrap();
    reader.execute_batch("ROLLBACK").unwrap();
    store.update_message_fields("shared", None).unwrap();
}
