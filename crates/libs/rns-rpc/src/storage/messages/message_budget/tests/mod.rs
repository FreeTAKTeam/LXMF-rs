use super::super::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

fn record(id: &str, timestamp: i64) -> MessageRecord {
    MessageRecord {
        id: id.into(),
        source: "a".repeat(32),
        destination: "b".repeat(32),
        title: "fixture".into(),
        content: "x".repeat(1024),
        timestamp,
        direction: "in".into(),
        fields: None,
        receipt_status: Some("delivered".into()),
    }
}

// SQLite LENGTH(TEXT) counts characters, stopping at the first NUL. It is not
// String::len(). Optional JSON is measured after the normal SQL serialization.
fn text_length(text: &str) -> u64 {
    u64::try_from(text.chars().take_while(|ch| *ch != '\0').count()).expect("length")
}

fn logical_length(message: &MessageRecord) -> u64 {
    [
        &message.id,
        &message.source,
        &message.destination,
        &message.title,
        &message.content,
        &message.direction,
    ]
    .into_iter()
    .map(|text| text_length(text))
    .sum::<u64>()
        + message.fields.as_ref().map_or(0, |fields| {
            text_length(&serde_json::to_string(fields).expect("serialized fields"))
        })
        + message.receipt_status.as_deref().map_or(0, text_length)
}

// Count the real synchronous writer command, not a separately copied SUM query.
// This test-only safe hook observes the bundled SQLite VM on its owning writer.
fn prune_work(store: &MessagesStore, cap: u64) -> (rusqlite::Result<Vec<String>>, u64) {
    let work = Arc::new(AtomicU64::new(0));
    let observed = Arc::clone(&work);
    store
        .with_control_conn(|conn| {
            conn.progress_handler(
                1,
                Some(move || {
                    observed.fetch_add(1, Ordering::Relaxed);
                    false
                }),
            )
        })
        .expect("install writer progress hook");
    let result = store.prune_messages_to_limit_bytes(cap);
    store
        .with_control_conn(|conn| conn.progress_handler(0, None::<fn() -> bool>))
        .expect("remove writer progress hook");
    (result, work.load(Ordering::Relaxed))
}

#[test]
fn message_budget_under_cap_current_method_work_ignores_retained_population() {
    const CAP: u64 = 410_000_000;
    let mut costs = Vec::new();
    for count in [128_usize, 10_000] {
        let store = MessagesStore::in_memory().expect("generated in-memory fixture");
        for index in 0..count {
            let message =
                record(&format!("{index:032x}"), i64::try_from(index).expect("timestamp"));
            store.insert_message(&message).expect("generated message");
        }
        let before = store.message_storage_stats().expect("fixture total");
        assert_eq!(before.count, u64::try_from(count).expect("count"));
        assert!(before.bytes < CAP);
        let mut repeated = Vec::new();
        for _ in 0..2 {
            let (removed, progress) = prune_work(&store, CAP);
            assert!(removed.expect("under-cap prune").is_empty());
            repeated.push(progress);
        }
        assert_eq!(store.message_storage_stats().expect("unchanged total"), before);
        println!(
            "SQLite {} retained_messages={count} cap={CAP} writer_progress_callbacks={repeated:?}",
            rusqlite::version()
        );
        costs.push(repeated);
    }
    assert_eq!(
        costs[0], costs[1],
        "under-cap maintenance must not revisit all retained message payloads"
    );
}

#[test]
fn message_budget_exact_cap_and_mutations_preserve_text_length_semantics() {
    let store = MessagesStore::in_memory().expect("store");
    let mut first = record("a", 10);
    first.title = "é🦀".into();
    first.content = "α\0ignored suffix".into();
    first.fields = Some(JsonValue::Null);
    first.receipt_status = None;
    let second = record("b", 10);
    // Insert ties in reverse order: pruning must still choose ascending ID.
    store.insert_message(&second).expect("second");
    store.insert_message(&first).expect("first");
    let total = logical_length(&first) + logical_length(&second);
    assert_eq!(store.message_storage_stats().expect("total").bytes, total);
    assert!(store.prune_messages_to_limit_bytes(total).expect("exact cap").is_empty());

    first.fields = Some(serde_json::json!({"é": "🦀"}));
    store.update_message_fields(&first.id, first.fields.as_ref()).expect("field update");
    first.receipt_status = Some("sent\0ignored".into());
    store
        .update_receipt_status(&first.id, first.receipt_status.as_deref().expect("status"))
        .expect("status update");
    first.content = "replacement é🦀".into();
    store.insert_message(&first).expect("legacy same-ID replacement");
    let total = logical_length(&first) + logical_length(&second);
    assert_eq!(store.message_storage_stats().expect("updated total").bytes, total);
    assert!(store.prune_messages_to_limit_bytes(total).expect("new exact cap").is_empty());
    assert_eq!(store.prune_messages_to_limit_bytes(total - 1).expect("over cap"), ["a"]);
    assert_eq!(store.get_message("b").expect("remaining"), Some(second));

    store.clear_messages().expect("clear");
    assert_eq!(store.message_storage_stats().expect("empty total").bytes, 0);
    assert!(store.prune_messages_to_limit_bytes(0).expect("empty zero cap").is_empty());
}

#[test]
fn message_budget_blob_fields_use_sqlite_byte_length_without_json_decoding() {
    let store = MessagesStore::in_memory().expect("store");
    let mut message = record("blob-fields", 0);
    message.fields = None;
    store.insert_message(&message).expect("message");
    let blob = vec![0xf0_u8, 0x9f, 0xa6, 0x80];
    store
        .with_write_conn(|conn| {
            conn.execute("UPDATE messages SET fields=?1 WHERE id=?2", params![blob, message.id])?;
            Ok(())
        })
        .expect("legacy BLOB fields");
    let total = logical_length(&message) + 4;
    assert_eq!(store.message_storage_stats().expect("BLOB total").bytes, total);
    assert!(store.prune_messages_to_limit_bytes(total).expect("exact cap").is_empty());
    assert_eq!(
        store.prune_messages_to_limit_bytes(total - 1).expect("delete raw row"),
        [message.id]
    );
}

#[test]
fn message_budget_durable_cap_preserves_outstanding_outbound_and_256_row_page() {
    let dir = tempfile::tempdir().expect("owned fixture directory");
    let store = MessagesStore::open(&dir.path().join("messages.db")).expect("store");
    store.enable_durable_broker(64 * 1024 * 1024).expect("durable profile");
    let mut protected = record("protected-queued", 0);
    protected.direction = "out".into();
    protected.receipt_status = None;
    store.insert_message(&protected).expect("protected queued");
    let mut protected_sent = record("protected-sent", 1);
    protected_sent.direction = "out".into();
    protected_sent.receipt_status = Some("sent".into());
    store.insert_message(&protected_sent).expect("protected sent");
    for index in 0..300 {
        let mut message = record(&format!("eligible-{index:03}"), index + 2);
        message.direction = "out".into();
        message.receipt_status =
            Some(if index % 2 == 0 { " DELIVERED " } else { "failed: fixture" }.into());
        store.insert_message(&message).expect("eligible terminal");
    }
    let first = store.prune_messages_to_limit_bytes(0).expect("first page");
    assert_eq!(first, (0..256).map(|n| format!("eligible-{n:03}")).collect::<Vec<_>>());
    assert_eq!(store.prune_messages_to_limit_bytes(0).expect("second page").len(), 44);
    assert!(store.prune_messages_to_limit_bytes(0).expect("protected over cap").is_empty());
    assert_eq!(store.get_message(&protected.id).expect("queued retained"), Some(protected));
    assert_eq!(store.get_message(&protected_sent.id).expect("sent retained"), Some(protected_sent));
    assert!(store.message_storage_stats().expect("protected total").bytes > 0);
}

#[test]
fn message_budget_restart_and_preexisting_rows_preserve_cap_result() {
    let dir = tempfile::tempdir().expect("owned fixture directory");
    let path = dir.path().join("legacy.db");
    let connection = Connection::open(&path).expect("legacy fixture");
    connection
        .execute_batch(
            "CREATE TABLE messages(id TEXT PRIMARY KEY,source TEXT,destination TEXT,title TEXT,
          content TEXT,timestamp INTEGER,direction TEXT,fields TEXT,receipt_status TEXT);
         INSERT INTO messages VALUES('old','src','dst',NULL,'é🦀',0,'in',NULL,NULL);",
        )
        .expect("preexisting schema and row");
    drop(connection);
    let store = MessagesStore::open(&path).expect("open legacy schema");
    let legacy = store.get_message("old").expect("legacy read").expect("legacy row");
    assert!(legacy.title.is_empty(), "existing startup normalization remains authoritative");
    let added = record("new", 1);
    store.insert_message(&added).expect("new row");
    let cap = logical_length(&legacy) + logical_length(&added);
    assert!(store.prune_messages_to_limit_bytes(cap).expect("before restart").is_empty());
    drop(store);
    let reopened = MessagesStore::open(&path).expect("restart");
    assert_eq!(reopened.message_storage_stats().expect("reopened total").bytes, cap);
    assert!(reopened.prune_messages_to_limit_bytes(cap).expect("reopened exact cap").is_empty());
    assert_eq!(reopened.prune_messages_to_limit_bytes(cap - 1).expect("reopened prune"), ["old"]);
    assert_eq!(reopened.get_message("new").expect("new retained"), Some(added));
}

#[test]
fn message_budget_legacy_null_arithmetic_keeps_existing_error_boundary() {
    let dir = tempfile::tempdir().expect("owned fixture directory");
    let path = dir.path().join("nulls.db");
    let connection = Connection::open(&path).expect("legacy fixture");
    connection
        .execute_batch(
            "CREATE TABLE messages(id TEXT PRIMARY KEY,source TEXT,destination TEXT,title TEXT,
          content TEXT,timestamp INTEGER,direction TEXT,fields TEXT,receipt_status TEXT);
         INSERT INTO messages VALUES('null-content','src','dst','',NULL,0,'in',NULL,NULL);",
        )
        .expect("legacy nullable row");
    drop(connection);
    let store = MessagesStore::open(&path).expect("store");
    assert_eq!(store.message_storage_stats().expect("NULL arithmetic").bytes, 0);
    assert!(store.prune_messages_to_limit_bytes(0).expect("ignored NULL sum").is_empty());
    store.insert_message(&record("valid", 1)).expect("valid positive row");
    assert!(
        store.prune_messages_to_limit_bytes(0).is_err(),
        "NULL oldest-row size still rejects pruning"
    );
    let rows: i64 = store
        .with_read_conn(|conn| conn.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0)))
        .expect("remaining rows");
    assert_eq!(rows, 2, "size decoding fails before any delete");
}

#[test]
fn message_budget_delete_failure_preserves_existing_per_statement_boundary() {
    let store = MessagesStore::in_memory().expect("store");
    store.insert_message(&record("a", 0)).expect("first");
    store.insert_message(&record("b", 1)).expect("second");
    store
        .with_write_conn(|conn| {
            conn.execute_batch(
                "CREATE TRIGGER fixture_abort_second BEFORE DELETE ON messages WHEN OLD.id='b'
         BEGIN SELECT RAISE(ABORT,'fixture second delete failure'); END;",
            )
        })
        .expect("abort fixture");
    assert!(store.prune_messages_to_limit_bytes(0).is_err());
    assert!(store.get_message("a").expect("earlier delete committed").is_none());
    assert!(store.get_message("b").expect("failed delete rolled back").is_some());
    assert_eq!(
        store.message_storage_stats().expect("remaining byte sum").bytes,
        logical_length(&record("b", 1))
    );
}

fn assert_exact_cap_is_noop(store: &MessagesStore) -> u64 {
    let before = store.list_messages(100, None).expect("owned fixture rows");
    let total = before.iter().map(logical_length).sum();
    if let Some(bound) = cached_bound(store) {
        assert!(bound >= 0 && bound as u64 >= total, "known bound must remain conservative");
    }
    assert_eq!(store.message_storage_stats().expect("current total").bytes, total);
    assert!(store.prune_messages_to_limit_bytes(total).expect("exact current cap").is_empty());
    assert_eq!(store.list_messages(100, None).expect("unchanged rows"), before);
    total
}

fn cached_bound(store: &MessagesStore) -> Option<i64> {
    store
        .with_read_conn(|conn| {
            conn.query_row(
                "SELECT upper_bound FROM message_budget_bound WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .optional()
        })
        .expect("private bound")
        .flatten()
}

fn forbid_sum(store: &MessagesStore, deny: bool) {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    store.with_control_conn(|conn| {
        if deny {
            conn.authorizer(Some(|context: AuthContext<'_>| {
                if matches!(context.action, AuthAction::Function { function_name } if function_name.eq_ignore_ascii_case("sum")) {
                    Authorization::Deny
                } else { Authorization::Allow }
            }))
        } else {
            conn.authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        }
    }).expect("test-only SUM authorizer");
}

#[test]
fn message_budget_unknown_malformed_and_missing_state_require_fallback() {
    for value in ["NULL", "-1", "1.5", "'unknown'"] {
        let store = MessagesStore::in_memory().expect("store");
        store.insert_message(&record("a", 0)).expect("first");
        store.insert_message(&record("b", 1)).expect("second");
        let total = store.message_storage_stats().expect("actual total").bytes;
        store
            .with_control_conn(|conn| {
                // Fault injection into PRIVATE bookkeeping, not a supported mutation
                // API: none of these values may be coerced into a small safe bound.
                conn.pragma_update(None, "ignore_check_constraints", true)?;
                conn.execute_batch(&format!(
                    "UPDATE message_budget_bound SET upper_bound={value}"
                ))?;
                conn.pragma_update(None, "ignore_check_constraints", false)
            })
            .expect("unknown state fixture");
        assert_eq!(store.prune_messages_to_limit_bytes(total - 1).expect("fallback prune"), ["a"]);
        assert!(cached_bound(&store).is_none(), "successful DELETE invalidates the bound");
    }
    let store = MessagesStore::in_memory().expect("store");
    store.insert_message(&record("a", 0)).expect("first");
    store.insert_message(&record("b", 1)).expect("second");
    let total = store.message_storage_stats().expect("total").bytes;
    store
        .with_control_conn(|conn| conn.execute("DELETE FROM message_budget_bound", []))
        .expect("missing state");
    assert_eq!(
        store.prune_messages_to_limit_bytes(total - 1).expect("missing state fallback"),
        ["a"]
    );
}

#[test]
fn message_budget_overflow_becomes_unknown_and_transaction_rollback_restores_bound() {
    let store = MessagesStore::in_memory().expect("store");
    store.insert_message(&record("a", 0)).expect("first");
    let original = cached_bound(&store);
    store
        .with_write_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute("UPDATE message_budget_bound SET upper_bound=?1", [i64::MAX])?;
            raw_message_write(&tx, &record("rollback", 1), "INSERT", "")?;
            let unknown: Option<i64> =
                tx.query_row("SELECT upper_bound FROM message_budget_bound", [], |r| r.get(0))?;
            assert!(unknown.is_none(), "overflow must not wrap or turn into a smaller known value");
            tx.rollback()
        })
        .expect("rollback overflow");
    assert_eq!(cached_bound(&store), original);
    assert!(store.get_message("rollback").expect("rollback row").is_none());
    store
        .with_control_conn(|conn| {
            conn.execute("UPDATE message_budget_bound SET upper_bound=?1", [i64::MAX])
        })
        .expect("conservative high bound");
    store.insert_message(&record("b", 1)).expect("overflowing bound update");
    assert!(cached_bound(&store).is_none());
    assert_exact_cap_is_noop(&store);
}

#[test]
fn message_budget_intact_initialization_skips_sum_and_fallback_errors_propagate() {
    let store = MessagesStore::in_memory().expect("store");
    store.insert_message(&record("a", 0)).expect("row");
    let known = cached_bound(&store);
    forbid_sum(&store, true);
    store
        .with_write_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            super::initialize(&tx)?;
            tx.commit()
        })
        .expect("intact initialization must not SUM");
    assert_eq!(cached_bound(&store), known);
    assert!(store
        .prune_messages_to_limit_bytes(410_000_000)
        .expect("known bound skips SUM")
        .is_empty());
    store
        .with_control_conn(|conn| {
            conn.execute("UPDATE message_budget_bound SET upper_bound=NULL", [])
        })
        .expect("unknown");
    store
        .with_write_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            super::initialize(&tx)?;
            tx.commit()
        })
        .expect("intact unknown initialization must not SUM either");
    assert!(
        store.prune_messages_to_limit_bytes(410_000_000).is_err(),
        "fallback SUM error is not zero bytes"
    );
    assert!(cached_bound(&store).is_none());
    assert!(store.get_message("a").expect("error retained row").is_some());
    forbid_sum(&store, false);
    assert_exact_cap_is_noop(&store);
}

#[test]
fn message_budget_reopen_invalidates_missing_or_changed_trigger_installation() {
    for change in ["missing-trigger", "changed-trigger", "recreated-messages"] {
        let dir = tempfile::tempdir().expect("owned migration fixture");
        let path = dir.path().join("budget.db");
        let store = MessagesStore::open(&path).expect("store");
        let old = record("old", 0);
        let cap = logical_length(&old);
        store.insert_message(&old).expect("old row");
        assert_eq!(cached_bound(&store), Some(i64::try_from(cap).expect("cap")));
        drop(store);
        let conn = Connection::open(&path).expect("owned raw migration");
        match change {
            "missing-trigger" => conn.execute_batch("DROP TRIGGER message_budget_insert;").expect("missing trigger"),
            "changed-trigger" => conn.execute_batch(
                "DROP TRIGGER message_budget_insert;
                 CREATE TRIGGER message_budget_insert AFTER INSERT ON messages BEGIN SELECT 1; END;",
            ).expect("changed trigger"),
            _ => {
                conn.execute_batch(
                    "DROP TABLE messages;
                     CREATE TABLE messages(id TEXT PRIMARY KEY,source TEXT,destination TEXT,title TEXT,
                       content TEXT,timestamp INTEGER,direction TEXT,fields TEXT,receipt_status TEXT);",
                ).expect("recreated message table");
                raw_message_write(&conn, &old, "INSERT", "").expect("restored old row");
            }
        }
        raw_message_write(&conn, &record("new", 1), "INSERT", "").expect("unaccounted row");
        drop(conn);
        let reopened = MessagesStore::open(&path).expect("reopen repairs installation");
        assert!(
            cached_bound(&reopened).is_none(),
            "stale low bound must not survive changed installation"
        );
        assert_eq!(
            reopened.prune_messages_to_limit_bytes(cap).expect("required prune after repair"),
            ["old"]
        );
        assert!(reopened.get_message("new").expect("new row remains").is_some());
    }
}

fn fail_trigger_fixture(
    before_open: bool,
    recursive: bool,
    event: &str,
) -> (tempfile::TempDir, MessagesStore) {
    let directory = tempfile::tempdir().expect("owned late FAIL fixture");
    let path = directory.path().join("late-fail.db");
    let trigger = format!(
        "CREATE TRIGGER fixture_late_fail AFTER {event} ON messages
         WHEN NEW.id='late-fail'
         BEGIN SELECT RAISE(FAIL,'fixture late failure'); END;"
    );
    if before_open {
        let conn = Connection::open(&path).expect("legacy fixture");
        conn.execute_batch(
            "CREATE TABLE messages(id TEXT PRIMARY KEY,source TEXT,destination TEXT,title TEXT,
             content TEXT,timestamp INTEGER,direction TEXT,fields TEXT,receipt_status TEXT);",
        )
        .expect("old table");
        conn.execute_batch(&trigger).expect("older FAIL trigger");
    }
    let store = MessagesStore::open(&path).expect("store");
    store
        .with_control_conn(|conn| {
            conn.pragma_update(None, "recursive_triggers", recursive)?;
            if !before_open {
                conn.execute_batch(&trigger)?;
            }
            Ok(())
        })
        .expect("FAIL trigger order");
    (directory, store)
}

#[test]
fn message_budget_late_insert_fail_cannot_leave_a_known_low_bound() {
    for recursive in [false, true] {
        for before_open in [false, true] {
            let (_directory, store) = fail_trigger_fixture(before_open, recursive, "INSERT");
            let message = record("late-fail", 0);
            assert!(store.insert_message(&message).is_err());
            assert_eq!(store.get_message("late-fail").expect("FAIL keeps row"), Some(message));
            assert!(store.message_storage_stats().expect("actual stored length").bytes > 0);
            assert_eq!(
                store.prune_messages_to_limit_bytes(0).expect("original SUM requires prune"),
                ["late-fail"],
                "AFTER FAIL may not cause an unsafe under-cap shortcut"
            );
        }
    }
}

#[test]
fn message_budget_late_update_fail_cannot_hide_committed_growth() {
    for recursive in [false, true] {
        for before_open in [false, true] {
            let (_directory, store) =
                fail_trigger_fixture(before_open, recursive, "UPDATE OF fields");
            let message = record("late-fail", 0);
            let old_cap = logical_length(&message);
            store.insert_message(&message).expect("initial row");
            let grown_fields = serde_json::json!({"growth": "é🦀".repeat(1024)});
            assert!(store.update_message_fields("late-fail", Some(&grown_fields)).is_err());
            assert_eq!(
                store.get_message("late-fail").expect("FAIL keeps growth").expect("row").fields,
                Some(grown_fields)
            );
            assert!(store.message_storage_stats().expect("actual length").bytes > old_cap);
            assert_eq!(
                store.prune_messages_to_limit_bytes(old_cap).expect("original SUM requires prune"),
                ["late-fail"]
            );
        }
    }
}

#[test]
fn message_budget_nested_late_fail_keeps_all_persisted_growth_accounted() {
    for recursive in [false, true] {
        for before_open in [false, true] {
            for update in [false, true] {
                let dir = tempfile::tempdir().expect("owned nested FAIL fixture");
                let path = dir.path().join("nested-fail.db");
                let triggers = if update {
                    "CREATE TRIGGER fixture_outer AFTER UPDATE OF fields ON messages WHEN NEW.id='outer'
                     BEGIN UPDATE messages SET content=content||'nested growth' WHERE id='inner'; END;
                     CREATE TRIGGER fixture_inner AFTER UPDATE OF content ON messages WHEN NEW.id='inner'
                     BEGIN SELECT RAISE(FAIL,'nested update failure'); END;"
                } else {
                    "CREATE TRIGGER fixture_outer AFTER INSERT ON messages WHEN NEW.id='outer'
                     BEGIN INSERT INTO messages VALUES('inner',NEW.source,NEW.destination,'inner','nested',1,'in',NULL,'delivered'); END;
                     CREATE TRIGGER fixture_inner AFTER INSERT ON messages WHEN NEW.id='inner'
                     BEGIN SELECT RAISE(FAIL,'nested insert failure'); END;"
                };
                if before_open {
                    let conn = Connection::open(&path).expect("legacy fixture");
                    conn.execute_batch(
                        "CREATE TABLE messages(id TEXT PRIMARY KEY,source TEXT,destination TEXT,title TEXT,
                         content TEXT,timestamp INTEGER,direction TEXT,fields TEXT,receipt_status TEXT);",
                    ).expect("legacy table");
                    conn.execute_batch(triggers).expect("older nested triggers");
                }
                let store = MessagesStore::open(&path).expect("store");
                store
                    .with_control_conn(|conn| {
                        conn.pragma_update(None, "recursive_triggers", recursive)?;
                        if !before_open {
                            conn.execute_batch(triggers)?;
                        }
                        Ok(())
                    })
                    .expect("nested trigger configuration");
                if update {
                    let outer = record("outer", 0);
                    let inner = record("inner", 1);
                    let cap = logical_length(&outer) + logical_length(&inner);
                    store.insert_message(&outer).expect("outer seed");
                    store.insert_message(&inner).expect("inner seed");
                    assert!(store
                        .update_message_fields(
                            "outer",
                            Some(&serde_json::json!("growth".repeat(1024)))
                        )
                        .is_err());
                    assert!(store
                        .get_message("inner")
                        .expect("inner persisted")
                        .expect("row")
                        .content
                        .ends_with("nested growth"));
                    assert!(
                        store.message_storage_stats().expect("actual nested growth").bytes > cap
                    );
                    assert_eq!(
                        store.prune_messages_to_limit_bytes(cap).expect("required prune"),
                        ["outer"]
                    );
                } else {
                    assert!(store.insert_message(&record("outer", 0)).is_err());
                    assert!(store.get_message("outer").expect("outer persisted").is_some());
                    assert!(store.get_message("inner").expect("inner persisted").is_some());
                    assert_eq!(
                        store.prune_messages_to_limit_bytes(0).expect("required nested prune"),
                        ["outer", "inner"]
                    );
                }
            }
        }
    }
}

#[test]
fn message_budget_before_fail_and_after_delete_fail_keep_safe_overestimates() {
    let store = MessagesStore::in_memory().expect("store");
    store
        .with_control_conn(|conn| {
            conn.execute_batch(
        "CREATE TRIGGER fixture_before_fail BEFORE INSERT ON messages WHEN NEW.id='before-fail'
         BEGIN SELECT RAISE(FAIL,'before insertion'); END;",
    )
        })
        .expect("before failure");
    assert!(store.insert_message(&record("before-fail", 0)).is_err());
    assert!(store.get_message("before-fail").expect("no insert").is_none());
    assert!(store
        .prune_messages_to_limit_bytes(0)
        .expect("overcredit reconciles to zero")
        .is_empty());
    store.insert_message(&record("delete-fail", 0)).expect("row");
    store
        .with_control_conn(|conn| {
            conn.execute_batch(
        "CREATE TRIGGER fixture_delete_fail AFTER DELETE ON messages WHEN OLD.id='delete-fail'
         BEGIN SELECT RAISE(FAIL,'after deletion'); END;",
    )
        })
        .expect("after delete failure");
    assert!(store.prune_messages_to_limit_bytes(0).is_err());
    assert!(store.get_message("delete-fail").expect("delete persisted").is_none());
    assert!(store.prune_messages_to_limit_bytes(0).expect("remaining bound reconciles").is_empty());
}

#[test]
fn message_budget_near_cap_unchanged_and_small_status_updates_keep_bounded_work() {
    let mut populations = Vec::new();
    for count in [128_usize, 10_000] {
        let store = MessagesStore::in_memory().expect("owned near-cap fixture");
        for index in 0..count {
            store
                .insert_message(&record(
                    &format!("{index:032x}"),
                    i64::try_from(index).expect("timestamp"),
                ))
                .expect("generated row");
        }
        let cap = store.message_storage_stats().expect("exact initial total").bytes + 64;
        let (removed, fast) = prune_work(&store, cap);
        assert!(removed.expect("initial prune").is_empty());
        let mut work = Vec::new();
        for status in [
            "delivered",
            "delivered",
            "sent",
            "sending",
            "sent",
            "sending",
            "sent",
            "sending",
            "delivered",
        ] {
            store
                .update_receipt_status(&format!("{:032x}", 0), status)
                .expect("real receipt update");
            let (removed, callbacks) = prune_work(&store, cap);
            assert!(removed.expect("still under cap").is_empty());
            work.push(callbacks);
        }
        println!("SQLite {} near_cap_messages={count} cap={cap} initial_callbacks={fast} update_prune_callbacks={work:?}",rusqlite::version());
        populations.push((fast, work));
    }
    for (fast, work) in &populations {
        assert!(
            work.iter().all(|value| value == fast),
            "unchanged/tiny updates must not trigger repeated SUM reconciliation"
        );
    }
    assert_eq!(populations[0], populations[1]);
}

#[test]
fn message_budget_near_cap_same_length_updates_do_not_need_sum() {
    let store = MessagesStore::in_memory().expect("store");
    let message = record("near-cap", 0);
    store.insert_message(&message).expect("row");
    let cap = logical_length(&message);
    assert!(store.prune_messages_to_limit_bytes(cap).expect("exact baseline").is_empty());
    forbid_sum(&store, true);
    for status in ["delivered", "scheduled", "delivered", "scheduled"] {
        store.update_receipt_status(&message.id, status).expect("same-length status update");
        let removed = store.prune_messages_to_limit_bytes(cap);
        if removed.is_err() {
            forbid_sum(&store, false);
        }
        assert!(removed.expect("same-length update must not need a SUM").is_empty());
    }
    forbid_sum(&store, false);
}

fn raw_message_write(
    conn: &Connection,
    message: &MessageRecord,
    verb: &str,
    conflict: &str,
) -> rusqlite::Result<usize> {
    let fields =
        message.fields.as_ref().map(|value| serde_json::to_string(value).expect("fixture fields"));
    conn.execute(
        &format!(
            "{verb} INTO messages(id,source,destination,title,content,timestamp,direction,fields,receipt_status)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) {conflict}"
        ),
        params![message.id, message.source, message.destination, message.title,
            message.content, message.timestamp, message.direction, fields, message.receipt_status],
    )
}

#[test]
fn message_budget_replace_ignore_upsert_and_rollback_preserve_required_pruning() {
    for recursive in [false, true] {
        let store = MessagesStore::in_memory().expect("store");
        store
            .with_control_conn(|conn| conn.pragma_update(None, "recursive_triggers", recursive))
            .expect("recursive trigger policy");
        let mut first = record("a", 0);
        let second = record("b", 1);
        store.insert_message(&first).expect("first");
        store.insert_message(&second).expect("second");
        assert_exact_cap_is_noop(&store);

        // REPLACE can omit the conflicting row's DELETE triggers when recursion
        // is off. Growing and shrinking it must both preserve the original cap.
        for content in ["small".to_string(), "replacement".repeat(400)] {
            first.content = content;
            store
                .with_write_conn(|conn| raw_message_write(conn, &first, "INSERT OR REPLACE", ""))
                .expect("insert or replace");
            assert_eq!(store.get_message("a").expect("replacement"), Some(first.clone()));
            assert_exact_cap_is_noop(&store);
        }
        store
            .with_write_conn(|conn| {
                conn.execute(
            "UPDATE OR REPLACE messages SET id='b',content='collision',timestamp=0 WHERE id='a'",
            [],
        )
            })
            .expect("update or replace conflicting ID");
        assert!(store.get_message("a").expect("old key").is_none());
        let replacement = store.get_message("b").expect("new key").expect("row");
        assert_exact_cap_is_noop(&store);

        let mut ignored = record("b", 9);
        ignored.content = "ignored large payload".repeat(500);
        assert_eq!(
            store
                .with_write_conn(|conn| raw_message_write(conn, &ignored, "INSERT OR IGNORE", ""))
                .expect("ignored insert"),
            0
        );
        assert_eq!(store.get_message("b").expect("ignore preserved row"), Some(replacement));
        assert_exact_cap_is_noop(&store);

        let mut updated = record("b", 0);
        updated.content = "upsert é🦀".repeat(100);
        updated.fields = Some(serde_json::json!({"mode": "upsert"}));
        updated.receipt_status = None;
        store.with_write_conn(|conn| raw_message_write(conn, &updated, "INSERT",
            "ON CONFLICT(id) DO UPDATE SET content=excluded.content,fields=excluded.fields,receipt_status=excluded.receipt_status"))
            .expect("upsert");
        let before_rollback = store.list_messages(100, None).expect("before rollback");
        assert_exact_cap_is_noop(&store);
        store
            .with_write_conn(|conn| {
                let tx = conn.unchecked_transaction()?;
                raw_message_write(&tx, &ignored, "INSERT OR REPLACE", "")?;
                tx.rollback()
            })
            .expect("rolled-back replacement");
        assert_eq!(store.list_messages(100, None).expect("after rollback"), before_rollback);
        let total = assert_exact_cap_is_noop(&store);
        assert_eq!(store.prune_messages_to_limit_bytes(total - 1).expect("required prune"), ["b"]);
    }
}

fn fixture_mutation_triggers(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TRIGGER fixture_insert_shrink AFTER INSERT ON messages WHEN NEW.id='triggered'
         BEGIN UPDATE messages SET content='é' WHERE id=NEW.id; END;
         CREATE TRIGGER fixture_update_side AFTER UPDATE OF content ON messages
         WHEN NEW.id='triggered' AND NEW.content<>OLD.content
         BEGIN INSERT OR REPLACE INTO messages
           VALUES('side',NEW.source,NEW.destination,'side','side:'||NEW.content,NEW.timestamp+1,'in',NULL,'delivered'); END;",
    )
}

#[test]
fn message_budget_trigger_order_and_recursive_policies_cannot_skip_required_pruning() {
    for recursive in [false, true] {
        for triggers_before_open in [false, true] {
            let dir = tempfile::tempdir().expect("owned trigger fixture");
            let path = dir.path().join("trigger-order.db");
            if triggers_before_open {
                let conn = Connection::open(&path).expect("legacy fixture");
                conn.execute_batch(
                    "CREATE TABLE messages(id TEXT PRIMARY KEY,source TEXT,destination TEXT,title TEXT,
                     content TEXT,timestamp INTEGER,direction TEXT,fields TEXT,receipt_status TEXT);",
                ).expect("preexisting table");
                fixture_mutation_triggers(&conn).expect("preexisting triggers");
            }
            let store = MessagesStore::open(&path).expect("store");
            store
                .with_control_conn(|conn| {
                    conn.pragma_update(None, "recursive_triggers", recursive)?;
                    if !triggers_before_open {
                        fixture_mutation_triggers(conn)?;
                    }
                    Ok(())
                })
                .expect("trigger setup");
            let mut triggered = record("triggered", 0);
            store.insert_message(&triggered).expect("insert with nested shrink/update");
            assert_eq!(
                store.get_message("triggered").expect("shrunk row").expect("row").content,
                "é"
            );
            assert!(store.get_message("side").expect("nested row").is_some());
            assert_exact_cap_is_noop(&store);
            // The second mutation replaces the existing side row; recursion may
            // suppress its DELETE trigger but cannot permit an unsafe low total.
            triggered.content = "updated payload é🦀".repeat(100);
            store.insert_message(&triggered).expect("nested replacement mutation");
            let total = assert_exact_cap_is_noop(&store);
            assert_eq!(
                store.prune_messages_to_limit_bytes(total - 1).expect("required prune"),
                ["triggered"]
            );
        }
    }
}
