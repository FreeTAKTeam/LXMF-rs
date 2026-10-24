//! Metrics providers owned by the daemon binary for issue 663: announce/path
//! persistence costs and transport table occupancy. Counts only; no
//! destinations, packet bytes or file names.

use rns_rpc::MetricsProvider;
use rns_transport::transport::{PathTableSaveReport, Transport};
use serde_json::{json, Value as JsonValue};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Debug, Default)]
pub(crate) struct PersistenceMetrics {
    runs_total: AtomicU64,
    errors_total: AtomicU64,
    triggers_total: AtomicU64,
    coalesced_triggers_total: AtomicU64,
    dropped_triggers_total: AtomicU64,
    records_examined_total: AtomicU64,
    skipped_known_total: AtomicU64,
    verified_unchanged_total: AtomicU64,
    written_total: AtomicU64,
    full_verifications_total: AtomicU64,
    duration_ns_total: AtomicU64,
    last_duration_ns: AtomicU64,
    last_kept_entries: AtomicU64,
    last_tunnel_paths: AtomicU64,
    last_fingerprint_map_len: AtomicU64,
    last_run_unix_secs: AtomicU64,
}

impl PersistenceMetrics {
    pub(crate) fn record_trigger(&self) {
        self.triggers_total.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_dropped_trigger(&self) {
        self.dropped_triggers_total.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_coalesced(&self, count: u64) {
        self.coalesced_triggers_total.fetch_add(count, Ordering::Relaxed);
    }

    pub(crate) fn record_error(&self) {
        self.runs_total.fetch_add(1, Ordering::Relaxed);
        self.errors_total.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_report(&self, report: &PathTableSaveReport) {
        let relaxed = Ordering::Relaxed;
        self.runs_total.fetch_add(1, relaxed);
        self.records_examined_total.fetch_add(report.records_examined as u64, relaxed);
        self.skipped_known_total.fetch_add(report.skipped_known as u64, relaxed);
        self.verified_unchanged_total.fetch_add(report.verified_unchanged as u64, relaxed);
        self.written_total.fetch_add(report.written as u64, relaxed);
        if report.full_verification {
            self.full_verifications_total.fetch_add(1, relaxed);
        }
        self.duration_ns_total.fetch_add(report.duration_ns, relaxed);
        self.last_duration_ns.store(report.duration_ns, relaxed);
        self.last_kept_entries.store(report.kept_entries as u64, relaxed);
        self.last_tunnel_paths.store(report.tunnel_paths as u64, relaxed);
        self.last_fingerprint_map_len.store(report.fingerprint_map_len as u64, relaxed);
        self.last_run_unix_secs.store(unix_now_secs(), relaxed);
    }
}

impl MetricsProvider for PersistenceMetrics {
    fn snapshot(&self) -> JsonValue {
        let load = |value: &AtomicU64| value.load(Ordering::Relaxed);
        json!({
            "runs_total": load(&self.runs_total),
            "errors_total": load(&self.errors_total),
            "triggers_total": load(&self.triggers_total),
            "coalesced_triggers_total": load(&self.coalesced_triggers_total),
            "dropped_triggers_total": load(&self.dropped_triggers_total),
            "records_examined_total": load(&self.records_examined_total),
            "skipped_known_total": load(&self.skipped_known_total),
            "verified_unchanged_total": load(&self.verified_unchanged_total),
            "written_total": load(&self.written_total),
            "full_verifications_total": load(&self.full_verifications_total),
            "duration_ns_total": load(&self.duration_ns_total),
            "last_duration_ns": load(&self.last_duration_ns),
            "last_kept_entries": load(&self.last_kept_entries),
            "last_tunnel_paths": load(&self.last_tunnel_paths),
            "last_fingerprint_map_len": load(&self.last_fingerprint_map_len),
            "last_run_unix_secs": load(&self.last_run_unix_secs),
        })
    }
}

/// Transport table sizes, read without blocking the network reactor.
pub(crate) struct TransportTablesProvider {
    transport: Arc<Transport>,
}

impl TransportTablesProvider {
    pub(crate) fn new(transport: Arc<Transport>) -> Self {
        Self { transport }
    }
}

impl MetricsProvider for TransportTablesProvider {
    fn snapshot(&self) -> JsonValue {
        match self.transport.table_occupancy_now() {
            Some(tables) => json!({
                "available": true,
                "announce_retransmit_entries": tables.announce_retransmit_entries,
                "announce_cache_entries": tables.announce_cache_entries,
                "announce_cache_capacity": tables.announce_cache_capacity,
                "path_entries": tables.path_entries,
                "tunnel_entries": tables.tunnel_entries,
                "tunnel_path_entries": tables.tunnel_path_entries,
                "link_entries": tables.link_entries,
                "packet_cache_hashes": tables.packet_cache_hashes,
                "packet_cache_tracked": tables.packet_cache_tracked,
            }),
            None => json!({"available": false, "reason": "transport tables busy"}),
        }
    }
}

fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}
