use super::*;
use rand_core::{OsRng, RngCore};
pub fn enabled(conn: &Connection) -> rusqlite::Result<bool> {
    let table: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='broker_meta')",
        [],
        |r| r.get(0),
    )?;
    if !table {
        return Ok(false);
    }
    let version: Option<i64> = conn
        .query_row("SELECT min_writer FROM broker_meta LIMIT 1", [], |r| r.get(0))
        .optional()?;
    if version.is_some_and(|version| version != 1) {
        return Err(error(
            "SDK_STORAGE_WRITER_VERSION_UNSUPPORTED",
            "durable database requires another writer version",
        ));
    }
    Ok(version.is_some())
}

/// Configure the durability profile before beginning a migration transaction.
pub fn configure_profile(conn: &Connection, budget: i64) -> rusqlite::Result<()> {
    configure_profile_with_limits(conn, budget, Limits::default())
}
pub fn configure_profile_with_limits(
    conn: &Connection,
    budget: i64,
    limits: Limits,
) -> rusqlite::Result<()> {
    limits.validate()?;
    // Reopening keeps the admitted database's budget; configuration cannot shrink custody.
    let budget = if enabled(conn)? {
        conn.query_row("SELECT budget FROM broker_meta", [], |r| r.get(0))?
    } else {
        budget
    };
    if budget < (limits.max_batch_bytes * 2) as i64 {
        return Err(error("SDK_VALIDATION_BROKER_BUDGET", "journal budget is too small"));
    }
    let native: String = conn.query_row("SELECT sqlite_version()", [], |r| r.get(0))?;
    let version: Vec<u32> = native
        .split('.')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    if version.as_slice() < [3, 51, 3].as_slice() {
        return Err(error(
            "SDK_STORAGE_DURABILITY_UNAVAILABLE",
            format!("native SQLite {native} lacks the required WAL fix"),
        ));
    }
    let mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
    if mode != "wal" {
        return Err(error(
            "SDK_STORAGE_DURABILITY_UNAVAILABLE",
            "durable broker requires a local disk WAL database",
        ));
    }
    conn.pragma_update(None, "synchronous", "FULL")?;
    conn.pragma_update(None, "wal_autocheckpoint", 64)?;
    // Let SQLite truncate the WAL file when it resets after a checkpoint; without
    // this the file only ever grows (production: 521 MiB that never shrank).
    conn.pragma_update(
        None,
        "journal_size_limit",
        wal_maintenance::truncate_limit_bytes(Some(budget.max(0) as u64)) as i64,
    )?;
    let page_size: i64 = conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
    conn.pragma_update(None, "max_page_count", budget / 4 / page_size)?;
    let actual: i64 = conn.query_row("PRAGMA max_page_count", [], |r| r.get(0))?;
    if actual > budget / 4 / page_size {
        return Err(error(
            "SDK_STORAGE_BROKER_FULL",
            "existing database exceeds native durable page cap; cutover refused",
        ));
    }
    let sync: i64 = conn.query_row("PRAGMA synchronous", [], |r| r.get(0))?;
    if sync != 2 {
        return Err(error(
            "SDK_STORAGE_DURABILITY_UNAVAILABLE",
            "FULL synchronous was not applied",
        ));
    }
    physical_guard::install(conn, budget as u64)?;
    physical_guard::mutation(conn, false)?;
    Ok(())
}
/// Create generic tables in the caller's transaction; return whether a journal was initialized.
pub fn initialize_schema(conn: &Connection, budget: i64) -> rusqlite::Result<bool> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS broker_meta (
        singleton INTEGER PRIMARY KEY CHECK(singleton=1), journal_id TEXT NOT NULL, secret BLOB NOT NULL,
        next_position INTEGER NOT NULL, floor INTEGER NOT NULL, budget INTEGER NOT NULL, used_bytes INTEGER NOT NULL,
        min_writer INTEGER NOT NULL CHECK(min_writer=1));
        CREATE TABLE IF NOT EXISTS broker_events (
        position INTEGER PRIMARY KEY, local_destination TEXT NOT NULL, event_type TEXT NOT NULL,
        created_at INTEGER NOT NULL, payload TEXT NOT NULL, bytes INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS broker_consumers (
        consumer_id TEXT PRIMARY KEY, owner TEXT NOT NULL, destination TEXT NOT NULL, stored INTEGER NOT NULL,
        issued TEXT);
        CREATE INDEX IF NOT EXISTS broker_events_scope ON broker_events(local_destination,position);")?;
    if enabled(conn)? {
        return Ok(false);
    }
    let mut secret = [0u8; 32];
    OsRng.fill_bytes(&mut secret);
    let mut journal = [0u8; 16];
    OsRng.fill_bytes(&mut journal);
    conn.execute(
        "INSERT INTO broker_meta VALUES(1,?1,?2,1,0,?3,0,1)",
        params![hex::encode(journal), secret.as_slice(), budget],
    )?;
    Ok(true)
}
pub fn physical_admission(conn: &Connection, incoming: u64, budget: u64) -> rusqlite::Result<()> {
    physical_guard::mutation(conn, true)?;
    let page_count: i64 = conn.query_row("PRAGMA page_count", [], |r| r.get(0))?;
    let free_count: i64 = conn.query_row("PRAGMA freelist_count", [], |r| r.get(0))?;
    let page_size: i64 = conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
    let wal = match conn.path() {
        Some(path) => match std::fs::metadata(format!("{path}-wal")) {
            Ok(meta) => meta.len(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
            Err(error) => return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(error))),
        },
        None => 0,
    };
    let file = (page_count.max(0) as u64).saturating_mul(page_size.max(0) as u64);
    let reusable = (free_count.max(0) as u64).saturating_mul(page_size.max(0) as u64);
    // Message + journal + indexes, with a conservative WAL and completion reserve.
    let growth = incoming.saturating_mul(8).saturating_sub(reusable).saturating_add(512 * 1024);
    let control_headroom = budget / 8;
    if file.saturating_add(wal).saturating_add(growth) > budget.saturating_sub(control_headroom) {
        return Err(error(
            "SDK_STORAGE_BROKER_FULL",
            "database and WAL physical high-water mark reached; replay/ACK remains available",
        ));
    }
    Ok(())
}
