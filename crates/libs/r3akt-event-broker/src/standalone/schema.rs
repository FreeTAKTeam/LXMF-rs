use super::*;
use rusqlite::{params, OptionalExtension};
pub(super) fn initialize(conn: &Connection, config: Config) -> rusqlite::Result<()> {
    storage::initialize_schema(conn, config.budget_bytes)?;
    conn.execute_batch("CREATE TABLE IF NOT EXISTS event_broker_operations (
        owner TEXT NOT NULL, scope TEXT NOT NULL, operation_id TEXT NOT NULL,
        event_id TEXT NOT NULL, fingerprint TEXT NOT NULL, route TEXT NOT NULL,
        position INTEGER NOT NULL, state TEXT NOT NULL,
        completion_reserve INTEGER NOT NULL CHECK(completion_reserve>=0),
        claim_token TEXT, claim_until INTEGER, completion_fingerprint TEXT,
        PRIMARY KEY(owner,scope,operation_id), UNIQUE(owner,scope,event_id));
        CREATE TABLE IF NOT EXISTS event_broker_config (
        singleton INTEGER PRIMARY KEY CHECK(singleton=1),batch_bytes INTEGER NOT NULL,
        batch_events INTEGER NOT NULL,payload_bytes INTEGER NOT NULL,completion_bytes INTEGER NOT NULL,
        lease_seconds INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS event_broker_event_keys (owner TEXT NOT NULL,scope TEXT NOT NULL,
        event_id TEXT NOT NULL,PRIMARY KEY(owner,scope,event_id));
        CREATE INDEX IF NOT EXISTS event_broker_claim_expiry
        ON event_broker_operations(owner,scope,state,claim_until);")?;
    conn.execute(
        "INSERT OR IGNORE INTO event_broker_config VALUES(1,?1,?2,?3,?4,?5)",
        params![
            config.limits.max_batch_bytes as i64,
            config.limits.max_batch_events as i64,
            config.max_payload_bytes as i64,
            config.max_completion_bytes as i64,
            config.max_lease_seconds
        ],
    )?;
    validate(conn, config)
}

pub(super) fn validate(conn: &Connection, config: Config) -> rusqlite::Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='event_broker_config')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        return Ok(());
    }
    let persisted: Option<(i64,i64,i64,i64,i64)> = conn.query_row("SELECT batch_bytes,batch_events,payload_bytes,completion_bytes,lease_seconds FROM event_broker_config WHERE singleton=1", [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?))).optional()?;
    if persisted
        != Some((
            config.limits.max_batch_bytes as i64,
            config.limits.max_batch_events as i64,
            config.max_payload_bytes as i64,
            config.max_completion_bytes as i64,
            config.max_lease_seconds,
        ))
    {
        return Err(storage::error(
            "BROKER_CONFIG_CONFLICT",
            "persisted standalone bounds cannot change while custody or work may be retained",
        ));
    }
    Ok(())
}
