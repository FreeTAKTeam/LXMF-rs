use super::*;
use crate::transport::reticulum_announce_cache::{
    CacheWriteOutcome, CachedAnnounce, CachedAnnounceRestore, ReticulumAnnounceCache,
};
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const ANNOUNCE_CACHE_VERIFY_INTERVAL: Duration = Duration::from_secs(300);

pub(super) struct PathTablePersistence {
    save_gate: tokio::sync::Semaphore,
    state: tokio::sync::Mutex<Option<AnnounceCacheState>>,
}

impl Default for PathTablePersistence {
    fn default() -> Self {
        Self { save_gate: tokio::sync::Semaphore::new(1), state: tokio::sync::Mutex::new(None) }
    }
}

#[derive(Default)]
struct AnnounceCacheState {
    storage_root: Option<PathBuf>,
    last_full_verify: Option<Instant>,
    files: HashMap<Hash, [u8; 32]>,
}

impl AnnounceCacheState {
    fn full_verification_due(&self, now: Instant) -> bool {
        self.last_full_verify
            .is_none_or(|last| now.duration_since(last) >= ANNOUNCE_CACHE_VERIFY_INTERVAL)
    }

    fn retain_current_files(&mut self, current_hashes: &HashSet<Hash>) {
        self.files.retain(|hash, _| current_hashes.contains(hash));
    }
}

/// What one path-table save did. Issue 663 needs the examined/skipped/written
/// split to show that unchanged snapshots no longer touch every cache file.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PathTableSaveReport {
    pub kept_entries: usize,
    pub tunnel_paths: usize,
    pub records_examined: usize,
    pub skipped_known: usize,
    pub verified_unchanged: usize,
    pub written: usize,
    pub full_verification: bool,
    pub duration_ns: u64,
    pub fingerprint_map_len: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReticulumPathTableRestoreReport {
    pub restored_active_paths: usize,
    pub restored_identities: Vec<RestoredReticulumPathIdentity>,
    pub skipped: ReticulumPathTableRestoreSkipped,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReticulumPathTableRestoreSkipped {
    pub active_unmapped_interface: usize,
    pub active_expired: usize,
    pub active_missing_cached_announce: usize,
    pub active_invalid_cached_announce: usize,
    pub active_mismatched_cached_announce: usize,
    pub active_identity_conflict: usize,
    pub active_blackholed_identity: usize,
    pub tunnel_duplicate_packet_hash: usize,
    pub tunnel_expired: usize,
    pub tunnel_missing_cached_announce: usize,
    pub tunnel_invalid_cached_announce: usize,
    pub tunnel_mismatched_cached_announce: usize,
    pub tunnel_identity_conflict: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestoredReticulumPathIdentity {
    pub destination: AddressHash,
    pub public_key: [u8; crate::identity::PUBLIC_KEY_LENGTH],
    pub verifying_key: [u8; crate::identity::PUBLIC_KEY_LENGTH],
}

impl Transport {
    pub async fn save_reticulum_path_table<P: AsRef<Path>>(
        &self,
        storage_path: P,
    ) -> io::Result<usize> {
        Ok(self.save_reticulum_path_table_report(storage_path).await?.kept_entries)
    }

    pub async fn save_reticulum_path_table_report<P: AsRef<Path>>(
        &self,
        storage_path: P,
    ) -> io::Result<PathTableSaveReport> {
        let save_started = Instant::now();
        let _save_permit = self
            .path_table_persistence
            .save_gate
            .acquire()
            .await
            .map_err(|_| io::Error::other("Reticulum path-table persistence gate closed"))?;
        if self.handler.lock().await.config.connected_to_shared_instance {
            return Ok(PathTableSaveReport::default());
        }

        let storage_path = storage_path.as_ref().to_path_buf();
        // Errors before the final state restore discard this cache; the next save verifies disk.
        let mut cache_state =
            self.path_table_persistence.state.lock().await.take().unwrap_or_default();
        if cache_state.storage_root.as_ref() != Some(&storage_path) {
            cache_state = AnnounceCacheState {
                storage_root: Some(storage_path.clone()),
                ..AnnounceCacheState::default()
            };
        }
        let now = std::time::Instant::now();
        let now_unix_secs = now_unix_secs();
        let (kept_entries, tunnel_entries, packets) = {
            let handler = self.handler.lock().await;
            let iface_manager = self.iface_manager.lock().await;
            let entries = handler.path_table.export_python_entries(now, now_unix_secs, |iface| {
                Some((iface_manager.mode(iface)?, iface_manager.full_hash(iface)?))
            });
            let mut kept_entries = Vec::new();
            let mut packets = Vec::new();
            for entry in entries {
                if let Some(packet) =
                    handler.announce_table.cached_packet_for_destination(&entry.destination)
                {
                    packets.push((entry.packet_hash, entry.iface, packet));
                    kept_entries.push(entry);
                }
            }
            let mut tunnel_entries =
                handler.tunnel_table.export_python_entries(now, now_unix_secs, |iface| {
                    iface_manager.full_hash(iface)
                });
            for tunnel in &mut tunnel_entries {
                tunnel.paths.retain(|path| {
                    let Some(packet) =
                        handler.announce_table.cached_packet_for_destination(&path.destination)
                    else {
                        return false;
                    };
                    let iface = path
                        .interface_hash
                        .map(|hash| AddressHash::new_from_hash(&hash))
                        .unwrap_or(path.destination);
                    packets.push((path.packet_hash, iface, packet));
                    true
                });
            }
            tunnel_entries.retain(|entry| !entry.paths.is_empty());
            (kept_entries, tunnel_entries, packets)
        };

        let payload = PathTable::encode_python_entries(&kept_entries)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "encode path table"))?;
        let tunnel_payload = TunnelTable::encode_python_entries(&tunnel_entries)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "encode tunnel table"))?;

        tokio::fs::create_dir_all(&storage_path).await?;
        let announce_cache = ReticulumAnnounceCache::new(&storage_path);
        let verify_existing = cache_state.full_verification_due(Instant::now());
        let mut current_hashes = HashSet::with_capacity(packets.len());
        current_hashes.extend(packets.iter().map(|(packet_hash, _, _)| *packet_hash));
        cache_state.retain_current_files(&current_hashes);
        let tunnel_paths = tunnel_entries.iter().map(|entry| entry.paths.len()).sum();
        let mut report = PathTableSaveReport {
            kept_entries: kept_entries.len(),
            tunnel_paths,
            records_examined: packets.len(),
            full_verification: verify_existing,
            ..PathTableSaveReport::default()
        };
        let result = async {
            announce_cache.ensure_dir().await?;
            for (packet_hash, iface, packet) in packets {
                let previous = cache_state.files.get(&packet_hash).copied();
                let (fingerprint, outcome) = announce_cache
                    .write_with_fingerprint(packet_hash, iface, packet, previous, verify_existing)
                    .await?;
                match outcome {
                    CacheWriteOutcome::SkippedKnownBytes => report.skipped_known += 1,
                    CacheWriteOutcome::VerifiedUnchanged => report.verified_unchanged += 1,
                    CacheWriteOutcome::Written => report.written += 1,
                }
                cache_state.files.insert(packet_hash, fingerprint);
            }

            tokio::fs::write(storage_path.join("destination_table"), payload).await?;
            tokio::fs::write(storage_path.join("tunnels"), tunnel_payload).await?;
            if verify_existing {
                cache_state.last_full_verify = Some(Instant::now());
            }
            Ok(())
        }
        .await;
        report.fingerprint_map_len = cache_state.files.len();
        report.duration_ns = save_started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        *self.path_table_persistence.state.lock().await = Some(cache_state);
        result.map(|()| report)
    }

    pub async fn restore_reticulum_path_table<P: AsRef<Path>>(
        &self,
        storage_path: P,
    ) -> io::Result<usize> {
        Ok(self.restore_reticulum_path_table_report(storage_path).await?.restored_active_paths)
    }

    pub async fn restore_reticulum_path_table_report<P: AsRef<Path>>(
        &self,
        storage_path: P,
    ) -> io::Result<ReticulumPathTableRestoreReport> {
        if self.handler.lock().await.config.connected_to_shared_instance {
            return Ok(ReticulumPathTableRestoreReport::default());
        }

        let storage_path = storage_path.as_ref().to_path_buf();
        let path = storage_path.join("destination_table");
        let announce_cache = ReticulumAnnounceCache::new(&storage_path);
        let now = std::time::Instant::now();
        let now_unix_secs = now_unix_secs();

        let mut report = ReticulumPathTableRestoreReport::default();

        let path_entries = match tokio::fs::read(&path).await {
            Ok(payload) => PathTable::decode_python_entries(&payload)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "decode path table"))?,
            Err(err) if err.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(err) => return Err(err),
        };

        let mut mapped_entries = Vec::new();
        {
            let iface_manager = self.iface_manager.lock().await;
            for mut entry in path_entries {
                let Some(iface) = iface_manager.address_for_full_hash(&entry.interface_hash) else {
                    report.skipped.active_unmapped_interface += 1;
                    continue;
                };
                entry.iface = iface;
                mapped_entries.push(entry);
            }
        }

        let mut path_candidates = Vec::new();
        for entry in mapped_entries {
            if python_path_entry_expired(&entry, now_unix_secs) {
                report.skipped.active_expired += 1;
                continue;
            }
            match announce_cache.restore_classified(entry.packet_hash).await? {
                CachedAnnounceRestore::Restored(cached) => {
                    let cached = *cached;
                    if cached.packet.destination != entry.destination
                        || cached.destination.desc.address_hash != entry.destination
                    {
                        report.skipped.active_mismatched_cached_announce += 1;
                    } else {
                        path_candidates.push(PathRestoreCandidate { entry, cached });
                    }
                }
                CachedAnnounceRestore::Missing => {
                    report.skipped.active_missing_cached_announce += 1;
                }
                CachedAnnounceRestore::Invalid => {
                    report.skipped.active_invalid_cached_announce += 1;
                }
            }
        }

        let tunnel_path = storage_path.join("tunnels");
        let mut tunnels = match tokio::fs::read(&tunnel_path).await {
            Ok(payload) => TunnelTable::decode_python_entries(&payload)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "decode tunnel table"))?,
            Err(err) if err.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(err) => return Err(err),
        };
        for tunnel in &mut tunnels {
            let before = tunnel.paths.len();
            tunnel.paths.retain(|path| !python_tunnel_path_entry_expired(path, now_unix_secs));
            report.skipped.tunnel_expired += before.saturating_sub(tunnel.paths.len());
        }
        tunnels.retain(|entry| !entry.paths.is_empty());

        let mut tunnel_announces = HashMap::new();
        for tunnel in &tunnels {
            for path in &tunnel.paths {
                if tunnel_announces.contains_key(&path.packet_hash) {
                    report.skipped.tunnel_duplicate_packet_hash += 1;
                    continue;
                }
                match announce_cache.restore_classified(path.packet_hash).await? {
                    CachedAnnounceRestore::Restored(cached) => {
                        let cached = *cached;
                        if cached.packet.destination != path.destination
                            || cached.destination.desc.address_hash != path.destination
                        {
                            report.skipped.tunnel_mismatched_cached_announce += 1;
                            continue;
                        }
                        let iface = path
                            .interface_hash
                            .map(|hash| AddressHash::new_from_hash(&hash))
                            .unwrap_or(path.destination);
                        tunnel_announces.insert(path.packet_hash, (cached, iface));
                    }
                    CachedAnnounceRestore::Missing => {
                        report.skipped.tunnel_missing_cached_announce += 1;
                    }
                    CachedAnnounceRestore::Invalid => {
                        report.skipped.tunnel_invalid_cached_announce += 1;
                    }
                }
            }
        }

        let mut handler = self.handler.lock().await;

        for candidate in path_candidates {
            if !cached_announce_compatible(
                &handler,
                &candidate.cached.packet,
                &candidate.cached.destination,
                candidate.entry.destination,
            ) {
                report.skipped.active_identity_conflict += 1;
                continue;
            }
            if handler.is_identity_blackholed(&candidate.cached.destination.identity.address_hash) {
                report.skipped.active_blackholed_identity += 1;
                continue;
            }
            let dest_hash = candidate.cached.destination.desc.address_hash;
            report.push_identity(&candidate.cached.destination);
            handler
                .single_out_destinations
                .entry(candidate.cached.packet.destination)
                .or_insert_with(|| Arc::new(Mutex::new(candidate.cached.destination)));
            handler.announce_table.add_cached(
                &candidate.cached.packet,
                dest_hash,
                candidate.entry.iface,
            );
            handler.path_table.restore_python_entry(candidate.entry, now, now_unix_secs);
            report.restored_active_paths += 1;
        }

        let mut valid_tunnel_paths = HashSet::new();
        for (packet_hash, (cached, iface)) in tunnel_announces {
            if !cached_announce_compatible(
                &handler,
                &cached.packet,
                &cached.destination,
                cached.packet.destination,
            ) {
                report.skipped.tunnel_identity_conflict += 1;
                continue;
            }
            let dest_hash = cached.destination.desc.address_hash;
            report.push_identity(&cached.destination);
            handler
                .single_out_destinations
                .entry(cached.packet.destination)
                .or_insert_with(|| Arc::new(Mutex::new(cached.destination)));
            handler.announce_table.add_cached(&cached.packet, dest_hash, iface);
            valid_tunnel_paths.insert((packet_hash, dest_hash));
        }

        for tunnel in &mut tunnels {
            tunnel
                .paths
                .retain(|path| valid_tunnel_paths.contains(&(path.packet_hash, path.destination)));
        }
        tunnels.retain(|entry| !entry.paths.is_empty());
        if !tunnels.is_empty() {
            handler.tunnel_table.restore_python_entries(tunnels, now, now_unix_secs);
        }

        Ok(report)
    }
}

impl ReticulumPathTableRestoreReport {
    fn push_identity(&mut self, destination: &SingleOutputDestination) {
        let restored = RestoredReticulumPathIdentity {
            destination: destination.desc.address_hash,
            public_key: *destination.desc.identity.public_key_bytes(),
            verifying_key: *destination.desc.identity.verifying_key_bytes(),
        };
        if !self
            .restored_identities
            .iter()
            .any(|existing| existing.destination == restored.destination)
        {
            self.restored_identities.push(restored);
        }
    }
}

struct PathRestoreCandidate {
    entry: super::path_table::PythonPathEntry,
    cached: CachedAnnounce,
}

fn cached_announce_compatible(
    handler: &TransportHandler,
    packet: &Packet,
    destination: &SingleOutputDestination,
    expected_destination: AddressHash,
) -> bool {
    if packet.destination != expected_destination
        || destination.desc.address_hash != expected_destination
    {
        return false;
    }
    if let Some(existing) = handler.single_out_destinations.get(&packet.destination) {
        let Ok(existing) = existing.try_lock() else {
            return false;
        };
        if existing.identity.public_key != destination.identity.public_key
            || existing.identity.verifying_key != destination.identity.verifying_key
        {
            return false;
        }
    }
    true
}

fn now_unix_secs() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64()
}

fn python_path_entry_expired(
    entry: &super::path_table::PythonPathEntry,
    now_unix_secs: f64,
) -> bool {
    !entry.expires_secs.is_finite() || entry.expires_secs <= now_unix_secs
}

fn python_tunnel_path_entry_expired(
    entry: &super::tunnels::PythonTunnelPathEntry,
    now_unix_secs: f64,
) -> bool {
    !entry.expires_secs.is_finite() || entry.expires_secs <= now_unix_secs
}

#[cfg(test)]
mod persistence_tests {
    use super::*;

    #[test]
    fn removed_routes_are_pruned_from_the_persistence_cache() {
        let retained = Hash::new_from_slice(b"retained-route");
        let removed = Hash::new_from_slice(b"removed-route");
        let mut state = AnnounceCacheState::default();
        state.files.insert(retained, [1; 32]);
        state.files.insert(removed, [2; 32]);

        state.retain_current_files(&HashSet::from([retained]));

        assert_eq!(state.files.len(), 1);
        assert!(state.files.contains_key(&retained));
        assert!(!state.files.contains_key(&removed));
    }

    #[test]
    fn full_verification_is_rate_limited_to_five_minutes() {
        let now = Instant::now();
        let mut state = AnnounceCacheState::default();
        assert!(state.full_verification_due(now));

        state.last_full_verify =
            Some(now - ANNOUNCE_CACHE_VERIFY_INTERVAL + Duration::from_secs(1));
        assert!(!state.full_verification_due(now));

        state.last_full_verify = Some(now - ANNOUNCE_CACHE_VERIFY_INTERVAL);
        assert!(state.full_verification_due(now));
    }
}
