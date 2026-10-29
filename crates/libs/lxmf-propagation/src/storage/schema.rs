use rusqlite::Connection;

/// Use the caller's connection and transaction, preserving the daemon's writer.
pub fn initialize(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("                CREATE TABLE IF NOT EXISTS propagation_entries (
                    transient_id TEXT PRIMARY KEY,
                    destination TEXT NOT NULL,
                    payload_hex TEXT NOT NULL,
                    received_at INTEGER NOT NULL,
                    size_bytes INTEGER NOT NULL,
                    stamp_value INTEGER
                );
                CREATE TABLE IF NOT EXISTS propagation_peer_entries (
                    peer TEXT NOT NULL,
                    transient_id TEXT NOT NULL,
                    state TEXT NOT NULL,
                    updated_at INTEGER NOT NULL,
                    PRIMARY KEY(peer, transient_id)
                );
                CREATE TABLE IF NOT EXISTS propagation_local_entries (
                    transient_id TEXT PRIMARY KEY,
                    processed_at INTEGER NOT NULL
                );                CREATE INDEX IF NOT EXISTS idx_propagation_entries_destination_size
                    ON propagation_entries(destination, size_bytes, transient_id);
                CREATE INDEX IF NOT EXISTS idx_propagation_entries_retention_cover
                    ON propagation_entries(received_at DESC, transient_id DESC, destination, size_bytes);
                DROP INDEX IF EXISTS idx_propagation_entries_received_transient;
                CREATE INDEX IF NOT EXISTS idx_propagation_peer_entries_state
                    ON propagation_peer_entries(peer, state, transient_id);
                CREATE INDEX IF NOT EXISTS idx_propagation_peer_entries_folded_lookup
                    ON propagation_peer_entries(LOWER(peer), transient_id, state);
                CREATE INDEX IF NOT EXISTS idx_propagation_peer_entries_expiry
                    ON propagation_peer_entries(state, updated_at);")
}
