use crate::{prepared::delivery_address, Error, StoreLimits};
use rns_core::identity::PrivateIdentity;
use rusqlite::{params, Connection, OptionalExtension};

pub(crate) fn initialize(
    conn: &Connection,
    owner: &PrivateIdentity,
    limits: StoreLimits,
) -> Result<(), Error> {
    if limits.capacity_bytes == 0
        || limits.capacity_bytes > crate::CAPACITY_BYTES
        || !(1..=crate::RETENTION_SECS).contains(&limits.retention_secs)
    {
        return Err(Error::InvalidState("storage limits exceed supported bounds"));
    }
    conn.pragma_update(None, "page_size", 4096)?;
    let page_size: i64 = conn.pragma_query_value(None, "page_size", |row| row.get(0))?;
    if page_size != 4096 {
        return Err(Error::InvalidState("unsupported database page size"));
    }
    // Rollback journals hold at most one before-image per database page; no unbounded WAL.
    conn.execute_batch(
        "PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;",
    )?;
    let max_pages = (limits.capacity_bytes * 3 + 8 * 1024 * 1024).div_ceil(4096);
    conn.pragma_update(None, "max_page_count", max_pages as i64)?;
    conn.pragma_update(None, "journal_size_limit", (max_pages * (4096 + 8) + 512) as i64)?;
    conn.execute_batch("CREATE TABLE IF NOT EXISTS propagation_owner (owner BLOB PRIMARY KEY CHECK(length(owner)=16));")?;
    let owner_bytes = delivery_address(owner.as_identity());
    let known: Option<Vec<u8>> = conn
        .query_row("SELECT owner FROM propagation_owner LIMIT 1", [], |r| r.get(0))
        .optional()?;
    if known.as_deref().is_some_and(|bytes| bytes != owner_bytes) {
        return Err(Error::ForeignMessage);
    }
    conn.execute(
        "INSERT OR IGNORE INTO propagation_owner(owner) VALUES(?1)",
        params![owner_bytes],
    )?;
    crate::storage::initialize(conn)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS propagation_owned (
        transient_id TEXT PRIMARY KEY CHECK(length(transient_id)=64),
        message_id TEXT NOT NULL UNIQUE CHECK(length(message_id)=64),
        destination TEXT NOT NULL CHECK(length(destination)=32),
        application_key TEXT CHECK(application_key IS NULL OR length(application_key)<=128),
        admitted_at INTEGER NOT NULL, expires_at INTEGER NOT NULL,
        recovery BLOB, encrypted_bytes INTEGER NOT NULL CHECK(encrypted_bytes>=0),
        kind INTEGER NOT NULL, replacement_key TEXT CHECK(length(replacement_key)<=128),
        state INTEGER NOT NULL, uploaded INTEGER NOT NULL DEFAULT 0,
        relay TEXT CHECK(relay IS NULL OR length(relay)=32),
        attempts INTEGER NOT NULL DEFAULT 0, next_attempt_at INTEGER NOT NULL DEFAULT 0,
        projected INTEGER NOT NULL DEFAULT 0, changed_at INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_propagation_owned_state ON propagation_owned(state, admitted_at);
    CREATE INDEX IF NOT EXISTS idx_propagation_owned_expiry ON propagation_owned(expires_at);
    ",
    )?;
    // A crash after dispatch cannot prove whether the relay accepted the payload.
    conn.execute("UPDATE propagation_owned SET state=2, uploaded=1 WHERE state=1", [])?;
    Ok(())
}
