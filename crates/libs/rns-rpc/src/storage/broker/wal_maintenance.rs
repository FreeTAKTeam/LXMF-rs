//! WAL file maintenance for the durable broker database (issue 663).
//!
//! `wal_autocheckpoint` keeps the WAL *content* checkpointed but never shrinks
//! the *file*; production sat on a 521 MiB WAL that never crossed the budget/8
//! emergency threshold. `journal_size_limit` lets SQLite truncate on reset, and
//! `maintain` runs a periodic PASSIVE-then-TRUNCATE pass that reports busy
//! instead of blocking on readers.

use crate::storage::operation_metrics::{WalCheckpointFrames, WalCheckpointOutcome};
use rusqlite::Connection;

pub const WAL_TRUNCATE_FLOOR_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalMaintenanceOutcome {
    pub outcome: WalCheckpointOutcome,
    pub size_before: u64,
    pub size_after: u64,
    pub checkpoint: Option<WalCheckpointFrames>,
}

/// WAL file size the routine maintenance tolerates before checkpointing:
/// `max(64 MiB, budget / 64)`. The budget/8 admission headroom is unchanged.
pub fn truncate_limit_bytes(budget: Option<u64>) -> u64 {
    budget.map_or(WAL_TRUNCATE_FLOOR_BYTES, |budget| (budget / 64).max(WAL_TRUNCATE_FLOOR_BYTES))
}

pub(crate) fn wal_file_size(conn: &Connection) -> rusqlite::Result<u64> {
    let Some(path) = conn.path() else {
        return Ok(0);
    };
    match std::fs::metadata(format!("{path}-wal")) {
        Ok(meta) => Ok(meta.len()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(rusqlite::Error::ToSqlConversionFailure(Box::new(error))),
    }
}

/// `PRAGMA wal_checkpoint(mode)` with `busy_timeout` forced to zero for the call,
/// so a pinned reader yields `Busy` at once instead of holding the write mutex.
fn checkpoint_nonblocking(
    conn: &Connection,
    mode: &str,
) -> rusqlite::Result<(WalCheckpointOutcome, WalCheckpointFrames)> {
    let timeout: i64 = conn.query_row("PRAGMA busy_timeout", [], |r| r.get(0))?;
    conn.pragma_update(None, "busy_timeout", 0)?;
    let result = conn.query_row(format!("PRAGMA wal_checkpoint({mode})").as_str(), [], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
    });
    conn.pragma_update(None, "busy_timeout", timeout)?;
    let (busy, log_frames, checkpointed_frames) = result?;
    let outcome = if busy != 0 {
        WalCheckpointOutcome::Busy
    } else if log_frames > checkpointed_frames {
        WalCheckpointOutcome::PassiveIncomplete
    } else {
        WalCheckpointOutcome::Truncated
    };
    Ok((outcome, WalCheckpointFrames { busy, log_frames, checkpointed_frames }))
}

pub(crate) fn checkpoint_truncate_nonblocking(
    conn: &Connection,
) -> rusqlite::Result<(WalCheckpointOutcome, WalCheckpointFrames)> {
    checkpoint_nonblocking(conn, "TRUNCATE")
}

/// Routine maintenance: once the WAL file reaches `threshold`, run PASSIVE and,
/// if every frame was checkpointed and nothing was busy, TRUNCATE the file.
/// `journal_size_limit` is the same value, so a WAL SQLite already trimmed on
/// reset sits at exactly `threshold`; `>=` keeps that steady state eligible.
pub(crate) fn maintain(
    conn: &Connection,
    threshold: u64,
) -> rusqlite::Result<WalMaintenanceOutcome> {
    let size_before = wal_file_size(conn)?;
    if size_before < threshold || !conn.is_autocommit() {
        return Ok(WalMaintenanceOutcome {
            outcome: WalCheckpointOutcome::BelowThreshold,
            size_before,
            size_after: size_before,
            checkpoint: None,
        });
    }
    let (outcome, checkpoint) = match checkpoint_nonblocking(conn, "PASSIVE")? {
        (WalCheckpointOutcome::Truncated, _) => checkpoint_nonblocking(conn, "TRUNCATE")?,
        other => other,
    };
    Ok(WalMaintenanceOutcome {
        outcome,
        size_before,
        size_after: wal_file_size(conn)?,
        checkpoint: Some(checkpoint),
    })
}

/// Shared cadence for the daemon timer and emergency write-path checkpoints.
pub fn wal_maintenance_interval() -> std::time::Duration {
    let seconds = match std::env::var("LXMD_STORAGE_WAL_MAINTENANCE_INTERVAL_SECS") {
        Ok(value) => match value.parse::<u64>() {
            Ok(seconds) => seconds.max(1),
            Err(error) => {
                log::warn!("invalid LXMD_STORAGE_WAL_MAINTENANCE_INTERVAL_SECS: {error}");
                60
            }
        },
        Err(_) => 60,
    };
    std::time::Duration::from_secs(seconds)
}
