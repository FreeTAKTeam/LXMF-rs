impl MessagesStore {
    pub fn list_meshchat_conversation(
        &self,
        destination: &str,
        after_id: Option<i64>,
        descending: bool,
        limit: usize,
    ) -> rusqlite::Result<Vec<(i64, MessageRecord)>> {
        self.with_read_conn(|conn| {
            let comparison = if descending { "<" } else { ">" };
            let ordering = if descending { "DESC" } else { "ASC" };
            let query = format!(
                "SELECT id, source, destination, title, content, timestamp, direction, fields, receipt_status, rowid \
                 FROM messages WHERE (LOWER(source) = LOWER(?1) OR LOWER(destination) = LOWER(?1)) \
                 AND (?2 IS NULL OR rowid {comparison} ?2) ORDER BY rowid {ordering} LIMIT ?3"
            );
            let mut statement = conn.prepare(&query)?;
            let mut rows = statement.query(params![destination, after_id, limit.min(i64::MAX as usize) as i64])?;
            let mut records = Vec::new();
            while let Some(row) = rows.next()? {
                records.push((row.get(9)?, message_record_from_row(row)?));
            }
            Ok(records)
        })
    }

    pub fn meshchat_message_row_id(&self, id: &str) -> rusqlite::Result<Option<i64>> {
        self.with_read_conn(|conn| {
            conn.query_row("SELECT rowid FROM messages WHERE id = ?1", params![id], |row| row.get(0))
                .optional()
        })
    }

    pub fn delete_meshchat_message(&self, id: &str) -> rusqlite::Result<usize> {
        self.with_write_conn(|conn| {
            let removed = conn.execute("DELETE FROM messages WHERE id = ?1", params![id])?;
            self.write_state.message_count_cache.fetch_sub(removed as u64, Ordering::Relaxed);
            Ok(removed)
        })
    }

    pub fn delete_meshchat_conversation(&self, destination: &str) -> rusqlite::Result<usize> {
        self.with_write_conn(|conn| {
            let removed = conn.execute(
                "DELETE FROM messages WHERE source = ?1 OR destination = ?1",
                params![destination],
            )?;
            self.write_state.message_count_cache.fetch_sub(removed as u64, Ordering::Relaxed);
            Ok(removed)
        })
    }
}
