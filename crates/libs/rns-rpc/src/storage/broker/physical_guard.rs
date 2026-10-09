//! Shared-writer physical admission, including pre-commit spills and checkpoint growth.
use super::*;
use std::path::Path;
fn wal_size(path: &str) -> rusqlite::Result<u64> {
    match std::fs::metadata(format!("{path}-wal")) {
        Ok(meta) => Ok(meta.len()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(rusqlite::Error::ToSqlConversionFailure(Box::new(error))),
    }
}
fn limits(conn: &Connection) -> rusqlite::Result<(u64, u64)> {
    let pages: u64 =
        conn.query_row("PRAGMA max_page_count", [], |r| r.get::<_, i64>(0))?.max(0) as u64;
    let size: u64 = conn.query_row("PRAGMA page_size", [], |r| r.get::<_, i64>(0))?.max(0) as u64;
    let database = pages
        .checked_mul(size)
        .ok_or_else(|| error("SDK_STORAGE_BROKER_FULL", "page cap overflow"))?;
    // A respilled page overwrites its uncommitted frame. Reserve one additional final
    // commit frame, frame/header overhead and FULL-sync sector padding (<=64 KiB).
    let transaction = pages
        .checked_add(2)
        .and_then(|n| size.checked_add(24).and_then(|p| n.checked_mul(p)))
        .and_then(|n| n.checked_add(32 + 65536))
        .ok_or_else(|| error("SDK_STORAGE_BROKER_FULL", "WAL reservation overflow"))?;
    Ok((database, transaction))
}
fn fits(database: u64, wal: u64, transaction: u64, budget: u64, ordinary: bool) -> bool {
    database
        .checked_add(wal)
        .and_then(|n| {
            transaction.checked_mul(if ordinary { 2 } else { 1 }).and_then(|t| n.checked_add(t))
        })
        .is_some_and(|n| n <= budget)
}
pub(super) fn install(conn: &Connection, budget: u64) -> rusqlite::Result<()> {
    let (database, transaction) = limits(conn)?;
    let path = conn
        .path()
        .ok_or_else(|| {
            error("SDK_STORAGE_DURABILITY_UNAVAILABLE", "physical admission requires a local file")
        })?
        .to_owned();
    if std::fs::metadata(&path)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?
        .len()
        > database
    {
        return Err(error(
            "SDK_STORAGE_BROKER_FULL",
            "existing database exceeds native physical cap",
        ));
    }
    if !Path::new(&path).is_file() {
        return Err(error("SDK_STORAGE_DURABILITY_UNAVAILABLE", "database path is unavailable"));
    }
    if !fits(database, wal_size(&path)?, transaction, budget, false) {
        return Err(error(
            "SDK_STORAGE_BROKER_FULL",
            "initial database/WAL exceeds cutover reservation",
        ));
    }
    install_hook(conn, budget, true)
}
fn install_hook(conn: &Connection, budget: u64, multiple: bool) -> rusqlite::Result<()> {
    let (database, transaction) = limits(conn)?;
    let path = conn
        .path()
        .ok_or_else(|| {
            error("SDK_STORAGE_DURABILITY_UNAVAILABLE", "physical admission requires a local file")
        })?
        .to_owned();
    conn.commit_hook(Some(move || {
        // No SQLite API may be called from this hook. This also guards individual
        // autocommits inside legacy writers; the job's first mutation is reserved below.
        std::panic::catch_unwind(|| match wal_size(&path) {
            Ok(wal) => !fits(database, wal, transaction, budget, multiple),
            Err(error) => {
                log::error!("durable WAL admission failed: {error}");
                true
            }
        })
        .unwrap_or(true)
    }))
}
pub(crate) fn single_transaction<T>(
    conn: &Connection,
    f: impl FnOnce(&Connection) -> rusqlite::Result<T>,
) -> rusqlite::Result<T> {
    if !enabled(conn)? {
        return f(conn);
    }
    let budget: i64 = conn.query_row("SELECT budget FROM broker_meta", [], |r| r.get(0))?;
    install_hook(conn, budget as u64, false)?;
    let result = f(conn);
    install_hook(conn, budget as u64, true)?;
    result
}
pub(crate) fn mutation(conn: &Connection, ordinary: bool) -> rusqlite::Result<()> {
    if !enabled(conn)? {
        return Ok(());
    }
    let budget: u64 =
        conn.query_row("SELECT budget FROM broker_meta", [], |r| r.get::<_, i64>(0))?.max(0) as u64;
    let (database, transaction) = limits(conn)?;
    let wal = conn.path().map(wal_size).transpose()?.unwrap_or(0);
    if !fits(database, wal, transaction, budget, ordinary) {
        return Err(error(
            "SDK_STORAGE_BROKER_FULL",
            "shared database/WAL admission is full; release pinned readers and retry",
        ));
    }
    Ok(())
}
pub(crate) fn map_commit_error(error: rusqlite::Error) -> rusqlite::Error {
    if matches!(&error,rusqlite::Error::SqliteFailure(code,_) if code.extended_code==rusqlite::ffi::SQLITE_CONSTRAINT_COMMITHOOK)
    {
        super::error(
            "SDK_STORAGE_BROKER_FULL",
            "shared WAL commit admission refused; transaction rolled back",
        )
    } else {
        error
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_spills_and_legacy_autocommits_remain_bounded_with_pinned_reader() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("spill.db");
        let store = crate::storage::messages::MessagesStore::open(&path).unwrap();
        let budget = 32 * 1024 * 1024;
        store.enable_durable_broker(budget).unwrap();
        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "max_page_count", budget / 4 / 4096).unwrap();
        conn.execute_batch("PRAGMA synchronous=FULL;PRAGMA wal_autocheckpoint=0;PRAGMA cache_size=4;CREATE TABLE spill(value BLOB);INSERT INTO spill VALUES(zeroblob(2000000));PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
        install(&conn, budget as u64).unwrap();
        let reader = Connection::open(&path).unwrap();
        reader.execute_batch("BEGIN;SELECT * FROM spill;").unwrap();
        mutation(&conn, true).unwrap();
        let (_, maximum_transaction) = limits(&conn).unwrap();
        let tx = conn.unchecked_transaction().unwrap();
        for _ in 0..10 {
            tx.execute("UPDATE spill SET value=randomblob(2000000)", []).unwrap();
            assert!(wal_size(path.to_str().unwrap()).unwrap() <= maximum_transaction);
        }
        tx.commit().unwrap();
        // One legacy job may contain several autocommits. The hook terminates that
        // job on the first refused commit, even though cache spills precede it.
        mutation(&conn, true).unwrap();
        let mut rejected = false;
        for _ in 0..30 {
            match conn.execute("UPDATE spill SET value=randomblob(2000000)", []) {
                Ok(_) => {}
                Err(error) => {
                    assert!(map_commit_error(error).to_string().contains("BROKER_FULL"));
                    rejected = true;
                    break;
                }
            }
            assert!(
                std::fs::metadata(&path).unwrap().len() + wal_size(path.to_str().unwrap()).unwrap()
                    <= budget as u64
            );
        }
        assert!(rejected);
        assert!(
            std::fs::metadata(&path).unwrap().len() + wal_size(path.to_str().unwrap()).unwrap()
                <= budget as u64
        );
        assert!(mutation(&conn, true).is_err());
        reader.execute_batch("ROLLBACK").unwrap();
        super::super::reclaim_wal(&conn).unwrap();
        mutation(&conn, true).unwrap();
        conn.execute("UPDATE spill SET value=zeroblob(2000000)", []).unwrap();
    }
}
