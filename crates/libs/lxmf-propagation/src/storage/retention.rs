use rusqlite::{params, Connection};

pub fn prune_propagation_entries_to_limit_bytes_with_priorities(
    conn: &Connection,
    limit_bytes: u64,
    prioritised_destinations: &[String],
) -> rusqlite::Result<Vec<String>> {
    let mut removed = Vec::new();
    loop {
        let page = prune_propagation_entries_page(conn, limit_bytes, prioritised_destinations)?;
        if page.is_empty() {
            return Ok(removed);
        }
        removed.extend(page);
    }
}

/// Evict from at most 4,096 oldest candidates; priority/weight ordering applies
/// within that window. The caller owns the transaction and paging lifetime.
pub fn prune_propagation_entries_page(
    conn: &Connection,
    limit_bytes: u64,
    prioritised_destinations: &[String],
) -> rusqlite::Result<Vec<String>> {
    let total: i64 = conn.query_row(
        "SELECT COALESCE(SUM(size_bytes), 0) FROM propagation_entries",
        [],
        |row| row.get(0),
    )?;
    let mut total = total.max(0) as u64;
    if total <= limit_bytes {
        return Ok(Vec::new());
    }

    let entries = {
        let mut stmt = conn.prepare(
            "SELECT transient_id, destination, size_bytes, received_at
                     FROM propagation_entries
                     ORDER BY received_at ASC, transient_id ASC LIMIT 4096",
        )?;
        let rows = stmt.query_map([], |row| {
            let transient_id: String = row.get(0)?;
            let destination: String = row.get(1)?;
            let size_bytes: i64 = row.get(2)?;
            let received_at: i64 = row.get(3)?;
            Ok((transient_id, destination, size_bytes.max(0) as u64, received_at))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let newest_received_at =
        conn.query_row("SELECT MAX(received_at) FROM propagation_entries", [], |row| {
            row.get::<_, Option<i64>>(0)
        })?;
    let mut entries = entries;
    entries.sort_by(|left, right| {
        let left_weight = propagation_prune_weight(
            left.1.as_str(),
            left.2,
            left.3,
            newest_received_at,
            prioritised_destinations,
        );
        let right_weight = propagation_prune_weight(
            right.1.as_str(),
            right.2,
            right.3,
            newest_received_at,
            prioritised_destinations,
        );
        right_weight
            .partial_cmp(&left_weight)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.3.cmp(&right.3))
            .then_with(|| left.0.cmp(&right.0))
    });

    let mut pruned = Vec::new();
    for (transient_id, _destination, size_bytes, _received_at) in entries {
        if total <= limit_bytes {
            break;
        }
        let affected = conn.execute(
            "DELETE FROM propagation_entries WHERE transient_id = ?1",
            params![transient_id],
        )?;
        if affected > 0 {
            conn.execute(
                "DELETE FROM propagation_peer_entries
                         WHERE transient_id = ?1
                           AND state = 'unhandled'",
                params![transient_id],
            )?;
            total = total.saturating_sub(size_bytes);
            pruned.push(transient_id);
        }
    }
    Ok(pruned)
}

fn propagation_prune_weight(
    destination: &str,
    size_bytes: u64,
    received_at: i64,
    newest_received_at: Option<i64>,
    prioritised_destinations: &[String],
) -> f64 {
    const FOUR_DAYS_SECS: f64 = 4.0 * 24.0 * 60.0 * 60.0;

    let age_secs = newest_received_at.unwrap_or(received_at).saturating_sub(received_at) as f64;
    let age_weight = (age_secs / FOUR_DAYS_SECS).max(1.0);
    let priority_weight = if prioritised_destinations
        .iter()
        .any(|candidate| destination.eq_ignore_ascii_case(candidate.trim()))
    {
        0.1
    } else {
        1.0
    };
    priority_weight * age_weight * size_bytes as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oldest_metadata_scan_is_covered_after_upgrading_the_legacy_index() {
        let conn = Connection::open_in_memory().expect("open in-memory database");
        crate::storage::initialize(&conn).expect("initialize propagation schema");
        conn.execute_batch(
            "DROP INDEX idx_propagation_entries_retention_cover;
             CREATE INDEX idx_propagation_entries_received_transient
                 ON propagation_entries(received_at DESC, transient_id DESC);",
        )
        .expect("install legacy received index");

        crate::storage::initialize(&conn).expect("upgrade retention index");
        let mut statement = conn
            .prepare(
                "EXPLAIN QUERY PLAN SELECT transient_id, destination, size_bytes, received_at
                 FROM propagation_entries
                 ORDER BY received_at ASC, transient_id ASC",
            )
            .expect("prepare retention query plan");
        let plan = statement
            .query_map([], |row| row.get::<_, String>(3))
            .expect("explain retention query")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("collect query plan")
            .join(" ");

        assert!(plan.contains("COVERING INDEX idx_propagation_entries_retention_cover"), "{plan}");
        assert!(!plan.contains("TEMP B-TREE"), "{plan}");
        let mut index_statement =
            conn.prepare("PRAGMA index_list(propagation_entries)").expect("prepare index list");
        let indexes = index_statement
            .query_map([], |row| row.get::<_, String>(1))
            .expect("read index list")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("collect index names");
        assert!(!indexes.iter().any(|name| name == "idx_propagation_entries_received_transient"));
    }
}

#[cfg(test)]
mod paging_tests {
    use super::*;

    #[test]
    fn retention_bounds_priority_sorting_to_oldest_candidate_window() {
        let conn = Connection::open_in_memory().expect("database");
        crate::storage::initialize(&conn).expect("schema");
        conn.execute_batch(
            "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<5000)
            INSERT INTO propagation_entries
            SELECT printf('id-%05d', x), CASE WHEN x<=4096 THEN 'priority' ELSE 'ordinary' END,
                   '00', x, 1, NULL FROM n;",
        )
        .expect("inventory");
        let removed = prune_propagation_entries_to_limit_bytes_with_priorities(
            &conn,
            4999,
            &["priority".into()],
        )
        .expect("prune");
        assert_eq!(
            removed,
            ["id-00001"],
            "age selects the window before priority orders its candidates"
        );
    }
}
