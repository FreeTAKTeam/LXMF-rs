//! Opt-in local query workload; this is not full-daemon/cgroup acceptance.
use super::tests::{ids_and_cost, BASELINE_SQL};
use super::*;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

fn counters(path: &str) -> BTreeMap<String, u64> {
    std::fs::read_to_string(path)
        .expect("Linux process counters")
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            Some((key.to_owned(), value.split_whitespace().next()?.parse().ok()?))
        })
        .collect()
}

fn phase(store: &MessagesStore, baseline: bool, rounds: usize) {
    let before = counters("/proc/self/io");
    let cpu_before = std::fs::read_to_string("/proc/self/stat").expect("CPU counters");
    let started = Instant::now();
    let mut total_ns = 0u128;
    let mut max_ns = 0u128;
    let mut steps = 0u64;
    let mut sorts = 0u64;
    let mut rss_min = u64::MAX;
    let mut rss_max = 0;
    for round in 0..rounds {
        let tick = Instant::now();
        store
            .with_read_conn(|conn| {
                let (ids, sort, vm) = ids_and_cost(
                    conn,
                    if baseline { BASELINE_SQL } else { UNHANDLED_IDS_SQL },
                    "peer",
                );
                assert_eq!(ids.len(), 100_000);
                assert_eq!(ids.first().map(String::as_str), Some(format!("{:064x}", 1).as_str()));
                assert_eq!(
                    ids.last().map(String::as_str),
                    Some(format!("{:064x}", 100_000).as_str())
                );
                steps += vm as u64;
                sorts += sort as u64;
                Ok(())
            })
            .expect("query");
        let elapsed = tick.elapsed();
        let rss = counters("/proc/self/status").get("VmRSS").copied().expect("RSS");
        rss_min = rss_min.min(rss);
        rss_max = rss_max.max(rss);
        total_ns += elapsed.as_nanos();
        max_ns = max_ns.max(elapsed.as_nanos());
        // Equal offered rate for each phase, and enough opportunities for WAL
        // readers to end between calls. No cache eviction or pragma changes.
        if round + 1 < rounds {
            std::thread::sleep(Duration::from_millis(500).saturating_sub(elapsed));
        }
    }
    assert_eq!(sorts, if baseline { rounds as u64 } else { 0 });
    let after = counters("/proc/self/io");
    let delta: BTreeMap<_, _> = before
        .iter()
        .map(|(key, value)| (key, after.get(key).copied().unwrap_or(0).saturating_sub(*value)))
        .collect();
    let status = counters("/proc/self/status");
    println!(
        "{}",
        serde_json::json!({
            "phase": if baseline { "baseline" } else { "candidate" }, "rounds": rounds,
            "elapsed_ms": started.elapsed().as_millis(), "query_ns_total": total_ns,
            "query_max_ns": max_ns, "vm_steps": steps, "sorts": sorts, "io_delta": delta,
            "rss_kib": status.get("VmRSS"), "rss_min_kib": rss_min, "rss_max_kib": rss_max, "swap_kib": status.get("VmSwap"),
            "cpu_stat_before": cpu_before,
            "cpu_stat_after": std::fs::read_to_string("/proc/self/stat").expect("CPU counters"),
        })
    );
}

#[test]
#[ignore = "1.24 GiB Linux synthetic query workload; run explicitly with --ignored --nocapture"]
fn sustained_unhandled_id_query_workload() {
    let rounds = std::env::var("LXMF_663_QUERY_ROUNDS")
        .map(|value| value.parse().expect("rounds"))
        .unwrap_or(120);
    let dir = tempfile::tempdir().expect("fixture directory");
    let path = dir.path().join("synthetic.db");
    {
        let store = MessagesStore::open(&path).expect("store");
        store.with_write_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<100000)
                INSERT INTO propagation_entries(transient_id,destination,payload_hex,received_at,size_bytes)
                SELECT printf('%064x',x),'destination',hex(zeroblob(5120)),100001-x,5120 FROM n;
                INSERT INTO propagation_peer_entries SELECT 'Peer',transient_id,'unhandled',0 FROM propagation_entries;")?;
            tx.commit()?;
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
            Ok(())
        }).expect("synthetic fixture");
        println!(
            "{}",
            serde_json::json!({"fixture_db_bytes": std::fs::metadata(&path).expect("DB metadata").len(), "payload_rows":100000, "payload_hex_bytes_per_row":10240})
        );
        phase(&store, true, rounds);
        phase(&store, false, rounds);
        phase(&store, false, rounds);
        phase(&store, true, rounds);
        // Mutation after readers end must remain visible and durable. This
        // exercises legacy completed variants without rewriting payload data.
        store.with_write_conn(|conn| {
            conn.execute("INSERT INTO propagation_peer_entries SELECT 'PEER',transient_id,'received',0 FROM propagation_entries WHERE received_at%2=0", [])?;
            Ok(())
        }).expect("completed ingress");
        assert_eq!(
            store.list_peer_unhandled_propagation_ids("peer").expect("updated IDs").len(),
            50000
        );
    }
    let reopened = MessagesStore::open(&path).expect("reopen");
    assert_eq!(
        reopened.list_peer_unhandled_propagation_ids("peer").expect("durable IDs").len(),
        50000
    );
    reopened
        .with_read_conn(|conn| {
            assert_eq!(
                ids_and_cost(conn, BASELINE_SQL, "peer").0,
                ids_and_cost(conn, UNHANDLED_IDS_SQL, "peer").0
            );
            Ok(())
        })
        .expect("reopened baseline agreement");
}
