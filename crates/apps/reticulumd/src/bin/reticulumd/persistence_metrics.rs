//! Metrics providers owned by the daemon binary for issues 663/709: requested
//! Rust heap bytes, persistence costs and transport table occupancy. No
//! destinations, packet bytes or file names.

use super::bridge::PeerCrypto;
use rns_rpc::MetricsProvider;
use rns_transport::hash::AddressHash;
use rns_transport::transport::{AnnounceCacheCleanupReport, PathTableSaveReport, Transport};
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex;

pub(crate) struct HeapMetricsProvider;

impl MetricsProvider for HeapMetricsProvider {
    fn snapshot(&self) -> JsonValue {
        // Requested sizes exclude native allocations and allocator overhead/residency.
        json!({"rust_allocated_bytes": super::GLOBAL_ALLOCATOR.allocated()})
    }
}

#[derive(Debug, Default)]
pub(crate) struct PersistenceMetrics {
    in_flight: AtomicU64,
    runs_total: AtomicU64,
    errors_total: AtomicU64,
    cleanup_errors_total: AtomicU64,
    triggers_total: AtomicU64,
    coalesced_triggers_total: AtomicU64,
    dropped_triggers_total: AtomicU64,
    records_examined_total: AtomicU64,
    skipped_known_total: AtomicU64,
    verified_unchanged_total: AtomicU64,
    written_total: AtomicU64,
    full_verifications_total: AtomicU64,
    cleanup_examined_total: AtomicU64,
    cleanup_removed_total: AtomicU64,
    cleanup_bytes_removed_total: AtomicU64,
    cleanup_scans_completed_total: AtomicU64,
    cleanup_duration_ns_total: AtomicU64,
    cleanup_last_duration_ns: AtomicU64,
    cleanup_last_success_unix_secs: AtomicU64,
    duration_ns_total: AtomicU64,
    last_duration_ns: AtomicU64,
    last_kept_entries: AtomicU64,
    last_tunnel_paths: AtomicU64,
    last_fingerprint_map_len: AtomicU64,
    last_run_unix_secs: AtomicU64,
}

impl PersistenceMetrics {
    pub(crate) fn begin_in_flight(&self) {
        self.in_flight.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn end_in_flight(&self) {
        self.in_flight.fetch_sub(1, Ordering::Relaxed);
    }

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

    pub(crate) fn record_cleanup_error(&self) {
        self.cleanup_errors_total.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_report(&self, report: &PathTableSaveReport) {
        let relaxed = Ordering::Relaxed;
        self.runs_total.fetch_add(1, relaxed);
        self.records_examined_total.fetch_add(report.records_examined as u64, relaxed);
        self.skipped_known_total.fetch_add(report.skipped_known as u64, relaxed);
        self.verified_unchanged_total.fetch_add(report.verified_unchanged as u64, relaxed);
        self.written_total.fetch_add(report.written as u64, relaxed);
        self.record_cleanup(AnnounceCacheCleanupReport {
            examined: report.cleanup_examined,
            removed: report.cleanup_removed,
            bytes_removed: report.cleanup_bytes_removed,
            scan_complete: report.cleanup_scan_complete,
            duration_ns: report.cleanup_duration_ns,
        });
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

    pub(crate) fn record_cleanup(&self, report: AnnounceCacheCleanupReport) {
        let relaxed = Ordering::Relaxed;
        self.cleanup_examined_total.fetch_add(report.examined as u64, relaxed);
        self.cleanup_removed_total.fetch_add(report.removed as u64, relaxed);
        self.cleanup_bytes_removed_total.fetch_add(report.bytes_removed, relaxed);
        if report.scan_complete {
            self.cleanup_scans_completed_total.fetch_add(1, relaxed);
        }
        self.cleanup_duration_ns_total.fetch_add(report.duration_ns, relaxed);
        self.cleanup_last_duration_ns.store(report.duration_ns, relaxed);
        if report.examined > 0 || report.scan_complete {
            self.cleanup_last_success_unix_secs.store(unix_now_secs(), relaxed);
        }
    }
}

impl MetricsProvider for PersistenceMetrics {
    fn snapshot(&self) -> JsonValue {
        let load = |value: &AtomicU64| value.load(Ordering::Relaxed);
        json!({
            "in_flight": load(&self.in_flight) > 0,
            "runs_total": load(&self.runs_total),
            "errors_total": load(&self.errors_total),
            "cleanup_errors_total": load(&self.cleanup_errors_total),
            "triggers_total": load(&self.triggers_total),
            "coalesced_triggers_total": load(&self.coalesced_triggers_total),
            "dropped_triggers_total": load(&self.dropped_triggers_total),
            "records_examined_total": load(&self.records_examined_total),
            "skipped_known_total": load(&self.skipped_known_total),
            "verified_unchanged_total": load(&self.verified_unchanged_total),
            "written_total": load(&self.written_total),
            "full_verifications_total": load(&self.full_verifications_total),
            "cleanup_examined_total": load(&self.cleanup_examined_total),
            "cleanup_removed_total": load(&self.cleanup_removed_total),
            "cleanup_bytes_removed_total": load(&self.cleanup_bytes_removed_total),
            "cleanup_scans_completed_total": load(&self.cleanup_scans_completed_total),
            "cleanup_duration_ns_total": load(&self.cleanup_duration_ns_total),
            "cleanup_last_duration_ns": load(&self.cleanup_last_duration_ns),
            "cleanup_last_success_unix_secs": load(&self.cleanup_last_success_unix_secs),
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
    peers: Arc<Mutex<HashMap<String, PeerCrypto>>>,
    peers_removed_total: AtomicU64,
}

impl TransportTablesProvider {
    pub(crate) fn new(
        transport: Arc<Transport>,
        peers: Arc<Mutex<HashMap<String, PeerCrypto>>>,
    ) -> Self {
        Self { transport, peers, peers_removed_total: AtomicU64::new(0) }
    }

    pub(crate) fn prune_peers(&self) {
        let mut peers = match self.peers.lock() {
            Ok(peers) => peers,
            Err(error) => {
                log::error!("[daemon] peer identity cache cleanup lock poisoned: {error}");
                return;
            }
        };
        let before = peers.len();
        peers.retain(|peer, _| {
            let mut bytes = [0; 16];
            if let Err(error) = hex::decode_to_slice(peer, &mut bytes) {
                log::warn!("[daemon] malformed destination in peer identity cache: {error}");
                return false;
            }
            self.transport.destination_is_cached_now(&AddressHash::new(bytes)) != Some(false)
        });
        self.peers_removed_total.fetch_add((before - peers.len()) as u64, Ordering::Relaxed);
    }
}

impl MetricsProvider for TransportTablesProvider {
    fn snapshot(&self) -> JsonValue {
        let peer_crypto = self.peers.try_lock().ok().map(|peers| peers.len());
        match self.transport.table_occupancy_now() {
            Some(tables) => json!({
                "available": true,
                "announce_retransmit_entries": tables.announce_retransmit_entries,
                "announce_cache_entries": tables.announce_cache_entries,
                "announce_cache_capacity": tables.announce_cache_capacity,
                "path_entries": tables.path_entries,
                "single_out_destinations": tables.single_out_destinations,
                "single_out_destinations_removed_total": tables.single_out_destinations_removed_total,
                "random_blob_count": tables.random_blob_count,
                "fingerprint_map_len": tables.fingerprint_map_len,
                "peer_crypto": peer_crypto,
                "peer_crypto_removed_total": self.peers_removed_total.load(Ordering::Relaxed),
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

#[cfg(test)]
mod tests {
    use super::*;
    use rns_transport::destination::{DestinationName, SingleOutputDestination};
    use rns_transport::identity::PrivateIdentity;
    use rns_transport::packet::Packet;
    use rns_transport::transport::TransportConfig;

    #[test]
    fn heap_requested_bytes_appear_in_registered_metrics() {
        let daemon = rns_rpc::RpcDaemon::with_store(
            rns_rpc::MessagesStore::in_memory().expect("in-memory store"),
            "heap-metrics-test".into(),
        );
        daemon.register_metrics_provider("heap", Arc::new(HeapMetricsProvider));
        let snapshot = daemon.metrics_snapshot();
        assert!(snapshot["providers"]["heap"]["rust_allocated_bytes"]
            .as_u64()
            .is_some_and(|bytes| bytes > 0));
    }

    #[tokio::test]
    async fn peer_cleanup_and_provider_keep_only_cached_destinations_without_blocking() {
        let local = PrivateIdentity::new_from_name("peer-cache-local");
        let transport = Arc::new(Transport::new(TransportConfig::new("peer-cache", &local, true)));
        let _channel = transport.iface_manager().lock().await.new_channel(16);
        let remote = PrivateIdentity::new_from_name("peer-cache-live");
        let name = DestinationName::new("lxmf", "delivery");
        let address = SingleOutputDestination::new(*remote.as_identity(), name).desc.address_hash;
        transport
            .send_prepared_single_packet_observed_with_trace(
                Packet { destination: address, ..Packet::default() },
                *remote.as_identity(),
                name,
                |_| {},
            )
            .await
            .expect("prepared recipient");
        let peers = Arc::new(Mutex::new(HashMap::from([
            (hex::encode(address.as_slice()), PeerCrypto { identity: *remote.as_identity() }),
            (hex::encode([0x77; 16]), PeerCrypto { identity: *local.as_identity() }),
        ])));
        let metrics = TransportTablesProvider::new(transport, peers.clone());
        assert_eq!(metrics.snapshot()["peer_crypto"], 2);
        metrics.prune_peers();
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot["peer_crypto"], 1);
        assert_eq!(snapshot["peer_crypto_removed_total"], 1);
        assert_eq!(snapshot["single_out_destinations"], 1);
        let _guard = peers.lock().expect("peers");
        assert!(metrics.snapshot()["peer_crypto"].is_null(), "busy cache is unavailable");
    }
}
