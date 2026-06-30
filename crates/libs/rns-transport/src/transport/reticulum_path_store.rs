use super::*;
use crate::transport::reticulum_announce_cache::{CachedAnnounce, ReticulumAnnounceCache};
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReticulumPathTableRestoreReport {
    pub restored_active_paths: usize,
    pub restored_identities: Vec<RestoredReticulumPathIdentity>,
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
        if self.handler.lock().await.config.connected_to_shared_instance {
            return Ok(0);
        }

        let storage_path = storage_path.as_ref().to_path_buf();
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
        announce_cache.ensure_dir().await?;

        for (packet_hash, iface, packet) in packets {
            announce_cache.write(packet_hash, iface, packet).await?;
        }

        tokio::fs::write(storage_path.join("destination_table"), payload).await?;
        tokio::fs::write(storage_path.join("tunnels"), tunnel_payload).await?;
        Ok(kept_entries.len())
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
                    continue;
                };
                entry.iface = iface;
                mapped_entries.push(entry);
            }
        }

        let mut path_candidates = Vec::new();
        for entry in mapped_entries {
            if python_path_entry_expired(&entry, now_unix_secs) {
                continue;
            }
            if let Some(cached) = announce_cache.restore(entry.packet_hash).await? {
                path_candidates.push(PathRestoreCandidate { entry, cached });
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
            tunnel.paths.retain(|path| !python_tunnel_path_entry_expired(path, now_unix_secs));
        }
        tunnels.retain(|entry| !entry.paths.is_empty());

        let mut tunnel_announces = HashMap::new();
        for tunnel in &tunnels {
            for path in &tunnel.paths {
                if tunnel_announces.contains_key(&path.packet_hash) {
                    continue;
                }
                if let Some(cached) = announce_cache.restore(path.packet_hash).await? {
                    if cached.packet.destination != path.destination
                        || cached.destination.desc.address_hash != path.destination
                    {
                        continue;
                    }
                    let iface = path
                        .interface_hash
                        .map(|hash| AddressHash::new_from_hash(&hash))
                        .unwrap_or(path.destination);
                    tunnel_announces.insert(path.packet_hash, (cached, iface));
                }
            }
        }

        let mut report = ReticulumPathTableRestoreReport::default();
        let mut handler = self.handler.lock().await;

        for candidate in path_candidates {
            if !cached_announce_compatible(
                &handler,
                &candidate.cached.packet,
                &candidate.cached.destination,
                candidate.entry.destination,
            ) {
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
