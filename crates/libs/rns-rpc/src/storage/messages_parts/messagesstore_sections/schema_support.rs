fn serialize_json_for_sql<T: serde::Serialize>(value: &T) -> rusqlite::Result<String> {
    serde_json::to_string(value)
        .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))
}

fn deserialize_json_column<T: serde::de::DeserializeOwned>(
    value: &str,
    column: usize,
) -> rusqlite::Result<T> {
    serde_json::from_str(value).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(err),
        )
    })
}

impl MessagesStore {
    fn schema_has_column(
        conn: &Connection,
        table: &'static str,
        column: &'static str,
    ) -> rusqlite::Result<bool> {
        let mut statement = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let existing: String = row.get(1)?;
            if existing == column {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn ensure_schema_column(
        conn: &Connection,
        table: &'static str,
        column: &'static str,
        declaration: &'static str,
    ) -> rusqlite::Result<()> {
        if !Self::schema_has_column(conn, table, column)? {
            conn.execute(format!("ALTER TABLE {table} ADD COLUMN {declaration}").as_str(), [])?;
        }
        Ok(())
    }
}
