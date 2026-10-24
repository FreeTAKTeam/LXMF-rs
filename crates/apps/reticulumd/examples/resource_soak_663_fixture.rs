//! Production-shaped daemon storage fixture for the issue #663 resource soak.
//!
//! Generates, under one root directory, a durable-broker `reticulum.db` with a
//! large propagation inventory and peer history, a deliberately large WAL, and a
//! Reticulum storage root (`cache/announces/*`, `destination_table`, `tunnels`)
//! holding many valid signed announces. Everything is synthetic: seeded RNG,
//! fresh identities, no production data.

use clap::Parser;
use rand_core::OsRng;
use rns_rpc::{AnnounceRecord, MessagesStore};
use rns_transport::destination::{DestinationName, SingleInputDestination};
use rns_transport::identity::PrivateIdentity;
use rns_transport::iface::{IfaceSource, RxMessage};
use rns_transport::transport::{Transport, TransportConfig};
use rusqlite::{params, Connection};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(name = "resource_soak_663_fixture")]
struct Args {
    /// Directory that receives `reticulum.db`, its WAL and the Reticulum storage root.
    #[arg(long)]
    root: PathBuf,
    /// Propagation inventory rows. About 600k rows of 2 KiB reach the ~1.1 GiB production size.
    #[arg(long, default_value_t = 300_000)]
    entries: u64,
    /// Payload bytes per propagation entry (stored hex-encoded, so disk use is about double).
    #[arg(long, default_value_t = 2_048)]
    payload_bytes: usize,
    /// Propagation peers with history; half of them get mixed-case key variants.
    #[arg(long, default_value_t = 300)]
    peers: u64,
    /// Peer-history rows per peer (handled, received and unhandled marks).
    #[arg(long, default_value_t = 2_000)]
    peer_history: u64,
    /// Cached Reticulum announces to write under `cache/announces`.
    #[arg(long, default_value_t = 100_000)]
    announces: u64,
    /// Durable broker budget recorded in the database (production uses 6 GiB).
    #[arg(long, default_value_t = 6 * 1024 * 1024 * 1024)]
    budget_bytes: i64,
    /// Deterministic seed for identifiers and payloads.
    #[arg(long, default_value_t = 663)]
    seed: u64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    std::fs::create_dir_all(&args.root)?;
    let started = Instant::now();
    let db_path = args.root.join("reticulum.db");
    if db_path.exists() {
        return Err(format!("{} already exists; refusing to overwrite", db_path.display()).into());
    }

    let db = build_database(&args, &db_path)?;
    let announces = build_announce_root(&args)?;

    let manifest = json!({
        "schema": "lxmf-rs-resource-soak-663-fixture-v1",
        "seed": args.seed,
        "git_revision": git_revision(),
        "elapsed_secs": started.elapsed().as_secs_f64(),
        "database": db,
        "announce_root": announces,
    });
    let rendered = serde_json::to_string_pretty(&manifest)?;
    std::fs::write(args.root.join("fixture.json"), format!("{rendered}\n"))?;
    println!("{rendered}");
    Ok(())
}

fn build_database(
    args: &Args,
    db_path: &Path,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let store = MessagesStore::open(db_path)?;
    store.enable_durable_broker(args.budget_bytes)?;
    let mut rng = SplitMix::new(args.seed);
    let peers: Vec<String> = (0..args.peers).map(|_| hex32(&mut rng)).collect();

    for (index, peer) in peers.iter().enumerate() {
        store.insert_announce(&AnnounceRecord {
            id: format!("fixture-announce-{index}"),
            peer: peer.clone(),
            timestamp: 1_700_000_000 + index as i64,
            name: None,
            name_source: None,
            first_seen: 1_700_000_000,
            seen_count: 1,
            app_data_hex: None,
            capabilities: vec!["propagation".to_string()],
            rssi: None,
            snr: None,
            q: None,
            stamp_cost: None,
            stamp_cost_flexibility: None,
            peering_cost: None,
        })?;
    }
    drop(store);

    // Bulk rows go through a plain connection: the store API round-trips one
    // writer command per row, which is far too slow for 10^5-10^6 inserts.
    let conn = Connection::open(db_path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    // A pinned reader on a second connection keeps SQLite from resetting the
    // WAL, so the file grows the way the production WAL did.
    let pin = Connection::open(db_path)?;
    pin.execute_batch("BEGIN; SELECT COUNT(*) FROM propagation_entries;")?;

    let now = 1_700_000_000i64;
    let payload = vec![0u8; args.payload_bytes];
    let mut transient_ids = Vec::with_capacity(args.entries.min(args.peer_history * 4) as usize);
    let batch = 10_000u64;
    let mut inserted = 0u64;
    while inserted < args.entries {
        let tx = conn.unchecked_transaction()?;
        {
            let mut statement = tx.prepare_cached(
                "INSERT OR IGNORE INTO propagation_entries
                    (transient_id, destination, payload_hex, received_at, size_bytes, stamp_value)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL)",
            )?;
            let end = (inserted + batch).min(args.entries);
            for index in inserted..end {
                let transient_id = hex32(&mut rng);
                let destination = hex32(&mut rng);
                let mut body = payload.clone();
                rng.fill_bytes(&mut body);
                let received_at = now - (args.entries - index) as i64;
                statement.execute(params![
                    transient_id,
                    destination,
                    hex::encode(&body),
                    received_at,
                    args.payload_bytes as i64,
                ])?;
                if transient_ids.len() < transient_ids.capacity() {
                    transient_ids.push(transient_id);
                }
            }
            inserted = end;
        }
        tx.commit()?;
        eprintln!("[fixture] propagation entries {inserted}/{}", args.entries);
    }

    // Peer history: handled and received marks outrank pending ones, and
    // mixed-case peer keys exercise the case-folded indexes.
    let history_rows = seed_peer_history(&conn, &peers, &transient_ids, args.peer_history, now)?;
    pin.execute_batch("ROLLBACK;")?;
    drop(pin);
    drop(conn);

    let db_bytes = std::fs::metadata(db_path)?.len();
    let wal_path = PathBuf::from(format!("{}-wal", db_path.display()));
    let wal_bytes = std::fs::metadata(&wal_path).map(|meta| meta.len()).unwrap_or(0);
    Ok(json!({
        "path": db_path,
        "entries": args.entries,
        "payload_bytes": args.payload_bytes,
        "peers": args.peers,
        "peer_history_rows": history_rows,
        "budget_bytes": args.budget_bytes,
        "db_bytes": db_bytes,
        "wal_bytes": wal_bytes,
    }))
}

fn seed_peer_history(
    conn: &Connection,
    peers: &[String],
    transient_ids: &[String],
    per_peer: u64,
    now: i64,
) -> rusqlite::Result<u64> {
    let mut rows = 0u64;
    let tx = conn.unchecked_transaction()?;
    {
        let mut statement = tx.prepare_cached(
            "INSERT OR IGNORE INTO propagation_peer_entries (peer, transient_id, state, updated_at)
             VALUES (?1, ?2, ?3, ?4)",
        )?;
        for (peer_index, peer) in peers.iter().enumerate() {
            let key = if peer_index % 2 == 0 { peer.clone() } else { peer.to_ascii_uppercase() };
            let offset = (peer_index * 7) % transient_ids.len().max(1);
            for step in 0..per_peer as usize {
                let Some(transient_id) = transient_ids.get((offset + step) % transient_ids.len())
                else {
                    break;
                };
                let state = match step % 10 {
                    0..=6 => "handled",
                    7 | 8 => "received",
                    _ => "unhandled",
                };
                statement.execute(params![key, transient_id, state, now])?;
                rows += 1;
            }
        }
    }
    tx.commit()?;
    Ok(rows)
}

fn build_announce_root(args: &Args) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let root = args.root.clone();
    let total = args.announces;
    let seed = args.seed;
    runtime
        .block_on(async move {
            let signer = PrivateIdentity::new_from_name(&format!("resource-soak-663-{seed}"));
            let mut config = TransportConfig::new("resource-soak-663-fixture", &signer, true);
            config.set_retransmit(true);
            config.set_announce_cache_capacity(total.max(1) as usize + 1024);
            let transport = Transport::new(config);
            let iface_channel = transport.iface_manager().lock().await.new_channel(1024);
            let iface = *iface_channel.address();

            let mut learned = 0u64;
            let mut last_destination = None;
            for index in 0..total {
                let identity = PrivateIdentity::new_from_rand(OsRng);
                let mut destination =
                    SingleInputDestination::new(identity, DestinationName::new("lxmf", "delivery"));
                let announce = destination.announce(OsRng, None).map_err(|err| {
                    std::io::Error::other(format!("announce packet {index}: {err:?}"))
                })?;
                last_destination = Some(announce.destination);
                iface_channel
                    .rx_channel
                    .send(RxMessage { address: iface, packet: announce, source: IfaceSource::None })
                    .await
                    .map_err(|_| std::io::Error::other("transport rx channel closed"))?;
                learned += 1;
                if learned.is_multiple_of(10_000) {
                    eprintln!("[fixture] announces queued {learned}/{total}");
                }
            }
            if let Some(destination) = last_destination {
                for _ in 0..4_000 {
                    if transport.has_path(&destination).await {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                }
                if !transport.has_path(&destination).await {
                    return Err(std::io::Error::other(
                        "transport did not learn the final announce",
                    ));
                }
            }
            let saved = transport.save_reticulum_path_table(&root).await?;
            let cache_files = std::fs::read_dir(root.join("cache").join("announces"))
                .map(|entries| entries.filter_map(Result::ok).count())
                .unwrap_or(0);
            Ok::<_, std::io::Error>(json!({
                "root": root,
                "announces_requested": total,
                "paths_saved": saved,
                "cache_files": cache_files,
                "destination_table_bytes": std::fs::metadata(root.join("destination_table"))
                    .map(|meta| meta.len())
                    .unwrap_or(0),
            }))
        })
        .map_err(Into::into)
}

fn hex32(rng: &mut SplitMix) -> String {
    let mut bytes = [0u8; 16];
    rng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Small deterministic generator (SplitMix64) so fixtures are reproducible
/// without adding a random-number crate to the daemon's dependency set.
struct SplitMix(u64);

impl SplitMix {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn fill_bytes(&mut self, out: &mut [u8]) {
        for chunk in out.chunks_mut(8) {
            let word = self.next_u64().to_le_bytes();
            chunk.copy_from_slice(&word[..chunk.len()]);
        }
    }
}

fn git_revision() -> Option<String> {
    let output = std::process::Command::new("git").args(["rev-parse", "HEAD"]).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}
