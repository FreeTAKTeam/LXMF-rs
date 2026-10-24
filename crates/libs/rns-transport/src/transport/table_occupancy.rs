use super::*;

/// Current in-memory population of the transport's route and duplicate tables.
/// Issue 663 needs these next to RSS so anonymous-memory growth can be
/// attributed; values are counts, never addresses or packet contents.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TableOccupancy {
    /// Announces awaiting retransmission.
    pub announce_retransmit_entries: usize,
    /// Cached announce packets (two-generation cache) and its configured capacity.
    pub announce_cache_entries: usize,
    pub announce_cache_capacity: usize,
    pub path_entries: usize,
    pub tunnel_entries: usize,
    pub tunnel_path_entries: usize,
    pub link_entries: usize,
    /// Packet hashes tracked across both duplicate-filter generations.
    pub packet_cache_hashes: usize,
    pub packet_cache_tracked: usize,
}

impl super::tunnels::TunnelTable {
    /// `(tunnels, paths across all tunnels)` for metrics.
    pub(super) fn occupancy(&self) -> (usize, usize) {
        (self.tunnels.len(), self.tunnels.values().map(|entry| entry.paths.len()).sum())
    }
}

impl Transport {
    /// Table sizes without waiting: returns `None` when the handler or packet
    /// cache is locked, so a metrics scrape never stalls the network reactor.
    pub fn table_occupancy_now(&self) -> Option<TableOccupancy> {
        let handler = self.handler.try_lock().ok()?;
        let packet_cache = handler.packet_cache.try_lock().ok()?;
        let (announce_retransmit_entries, announce_cache_entries) =
            handler.announce_table.occupancy();
        let (tunnel_entries, tunnel_path_entries) = handler.tunnel_table.occupancy();
        Some(TableOccupancy {
            announce_retransmit_entries,
            announce_cache_entries,
            announce_cache_capacity: handler.announce_table.cache_capacity(),
            path_entries: handler.path_table.len(),
            tunnel_entries,
            tunnel_path_entries,
            link_entries: handler.link_table.len(),
            packet_cache_hashes: packet_cache.hash_count(),
            packet_cache_tracked: packet_cache.tracked_count(),
        })
    }

    pub async fn table_occupancy(&self) -> TableOccupancy {
        let handler = self.handler.lock().await;
        let packet_cache = handler.packet_cache.lock().await;
        let (announce_retransmit_entries, announce_cache_entries) =
            handler.announce_table.occupancy();
        let (tunnel_entries, tunnel_path_entries) = handler.tunnel_table.occupancy();
        TableOccupancy {
            announce_retransmit_entries,
            announce_cache_entries,
            announce_cache_capacity: handler.announce_table.cache_capacity(),
            path_entries: handler.path_table.len(),
            tunnel_entries,
            tunnel_path_entries,
            link_entries: handler.link_table.len(),
            packet_cache_hashes: packet_cache.hash_count(),
            packet_cache_tracked: packet_cache.tracked_count(),
        }
    }
}
