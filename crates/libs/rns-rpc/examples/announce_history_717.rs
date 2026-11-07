//! Synthetic, isolated #717 query/retention probe. Never point this at a live database.
//! Run in release mode: announce_history_717 <new-db-path> <baseline|candidate> [rows].
use rns_rpc::{AnnounceRecord, MessagesStore};
use rusqlite::{Connection, OptionalExtension, StatementStatus};
use serde_json::json;
use std::{error::Error, path::Path, time::Instant};

const STAMP: &str = "SELECT stamp_cost FROM announces WHERE peer = ?1 AND stamp_cost IS NOT NULL ORDER BY timestamp DESC, id DESC LIMIT 1";
const HISTORY_OLD: &str = "SELECT id, peer, timestamp, name, name_source, first_seen, seen_count, app_data_hex, capabilities, rssi, snr, q, stamp_cost, stamp_cost_flexibility, peering_cost FROM announces WHERE (timestamp < ?1 OR (timestamp = ?1 AND id < ?2)) ORDER BY timestamp DESC, id DESC LIMIT ?3";

fn query(
    conn: &Connection,
    family: &str,
    sql: &str,
    args: &[&dyn rusqlite::ToSql],
) -> rusqlite::Result<()> {
    let plan = conn
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?
        .query_map(args, |row| row.get::<_, String>(3))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    eprintln!("BEGIN {family}");
    let mut statement = conn.prepare(sql)?;
    let started = Instant::now();
    let mut count = 0;
    {
        let mut rows = statement.query(args)?;
        while rows.next()?.is_some() {
            count += 1;
        }
    }
    println!(
        "{}",
        json!({"family":family,"plan":plan,"rows":count,
        "wall_ns":started.elapsed().as_nanos(),"vm_steps":statement.get_status(StatementStatus::VmStep),
        "sorts":statement.get_status(StatementStatus::Sort),"fullscan_steps":statement.get_status(StatementStatus::FullscanStep)})
    );
    eprintln!("END {family}");
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    let path = Path::new(args.get(1).ok_or("supply new database path")?);
    let mode = args.get(2).ok_or("supply baseline or candidate")?;
    if !matches!(mode.as_str(), "baseline" | "candidate") {
        return Err("invalid mode".into());
    }
    if path.exists() {
        return Err("refusing to modify an existing database".into());
    }
    let count: i64 = args.get(3).map(|s| s.parse()).transpose()?.unwrap_or(1_700_000);
    if count < 2000 {
        return Err("fixture needs at least 2000 rows".into());
    }
    drop(MessagesStore::open(path)?);
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
        DROP INDEX idx_announces_peer_latest; DROP INDEX idx_announces_peer_stamp;",
    )?;
    let started = Instant::now();
    conn.execute("WITH RECURSIVE seq(x) AS (VALUES(0) UNION ALL SELECT x+1 FROM seq WHERE x+1 < ?1)
        INSERT INTO announces(id,peer,timestamp,name,first_seen,seen_count,app_data_hex,capabilities,stamp_cost)
        SELECT printf('%012d',x),printf('%032x',x%300),1700000000+x/10,
        'synthetic peer',1700000000,1,hex(zeroblob(128)),'[]',CASE WHEN x%7=0 THEN x%16 ELSE NULL END FROM seq", [count])?;
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
    println!(
        "{}",
        json!({"stage":"fixture","rows":count,"seconds":started.elapsed().as_secs_f64(),"sqlite":rusqlite::version(),"mode":mode})
    );
    let cursor = 1_700_000_000 + count / 20;
    let cursor_id = format!("{:012}", count / 2);
    if mode == "baseline" {
        query(&conn, "stamp_absent", STAMP, &[&"missing"])?;
        query(&conn, "stamp_present", STAMP, &[&format!("{:032x}", 42)])?;
        query(&conn, "history_deep", HISTORY_OLD, &[&cursor, &cursor_id, &100])?;
        return Ok(());
    }
    let started = Instant::now();
    let store = MessagesStore::open(path)?;
    println!("{}", json!({"stage":"migration","seconds":started.elapsed().as_secs_f64()}));
    let conn = Connection::open(path)?;
    query(&conn, "stamp_absent", STAMP, &[&"missing"])?;
    query(&conn, "stamp_present", STAMP, &[&format!("{:032x}", 42)])?;
    query(&conn, "history_deep", HISTORY_OLD, &[&cursor, &cursor_id, &100])?;
    let original = (0..300)
        .map(|peer| {
            let peer = format!("{peer:032x}");
            let id: String = conn.query_row(
                "SELECT id FROM announces WHERE peer=?1 ORDER BY timestamp DESC,id DESC LIMIT 1",
                [&peer],
                |row| row.get(0),
            )?;
            Ok((peer.clone(), id, store.latest_announce_stamp_cost_for(&peer)?))
        })
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let started = Instant::now();
    let mut deleted = 0;
    let mut injected = 0;
    eprintln!("BEGIN retention");
    for batch in 0..10000 {
        // Six new events per batch, above the observed one-per-second ingest relative to a 512-row pass.
        for _ in 0..6 {
            store.insert_announce(&AnnounceRecord {
                id: format!("new-{injected:012}"),
                peer: "ingress".into(),
                timestamp: 1_800_000_000 + injected,
                name: None,
                name_source: None,
                first_seen: 1_800_000_000,
                seen_count: 1,
                app_data_hex: None,
                capabilities: vec![],
                rssi: None,
                snr: None,
                q: None,
                stamp_cost: Some(7),
                stamp_cost_flexibility: None,
                peering_cost: None,
            })?;
            injected += 1;
        }
        let removed = store.prune_announce_history_batch(100_000)?;
        assert!(removed <= 512);
        deleted += removed;
        if batch % 500 == 0 {
            println!(
                "{}",
                json!({"stage":"prune","batch":batch,"deleted":deleted,"seconds":started.elapsed().as_secs_f64()})
            );
        }
        if count + injected - deleted as i64 <= 100_602 {
            break;
        }
        assert!(batch < 9999, "retention failed to converge");
    }
    for _ in 0..5 {
        deleted += store.prune_announce_history_batch(100_000)?;
    }
    eprintln!("END retention");
    let retained: i64 = conn.query_row("SELECT COUNT(*) FROM announces", [], |r| r.get(0))?;
    assert_eq!(retained, count + injected - deleted as i64);
    assert!(retained <= 100_602);
    for (peer, id, stamp) in original {
        assert!(conn
            .query_row("SELECT 1 FROM announces WHERE id=?1", [id], |r| r.get::<_, i64>(0))
            .optional()?
            .is_some());
        assert_eq!(store.latest_announce_stamp_cost_for(&peer)?, stamp);
    }
    println!(
        "{}",
        json!({"stage":"complete","retained":retained,"deleted":deleted,"injected":injected,
        "seconds":started.elapsed().as_secs_f64(),"metrics":store.operation_metrics_snapshot()["announces"]})
    );
    Ok(())
}
