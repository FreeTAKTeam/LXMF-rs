use super::*;
pub use r3akt_event_broker::storage::{enabled, physical_admission};
/// Startup-only LXMF migration using the shared profile and the existing writer.
pub fn enable(conn: &mut Connection, budget: i64) -> rusqlite::Result<()> {
    r3akt_event_broker::storage::configure_profile(conn, budget)?;
    let budget = if enabled(conn)? {
        conn.query_row("SELECT budget FROM broker_meta", [], |r| r.get(0))?
    } else {
        budget
    };
    let tx = conn.unchecked_transaction()?;
    let fresh = r3akt_event_broker::storage::initialize_schema(&tx, budget)?;
    tx.execute_batch("        CREATE TABLE IF NOT EXISTS broker_operations (
        owner TEXT NOT NULL,source TEXT NOT NULL,operation_id TEXT NOT NULL,fingerprint TEXT NOT NULL,
        message_id TEXT UNIQUE NOT NULL,state TEXT NOT NULL,options TEXT NOT NULL,prepared TEXT,prepared_reserve INTEGER NOT NULL,next_attempt_at INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY(owner,source,operation_id));
        CREATE INDEX IF NOT EXISTS broker_dispatch_due ON broker_operations(state,next_attempt_at,message_id);
        DROP INDEX IF EXISTS broker_dispatch_pending;
        CREATE TABLE IF NOT EXISTS broker_completions (message_id TEXT PRIMARY KEY,bytes INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS broker_message_keys (message_id TEXT PRIMARY KEY, fingerprint TEXT NOT NULL);
        CREATE UNIQUE INDEX IF NOT EXISTS broker_consumer_owner_scope ON broker_consumers(owner,destination);
        CREATE TRIGGER IF NOT EXISTS broker_protect_pending_message BEFORE DELETE ON messages
        WHEN OLD.direction='out' AND (OLD.receipt_status IS NULL OR (LOWER(TRIM(OLD.receipt_status)) NOT LIKE 'failed%' AND LOWER(TRIM(OLD.receipt_status)) NOT IN ('delivered','cancelled','expired','rejected')))
        BEGIN SELECT RAISE(ABORT,'SDK_STORAGE_BROKER_REFERENCED: pending durable message cannot be deleted'); END;
")?;
    if fresh {
        // A migration snapshot does not re-execute historical commands.
        let mut stmt = tx.prepare("SELECT id,source,destination,title,content,timestamp,direction,fields,receipt_status FROM messages ORDER BY timestamp,id")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let record = record_from_row(row)?;
            let destination = local_destination(&record);
            let fingerprint = message_fingerprint(&record)?;
            charge(&tx, (record.id.len() + fingerprint.len() + 128) as i64)?;
            tx.execute(
                "INSERT INTO broker_message_keys VALUES(?1,?2)",
                params![record.id, fingerprint],
            )?;
            reserve_completion(&tx, &record)?;
            append(&tx, &destination, "bootstrap_message", &json!({"message": record}))?;
        }
    }
    tx.commit()
}
pub fn restore_profile(conn: &mut Connection) -> rusqlite::Result<()> {
    if enabled(conn)? {
        let budget: i64 = conn.query_row("SELECT budget FROM broker_meta", [], |r| r.get(0))?;
        enable(conn, budget)?;
    }
    Ok(())
}
