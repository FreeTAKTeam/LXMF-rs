use super::*;
use rmpv::Value as RmpValue;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

impl Transport {
    pub async fn save_reticulum_path_table<P: AsRef<Path>>(
        &self,
        storage_path: P,
    ) -> io::Result<usize> {
        let storage_path = storage_path.as_ref().to_path_buf();
        let now = std::time::Instant::now();
        let now_unix_secs = now_unix_secs();
        let (entries, tunnel_entries, packets) = {
            let handler = self.handler.lock().await;
            let iface_manager = self.iface_manager.lock().await;
            let entries = handler.path_table.export_python_entries(now, now_unix_secs, |iface| {
                Some((iface_manager.mode(iface)?, iface_manager.full_hash(iface)?))
            });
            let mut kept_entries = Vec::new();
            let mut packets = Vec::new();
            for entry in entries {
                if let Some(packet) =
                    handler.announce_table.packet_for_destination(&entry.destination)
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
                        handler.announce_table.packet_for_destination(&path.destination)
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

        let mut cached_announces = Vec::new();
        for (packet_hash, iface, packet) in packets {
            cached_announces.push((packet_hash, encode_cached_announce(iface, packet)?));
        }

        let payload = PathTable::encode_python_entries(&entries)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "encode path table"))?;
        let tunnel_payload = TunnelTable::encode_python_entries(&tunnel_entries)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "encode tunnel table"))?;

        tokio::fs::create_dir_all(&storage_path).await?;
        let announce_cache_dir = storage_path.join("cache").join("announces");
        tokio::fs::create_dir_all(&announce_cache_dir).await?;

        for (packet_hash, payload) in cached_announces {
            tokio::fs::write(cached_announce_path(&announce_cache_dir, packet_hash), payload)
                .await?;
        }

        tokio::fs::write(storage_path.join("destination_table"), payload).await?;
        tokio::fs::write(storage_path.join("tunnels"), tunnel_payload).await?;
        Ok(entries.len())
    }

    pub async fn restore_reticulum_path_table<P: AsRef<Path>>(
        &self,
        storage_path: P,
    ) -> io::Result<usize> {
        let storage_path = storage_path.as_ref().to_path_buf();
        let path = storage_path.join("destination_table");
        let announce_cache_dir = storage_path.join("cache").join("announces");
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
            if let Some(cached) =
                restore_cached_announce(&announce_cache_dir, entry.packet_hash).await?
            {
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

        let mut tunnel_announces = HashMap::new();
        for tunnel in &tunnels {
            for path in &tunnel.paths {
                if tunnel_announces.contains_key(&path.packet_hash) {
                    continue;
                }
                if let Some(cached) =
                    restore_cached_announce(&announce_cache_dir, path.packet_hash).await?
                {
                    tunnel_announces.insert(path.packet_hash, cached);
                }
            }
        }

        let mut restored = 0usize;
        let mut handler = self.handler.lock().await;

        for candidate in path_candidates {
            if !cached_announce_compatible(
                &handler,
                &candidate.cached.packet,
                &candidate.cached.destination,
            ) {
                continue;
            }
            let dest_hash = candidate.cached.destination.desc.address_hash;
            handler
                .single_out_destinations
                .entry(candidate.cached.packet.destination)
                .or_insert_with(|| Arc::new(Mutex::new(candidate.cached.destination)));
            handler.announce_table.add(&candidate.cached.packet, dest_hash, candidate.entry.iface);
            handler.path_table.restore_python_entry(candidate.entry, now, now_unix_secs);
            restored += 1;
        }

        let mut valid_tunnel_announces = HashSet::new();
        for (packet_hash, cached) in tunnel_announces {
            if !cached_announce_compatible(&handler, &cached.packet, &cached.destination) {
                continue;
            }
            handler
                .single_out_destinations
                .entry(cached.packet.destination)
                .or_insert_with(|| Arc::new(Mutex::new(cached.destination)));
            valid_tunnel_announces.insert(packet_hash);
        }

        for tunnel in &mut tunnels {
            tunnel.paths.retain(|path| valid_tunnel_announces.contains(&path.packet_hash));
        }
        tunnels.retain(|entry| !entry.paths.is_empty());
        if !tunnels.is_empty() {
            handler.tunnel_table.restore_python_entries(tunnels, now, now_unix_secs);
        }

        Ok(restored)
    }
}

struct PathRestoreCandidate {
    entry: super::path_table::PythonPathEntry,
    cached: CachedAnnounce,
}

struct CachedAnnounce {
    packet: Packet,
    destination: SingleOutputDestination,
}

fn cached_announce_compatible(
    handler: &TransportHandler,
    packet: &Packet,
    destination: &SingleOutputDestination,
) -> bool {
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

async fn restore_cached_announce(
    announce_cache_dir: &Path,
    packet_hash: Hash,
) -> io::Result<Option<CachedAnnounce>> {
    let Some(packet) = read_cached_announce(announce_cache_dir, packet_hash).await? else {
        return Ok(None);
    };
    if packet.header.packet_type != PacketType::Announce {
        return Ok(None);
    }
    let Ok(announce) = DestinationAnnounce::validate(&packet) else {
        return Ok(None);
    };
    Ok(Some(CachedAnnounce { packet, destination: announce.destination }))
}

fn encode_cached_announce(iface: AddressHash, packet: Packet) -> io::Result<Vec<u8>> {
    let raw = packet
        .to_bytes()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "encode cached announce"))?;
    let value =
        RmpValue::Array(vec![RmpValue::Binary(raw), RmpValue::String(iface.to_string().into())]);
    let mut payload = Vec::new();
    rmpv::encode::write_value(&mut payload, &value)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "encode cached announce"))?;
    Ok(payload)
}

async fn read_cached_announce(
    announce_cache_dir: &Path,
    packet_hash: Hash,
) -> io::Result<Option<Packet>> {
    let path = cached_announce_path(announce_cache_dir, packet_hash);
    let payload = match tokio::fs::read(path).await {
        Ok(payload) => payload,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    let value: RmpValue = rmpv::decode::read_value(&mut std::io::Cursor::new(payload))
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "decode cached announce"))?;
    let RmpValue::Array(fields) = value else {
        return Ok(None);
    };
    let Some(raw) = fields.first().and_then(rmp_bytes) else {
        return Ok(None);
    };
    Packet::from_bytes(raw)
        .map(Some)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "decode cached announce packet"))
}

fn rmp_bytes(value: &RmpValue) -> Option<&[u8]> {
    match value {
        RmpValue::Binary(bytes) => Some(bytes),
        RmpValue::String(text) => text.as_str().map(str::as_bytes),
        _ => None,
    }
}

fn cached_announce_path(announce_cache_dir: &Path, packet_hash: Hash) -> PathBuf {
    announce_cache_dir.join(hex::encode(packet_hash.as_slice()))
}

fn now_unix_secs() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64()
}
