//! Synthetic #717 protected-floor probe. Never point this at a live database.
//! Run in release mode: announce_floor_717 <new-db-path> [protected_peers] [window]
use rns_rpc::{AnnounceRecord, MessagesStore};
use serde_json::json;
use std::{error::Error, path::Path, time::Instant};

fn record(id: u64, peer: String, timestamp: i64) -> AnnounceRecord {
    serde_json::from_value(json!({"id": format!("{id:012}"), "peer": peer, "timestamp": timestamp,
        "name": null, "name_source": null, "first_seen": timestamp, "seen_count": 1,
        "capabilities": [], "stamp_cost": 1}))
    .expect("record")
}

fn examined(store: &MessagesStore) -> u64 {
    store.operation_metrics_snapshot()["announces"]["examined_total"].as_u64().unwrap_or(0)
}

/// Runs sweeps until one deletes nothing, plus `extra` idle ticks; returns (calls, examined, ms).
fn sweep(
    store: &MessagesStore,
    window: u64,
    extra: usize,
) -> Result<(usize, u64, u128), Box<dyn Error>> {
    let (before, started, mut calls) = (examined(store), Instant::now(), 0);
    while store.prune_announce_history_batch(window)? > 0 {
        calls += 1;
    }
    for _ in 0..=extra {
        store.prune_announce_history_batch(window)?;
        calls += 1;
    }
    Ok((calls, examined(store) - before, started.elapsed().as_millis()))
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    let path = Path::new(args.get(1).ok_or("supply new database path")?);
    if path.exists() {
        return Err("refusing to modify an existing database".into());
    }
    let protected: u64 = args.get(2).map(|s| s.parse()).transpose()?.unwrap_or(50_000);
    let window: u64 = args.get(3).map(|s| s.parse()).transpose()?.unwrap_or(10_000);
    let store = MessagesStore::open(path)?;
    let mut id = 0;
    // Old peers: one protected latest row each, below the recent window.
    for peer in 0..protected {
        id += 1;
        store.insert_announce(&record(id, format!("old-{peer:08}"), id as i64))?;
    }
    // One busy peer fills the window and leaves `window` disposable older events.
    for _ in 0..2 * window {
        id += 1;
        store.insert_announce(&record(id, "busy".into(), id as i64))?;
    }
    let report = |phase: &str, (calls, examined, ms): (usize, u64, u128)| {
        println!(
            "{}",
            json!({"phase": phase, "calls": calls, "rows_examined": examined, "wall_ms": ms})
        );
    };
    report("drain", sweep(&store, window, 0)?);
    report("quiet_30_ticks", sweep(&store, window, 29)?);
    for _ in 0..100 {
        id += 1;
        store.insert_announce(&record(id, "busy".into(), id as i64))?;
    }
    report("ingest_100", sweep(&store, window, 0)?);
    println!(
        "{}",
        json!({"phase": "final", "rows": store.list_announces(usize::MAX >> 1, None, None)?.len()})
    );
    Ok(())
}
