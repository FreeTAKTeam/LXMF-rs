use super::*;
use rusqlite::StatementStatus;

fn query_cost(store: &MessagesStore) -> (Vec<String>, i32) {
    store
        .with_read_conn(|conn| {
            let mut statement = conn.prepare(RETRYABLE_OUTBOUND_FOR_DESTINATION_SQL)?;
            let ids = statement
                .query_map(["target"], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<String>>>()?;
            Ok((ids, statement.get_status(StatementStatus::VmStep)))
        })
        .expect("query cost")
}

#[test]
fn retryable_outbound_query_cost_ignores_completed_history() {
    let mut measured = Vec::new();
    for completed in [128, 10_000] {
        let store = MessagesStore::in_memory().expect("store");
        store.with_write_conn(|conn| conn.execute_batch(&format!(
            "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<{completed})
             INSERT INTO messages(id,source,destination,title,content,timestamp,direction,receipt_status)
             SELECT printf('done-%08d',x),'source','target','title','body',x,'out','delivered' FROM n;"
        ))).expect("completed history");
        assert!(store
            .list_retryable_outbound_for_destination("target")
            .expect("actual query")
            .is_empty());
        let (ids, steps) = query_cost(&store);
        println!("completed_rows={completed} returned={} vm_steps={steps}", ids.len());
        assert!(ids.is_empty());
        measured.push((completed, steps));
    }
    assert!(
        measured.iter().all(|(_, steps)| *steps < 200),
        "empty retry query scans: {measured:?}"
    );
}

#[test]
fn retryable_outbound_query_preserves_status_case_direction_and_tie_order() {
    let store = MessagesStore::in_memory().expect("store");
    for (id, ts, destination, direction, status) in [
        ("sparse-z", 1, "TaRgEt", "out", None),
        ("sparse-a", 1, "target", "out", Some("  ")),
        ("custom", 2, "TARGET", "out", Some("retry-needed")),
        ("null-other", 0, "elsewhere", "out", None),
        ("inbound", 0, "target", "in", None),
        ("sent", 0, "target", "out", Some(" SENT: packet")),
        ("sending", 0, "target", "out", Some(" Sending: link")),
        ("failed", 0, "target", "out", Some(" FAILED: retry")),
        ("done", 0, "target", "out", Some(" DELIVERED ")),
        ("cancel", 0, "target", "out", Some("cancelled")),
        ("expire", 0, "target", "out", Some("expired")),
        ("reject", 0, "target", "out", Some("rejected")),
        ("tab-status", 3, "target", "out", Some("\tdelivered\t")),
        ("upper-direction", 0, "target", "OUT", None),
    ] {
        store
            .insert_message(&MessageRecord {
                id: id.into(),
                source: "source".into(),
                destination: destination.into(),
                title: "title".into(),
                content: "body".into(),
                timestamp: ts,
                direction: direction.into(),
                fields: None,
                receipt_status: status.map(str::to_owned),
            })
            .expect("insert message");
    }
    let expected = vec!["sparse-a", "sparse-z", "custom", "tab-status"];
    let actual = store.list_retryable_outbound_for_destination("target").expect("query");
    assert_eq!(actual.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), expected);
    assert_eq!(query_cost(&store).0, expected);
}

#[test]
fn retryable_index_migrates_and_status_transitions_update_membership() {
    let root = tempfile::tempdir().expect("root");
    let path = root.path().join("messages.db");
    {
        let store = MessagesStore::open(&path).expect("store");
        store.with_write_conn(|conn| conn.execute_batch(
            "DROP INDEX idx_messages_retryable_destination;
             INSERT INTO messages(id,source,destination,title,content,timestamp,direction,receipt_status)
             VALUES('transition','source','target','title','body',1,'out','delivered');"
        )).expect("legacy fixture");
    }
    let store = MessagesStore::open(&path).expect("migrate");
    assert!(query_cost(&store).0.is_empty());
    for (status, expected) in
        [(None, 1), (Some("sending"), 0), (Some("custom-retry"), 1), (Some("delivered"), 0)]
    {
        store
            .with_write_conn(|conn| {
                conn.execute(
                    "UPDATE messages SET receipt_status=?1 WHERE id='transition'",
                    [status],
                )
                .map(|_| ())
            })
            .expect("status transition");
        assert_eq!(query_cost(&store).0.len(), expected);
    }
    let plan = store
        .with_read_conn(|conn| {
            let mut statement = conn
                .prepare(&format!("EXPLAIN QUERY PLAN {RETRYABLE_OUTBOUND_FOR_DESTINATION_SQL}"))?;
            let rows = statement.query_map(["target"], |row| row.get::<_, String>(3))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })
        .expect("query plan")
        .join(" ");
    assert!(plan.contains("idx_messages_retryable_destination"), "{plan}");
    assert!(!plan.contains("TEMP B-TREE"), "{plan}");
}
