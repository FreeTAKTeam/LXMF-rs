use super::*;
use crate::transport::packet_disk_cache::ReticulumPacketDiskCache;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

// shortcut: retention is reference/age-based; add a byte budget if ingress outpaces sweeps.
const ANNOUNCE_CACHE_CLEAN_BATCH: usize = 512;
const ANNOUNCE_CACHE_CLEAN_GRACE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AnnounceCacheCleanupReport {
    pub examined: usize,
    pub removed: usize,
    pub bytes_removed: u64,
    pub scan_complete: bool,
    pub duration_ns: u64,
}

impl Transport {
    /// Continue one bounded cleanup batch using references from the last
    /// successfully persisted active-route and tunnel snapshots.
    pub async fn clean_reticulum_announce_cache_batch<P: AsRef<Path>>(
        &self,
        storage_path: P,
    ) -> io::Result<Option<AnnounceCacheCleanupReport>> {
        let _save_permit = self
            .path_table_persistence
            .save_gate
            .acquire()
            .await
            .map_err(|_| io::Error::other("Reticulum path-table persistence gate closed"))?;
        if self.handler.lock().await.config.connected_to_shared_instance {
            return Ok(None);
        }
        let storage_path = storage_path.as_ref().to_path_buf();
        let mut cache_state =
            self.path_table_persistence.state.lock().await.take().unwrap_or_default();
        if cache_state.storage_root.as_ref() != Some(&storage_path)
            || !cache_state.persisted_snapshot_matches_files
        {
            *self.path_table_persistence.state.lock().await = Some(cache_state);
            return Ok(None);
        }
        let result = clean_announce_cache_batch_for_fingerprints(
            &storage_path,
            &cache_state.files,
            &mut cache_state.cleanup_scan,
        )
        .await;
        *self.path_table_persistence.state.lock().await = Some(cache_state);
        result.map(Some)
    }
}

pub(super) async fn clean_announce_cache_batch(
    storage_path: &Path,
    retained_hashes: &HashSet<Hash>,
    scan: &mut Option<tokio::fs::ReadDir>,
) -> io::Result<AnnounceCacheCleanupReport> {
    let started = Instant::now();
    let (examined, removed, bytes_removed, scan_complete) =
        ReticulumPacketDiskCache::new(storage_path)
            .clean_announces_batch_for_hashes(
                retained_hashes,
                scan,
                ANNOUNCE_CACHE_CLEAN_BATCH,
                ANNOUNCE_CACHE_CLEAN_GRACE,
            )
            .await?;
    Ok(AnnounceCacheCleanupReport {
        examined,
        removed,
        bytes_removed,
        scan_complete,
        duration_ns: started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64,
    })
}

async fn clean_announce_cache_batch_for_fingerprints(
    storage_path: &Path,
    fingerprints: &HashMap<Hash, [u8; 32]>,
    scan: &mut Option<tokio::fs::ReadDir>,
) -> io::Result<AnnounceCacheCleanupReport> {
    let started = Instant::now();
    let (examined, removed, bytes_removed, scan_complete) =
        ReticulumPacketDiskCache::new(storage_path)
            .clean_announces_batch_for_fingerprints(
                fingerprints,
                scan,
                ANNOUNCE_CACHE_CLEAN_BATCH,
                ANNOUNCE_CACHE_CLEAN_GRACE,
            )
            .await?;
    Ok(AnnounceCacheCleanupReport {
        examined,
        removed,
        bytes_removed,
        scan_complete,
        duration_ns: started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64,
    })
}
