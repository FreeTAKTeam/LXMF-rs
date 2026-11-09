//! A conservative cap-check shortcut; public storage statistics remain exact.
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};

#[cfg(test)]
mod tests;

const ROW_SIZE: &str = "LENGTH(@id) +
    LENGTH(@source) +
    LENGTH(@destination) +
    LENGTH(@title) +
    LENGTH(@content) +
    LENGTH(@direction) +
    COALESCE(LENGTH(@fields), 0) +
    COALESCE(LENGTH(@receipt_status), 0)";

pub(super) fn exact_total(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row(
        &format!("SELECT COALESCE(SUM({}), 0) FROM messages", ROW_SIZE.replace('@', "")),
        [],
        |row| row.get(0),
    )
}

pub(super) fn ensure_table(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS message_budget_bound (
            singleton INTEGER PRIMARY KEY CHECK(singleton=1),
            upper_bound INTEGER CHECK(upper_bound IS NULL OR
                (typeof(upper_bound)='integer' AND upper_bound>=0))
         );",
    )
}

fn credited_bound(add: &str) -> String {
    // Charge before the row write: RAISE(FAIL) can retain it while skipping later
    // AFTER triggers. Ignored/failed writes may overcredit; never subtract an
    // update's old size, including nested mutations and REPLACE collisions.
    let value = format!("upper_bound + ({add})");
    format!(
        "UPDATE message_budget_bound SET upper_bound=CASE
            WHEN typeof(upper_bound)='integer' AND upper_bound>=0
             AND typeof({add})='integer' AND ({add})>=0
             AND typeof({value})='integer' AND ({value})>=0
            THEN ({value}) ELSE NULL END WHERE singleton=1;"
    )
}

fn definitions() -> [(&'static str, String); 3] {
    // COALESCE belongs around the whole nullable row expression: SUM ignored a
    // legacy row with NULL in a required column, rather than counting its rest.
    let new = format!("COALESCE(({}),0)", ROW_SIZE.replace('@', "NEW."));
    let old = format!("COALESCE(({}),0)", ROW_SIZE.replace('@', "OLD."));
    // Preserve existing allowance on shrink/no-op updates. Positive growth is
    // charged before a later trigger can FAIL; shrink/regrow and ignored writes
    // may retain excess allowance, which the exact SUM reconciles when needed.
    let growth = format!(
        "CASE WHEN typeof({new})='integer' AND typeof({old})='integer'
         THEN CASE WHEN ({new})>({old}) THEN ({new})-({old}) ELSE 0 END ELSE NULL END"
    );
    [
        (
            "message_budget_insert",
            format!(
                "CREATE TRIGGER message_budget_insert BEFORE INSERT ON messages
                 BEGIN {} END",
                credited_bound(&new)
            ),
        ),
        (
            "message_budget_update",
            format!(
                "CREATE TRIGGER message_budget_update BEFORE UPDATE OF
                    id,source,destination,title,content,direction,fields,receipt_status ON messages
                 BEGIN {} END",
                credited_bound(&growth)
            ),
        ),
        (
            "message_budget_delete",
            "CREATE TRIGGER message_budget_delete AFTER DELETE ON messages
             BEGIN UPDATE message_budget_bound SET upper_bound=NULL
               WHERE singleton=1 AND upper_bound IS NOT NULL; END"
                .to_owned(),
        ),
    ]
}

fn normalized(sql: &str) -> &str {
    sql.trim().trim_end_matches(';').trim()
}

/// Runs inside the existing schema transaction, after all referenced columns exist.
pub(super) fn initialize(conn: &Connection) -> rusqlite::Result<()> {
    let mut changed = false;
    for (name, sql) in definitions() {
        let installed: Option<String> = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='trigger' AND name=?1",
                [name],
                |row| row.get(0),
            )
            .optional()?;
        if installed.as_deref().map(normalized) != Some(normalized(&sql)) {
            conn.execute_batch(&format!("DROP TRIGGER IF EXISTS {name}; {sql};"))?;
            changed = true;
        }
    }
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM message_budget_bound WHERE singleton=1)",
        [],
        |row| row.get(0),
    )?;
    if changed || !exists {
        // No hidden SUM on reopen (including unknown/overflow state). An old
        // population starts unknown; the first needed cap check reconciles it.
        let nonempty: bool =
            conn.query_row("SELECT EXISTS(SELECT 1 FROM messages)", [], |row| row.get(0))?;
        conn.execute(
            "INSERT INTO message_budget_bound VALUES(1,?1)
             ON CONFLICT(singleton) DO UPDATE SET upper_bound=excluded.upper_bound",
            [if nonempty { None } else { Some(0_i64) }],
        )?;
    }
    Ok(())
}

/// A returned over-cap value is exact. A possibly conservative value is only
/// returned when it already certifies that no pruning is required.
pub(super) fn total_for_prune(conn: &Connection, limit_bytes: u64) -> rusqlite::Result<i64> {
    let bound: Option<i64> = conn
        .query_row(
            "SELECT CASE WHEN typeof(upper_bound)='integer' AND upper_bound>=0
                THEN upper_bound ELSE NULL END
             FROM message_budget_bound WHERE singleton=1",
            [],
            |row| row.get::<_, Option<i64>>(0),
        )
        .optional()?
        .flatten();
    if let Some(bound) = bound {
        if bound as u64 <= limit_bytes {
            return Ok(bound);
        }
    }

    // Reconcile a consistent total and its bound without an intervening writer.
    // This transaction ends BEFORE the legacy candidate/deletion loop. In
    // particular, a later failed DELETE must not undo earlier committed ones.
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let total = exact_total(&tx)?;
    tx.execute(
        "INSERT INTO message_budget_bound VALUES(1,?1)
         ON CONFLICT(singleton) DO UPDATE SET upper_bound=excluded.upper_bound",
        [total.max(0)],
    )?;
    tx.commit()?;
    Ok(total)
}
