//! Existing daemon identity metadata, shared without importing its runtime.
use rusqlite::{params, Connection, OptionalExtension};

pub fn initialize(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS announce_identities (
        peer TEXT PRIMARY KEY, public_key_hex TEXT NOT NULL,
        verifying_key_hex TEXT NOT NULL, updated_at INTEGER NOT NULL
    );",
    )
}

pub fn keys(conn: &Connection, peer: &str) -> rusqlite::Result<Option<(String, String)>> {
    conn.query_row(
        "SELECT public_key_hex, verifying_key_hex FROM announce_identities WHERE peer=?1 LIMIT 1",
        [peer],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
}

pub fn upsert(
    conn: &Connection,
    peer: &str,
    public: &str,
    verifying: &str,
    now: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO announce_identities(peer,public_key_hex,verifying_key_hex,updated_at)
        VALUES(?1,?2,?3,?4) ON CONFLICT(peer) DO UPDATE SET
        public_key_hex=excluded.public_key_hex, verifying_key_hex=excluded.verifying_key_hex,
        updated_at=excluded.updated_at",
        params![peer, public, verifying, now],
    )?;
    Ok(())
}
