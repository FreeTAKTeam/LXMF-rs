use super::*;

impl MessagesStore {
    /// Bounded keyset sweep of each peer's retained latest announce. Busy peers
    /// cannot push quiet peers out of the projection. Updates behind the cursor
    /// are reconciled on the next sweep, including after a consumer restart.
    pub fn broker_announce_peer_page(
        &self,
        after_peer: Option<&str>,
    ) -> rusqlite::Result<Vec<AnnounceRecord>> {
        self.with_read_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            let mut records = Vec::with_capacity(32);
            {
                // Seek past all history for the previous peer using the retention
                // index, rather than DISTINCT/GROUP BY over the entire history.
                let mut next = tx.prepare(
                    "SELECT id, peer, COALESCE(length(CAST(app_data_hex AS BLOB)),0)
                     +COALESCE(length(CAST(name AS BLOB)),0)
                     +COALESCE(length(CAST(name_source AS BLOB)),0)
                     +COALESCE(length(CAST(capabilities AS BLOB)),0)
                     +length(CAST(id AS BLOB))+length(CAST(peer AS BLOB))
                     FROM announces WHERE peer > ?1
                     ORDER BY peer, timestamp DESC, id DESC LIMIT 1",
                )?;
                let mut record = tx.prepare(
                    "SELECT id, peer, timestamp, name, name_source, first_seen, seen_count,
                     app_data_hex, capabilities, rssi, snr, q, stamp_cost,
                     stamp_cost_flexibility, peering_cost FROM announces WHERE id = ?1",
                )?;
                let mut cursor = after_peer.unwrap_or("").to_owned();
                for _ in 0..32 {
                    let candidate = next
                        .query_row([&cursor], |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, i64>(2)?,
                            ))
                        })
                        .optional()?;
                    let Some((id, peer, bytes)) = candidate else { break };
                    if bytes > 128 * 1024 {
                        return Err(super::super::broker::error(
                            "SDK_STORAGE_BROKER_EVENT_TOO_LARGE",
                            "announce projection record exceeds 128 KiB",
                        ));
                    }
                    records.push(record.query_row([id], parse_announce_row)?);
                    cursor = peer;
                }
            }
            tx.commit()?;
            Ok(records)
        })
    }
}

#[cfg(test)]
mod tests;
