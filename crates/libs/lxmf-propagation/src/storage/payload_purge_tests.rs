use super::entries::DELETE_UNHANDLED_PAYLOAD_MARKS;
use super::*;
use rusqlite::{Connection, StatementStatus};

const PAYLOAD_INDEX: &str = "idx_propagation_peer_entries_unhandled_payload";

fn seed(conn: &Connection, unrelated: usize) {
    conn.execute_batch(&format!(
        "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<{unrelated})
         INSERT INTO propagation_peer_entries
         SELECT printf('peer-%03d', x%100), printf('unrelated-%08d', x), 'unhandled', x FROM n;
         INSERT INTO propagation_entries VALUES
             ('aa', 'ff', '00', 1, 1, NULL), ('bb', 'ff', '00', 2, 1, NULL);
         INSERT INTO propagation_peer_entries VALUES
             ('peer', 'aa', 'unhandled', 1), ('PEER', 'aa', 'unhandled', 2),
             ('other', 'aa', 'unhandled', 3),
             ('completed-1', 'aa', 'handled', 1), ('completed-2', 'aa', 'received', 1),
             ('completed-3', 'aa', 'transferred', 1),
             ('completed-4', 'aa', 'transfer_limited', 1),
             ('unknown', 'aa', 'future-state', 1), ('peer', 'bb', 'unhandled', 1);"
    ))
    .expect("seed pending, completed, unknown and unrelated marks");
}

fn marks(conn: &Connection) -> Vec<(String, String, String, i64)> {
    conn.prepare("SELECT peer, transient_id, state, updated_at FROM propagation_peer_entries ORDER BY peer, transient_id")
        .expect("prepare marks")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
        .expect("read marks")
        .collect::<rusqlite::Result<_>>()
        .expect("collect marks")
}

fn measure_delete(conn: &Connection) -> i32 {
    conn.execute_batch("SAVEPOINT measure").expect("savepoint");
    let mut statement = conn.prepare(DELETE_UNHANDLED_PAYLOAD_MARKS).expect("production delete");
    assert_eq!(statement.execute(["aa"]).expect("delete pending marks"), 3);
    let steps = statement.get_status(StatementStatus::VmStep);
    assert_eq!(statement.get_status(StatementStatus::Sort), 0);
    drop(statement);
    conn.execute_batch("ROLLBACK TO measure; RELEASE measure").expect("restore fixture");
    steps
}

#[test]
fn payload_mark_purge_cost_is_independent_of_unrelated_pending_history() {
    let mut costs = Vec::new();
    for unrelated in [1_000, 100_000] {
        let conn = Connection::open_in_memory().expect("database");
        initialize(&conn).expect("schema");
        seed(&conn, unrelated);
        let indexed_steps = measure_delete(&conn);
        conn.execute_batch(&format!("DROP INDEX IF EXISTS {PAYLOAD_INDEX}"))
            .expect("legacy schema");
        let legacy_steps = measure_delete(&conn);
        eprintln!(
            "unrelated={unrelated} legacy_vm_steps={legacy_steps} indexed_vm_steps={indexed_steps}"
        );
        assert!(indexed_steps < 500, "one payload purge scanned history: {indexed_steps}");
        assert!(legacy_steps > indexed_steps * 10, "negative control must reproduce the scan");
        costs.push(indexed_steps);
    }
    assert_eq!(costs[0], costs[1], "unrelated history must not add VM work");
}

#[test]
fn payload_purge_index_migrates_on_reopen_and_preserves_haves_and_rollback() {
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("propagation.db");
    let conn = Connection::open(&path).expect("database");
    initialize(&conn).expect("schema");
    conn.execute_batch(&format!("DROP INDEX IF EXISTS {PAYLOAD_INDEX}")).expect("legacy schema");
    seed(&conn, 100);
    let before = marks(&conn);
    drop(conn);
    let conn = Connection::open(&path).expect("reopen");
    initialize(&conn).expect("upgrade");
    initialize(&conn).expect("idempotent initialization");
    let plan: String = conn
        .query_row(&format!("EXPLAIN QUERY PLAN {DELETE_UNHANDLED_PAYLOAD_MARKS}"), ["aa"], |row| {
            row.get(3)
        })
        .expect("production query plan");
    assert!(plan.contains(PAYLOAD_INDEX), "{plan}");
    assert!(plan.contains("transient_id=?"), "{plan}");
    assert_eq!(marks(&conn), before, "index migration must not mutate marks");
    assert_eq!(
        purge_propagation_entries_for_destination(&conn, "wrong", &["aa".into()])
            .expect("wrong recipient"),
        0
    );
    assert_eq!(
        purge_propagation_entries_for_destination(&conn, "ff", &["missing".into()])
            .expect("absent payload"),
        0
    );
    assert_eq!(marks(&conn), before);

    let expected: Vec<_> = before
        .iter()
        .filter(|(_, id, state, _)| id != "aa" || state != "unhandled")
        .cloned()
        .collect();
    let tx = conn.unchecked_transaction().expect("transaction");
    assert_eq!(
        purge_propagation_entries_for_destination(&tx, " FF ", &[" AA ".into(), "aa".into()])
            .expect("haves purge"),
        1
    );
    assert_eq!(marks(&tx), expected, "preserve completed history and unrelated marks");
    tx.rollback().expect("rollback");
    assert_eq!(marks(&conn), before);
    assert!(get_propagation_entry(&conn, "aa").expect("restored payload").is_some());

    assert_eq!(
        purge_propagation_entries_for_destination(&conn, "ff", &["aa".into()])
            .expect("committed purge"),
        1
    );
    drop(conn);
    let conn = Connection::open(&path).expect("reopen committed purge");
    initialize(&conn).expect("schema");
    assert_eq!(marks(&conn), expected);
    assert!(get_propagation_entry(&conn, "aa").expect("purged payload").is_none());
    assert!(get_propagation_entry(&conn, "bb").expect("other payload").is_some());
}

#[test]
fn payload_retention_preserves_completed_marks_and_tracks_state_transitions() {
    let conn = Connection::open_in_memory().expect("database");
    initialize(&conn).expect("schema");
    seed(&conn, 100);
    conn.execute_batch(
        "UPDATE propagation_peer_entries SET state='handled' WHERE peer='other' AND transient_id='aa';
         UPDATE propagation_peer_entries SET state='unhandled' WHERE peer='completed-1' AND transient_id='aa';"
    ).expect("transition marks across partial index predicate");
    let before = marks(&conn);
    assert!(prune_propagation_entries_page(&conn, 2, &[]).expect("under limit").is_empty());
    assert_eq!(marks(&conn), before);
    assert_eq!(prune_propagation_entries_page(&conn, 1, &[]).expect("retention purge"), ["aa"]);
    let expected: Vec<_> =
        before.into_iter().filter(|(_, id, state, _)| id != "aa" || state != "unhandled").collect();
    assert_eq!(marks(&conn), expected);
    assert_eq!(
        propagation_entry_stats(&conn).expect("remaining payload"),
        PropagationEntryStats { entries: 1, bytes: 1 }
    );
}
