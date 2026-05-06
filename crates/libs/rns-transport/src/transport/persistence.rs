use super::*;
use rmpv::Value as RmpValue;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

impl Transport {
    pub async fn save_reticulum_path_table<P: AsRef<Path>>(
        &self,
        storage_path: P,
    ) -> io::Result<usize> {
        let storage_path = storage_path.as_ref();
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

        fs::create_dir_all(storage_path)?;
        let announce_cache_dir = storage_path.join("cache").join("announces");
        fs::create_dir_all(&announce_cache_dir)?;

        for (packet_hash, iface, packet) in packets {
            write_cached_announce(&announce_cache_dir, packet_hash, iface, packet)?;
        }

        let payload = PathTable::encode_python_entries(&entries)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "encode path table"))?;
        fs::write(storage_path.join("destination_table"), payload)?;
        let tunnel_payload = TunnelTable::encode_python_entries(&tunnel_entries)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "encode tunnel table"))?;
        fs::write(storage_path.join("tunnels"), tunnel_payload)?;
        Ok(entries.len())
    }

    pub async fn restore_reticulum_path_table<P: AsRef<Path>>(
        &self,
        storage_path: P,
    ) -> io::Result<usize> {
        let storage_path = storage_path.as_ref();
        let path = storage_path.join("destination_table");
        let announce_cache_dir = storage_path.join("cache").join("announces");
        let now = std::time::Instant::now();
        let now_unix_secs = now_unix_secs();
        let mut restored = 0usize;

        let mut handler = self.handler.lock().await;
        let iface_manager = self.iface_manager.lock().await;

        if path.exists() {
            let payload = fs::read(path)?;
            let entries = PathTable::decode_python_entries(&payload)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "decode path table"))?;
            for mut entry in entries {
                let Some(iface) = iface_manager.address_for_full_hash(&entry.interface_hash) else {
                    continue;
                };
                entry.iface = iface;
                let Some((packet, destination)) =
                    restore_cached_announce(&announce_cache_dir, entry.packet_hash, &handler)?
                else {
                    continue;
                };
                let dest_hash = destination.desc.address_hash;
                handler
                    .single_out_destinations
                    .entry(packet.destination)
                    .or_insert_with(|| Arc::new(Mutex::new(destination)));
                handler.announce_table.add(&packet, dest_hash, entry.iface);
                handler.path_table.restore_python_entry(entry, now, now_unix_secs);
                restored += 1;
            }
        }

        let tunnel_path = storage_path.join("tunnels");
        if tunnel_path.exists() {
            let payload = fs::read(tunnel_path)?;
            let mut tunnels = TunnelTable::decode_python_entries(&payload)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "decode tunnel table"))?;
            for tunnel in &mut tunnels {
                tunnel.paths.retain(|path| {
                    let Ok(Some((packet, destination))) =
                        restore_cached_announce(&announce_cache_dir, path.packet_hash, &handler)
                    else {
                        return false;
                    };
                    handler
                        .single_out_destinations
                        .entry(packet.destination)
                        .or_insert_with(|| Arc::new(Mutex::new(destination)));
                    true
                });
            }
            tunnels.retain(|entry| !entry.paths.is_empty());
            handler.tunnel_table.restore_python_entries(tunnels, now, now_unix_secs);
        }

        Ok(restored)
    }
}

fn restore_cached_announce(
    announce_cache_dir: &Path,
    packet_hash: Hash,
    handler: &TransportHandler,
) -> io::Result<Option<(Packet, SingleOutputDestination)>> {
    let Some(packet) = read_cached_announce(announce_cache_dir, packet_hash)? else {
        return Ok(None);
    };
    if packet.header.packet_type != PacketType::Announce {
        return Ok(None);
    }
    let Ok(announce) = DestinationAnnounce::validate(&packet) else {
        return Ok(None);
    };
    if let Some(existing) = handler.single_out_destinations.get(&packet.destination) {
        let Ok(existing) = existing.try_lock() else {
            return Ok(None);
        };
        if existing.identity.public_key != announce.destination.identity.public_key
            || existing.identity.verifying_key != announce.destination.identity.verifying_key
        {
            return Ok(None);
        }
    }
    Ok(Some((packet, announce.destination)))
}

fn write_cached_announce(
    announce_cache_dir: &Path,
    packet_hash: Hash,
    iface: AddressHash,
    packet: Packet,
) -> io::Result<()> {
    let raw = packet
        .to_bytes()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "encode cached announce"))?;
    let value =
        RmpValue::Array(vec![RmpValue::Binary(raw), RmpValue::String(iface.to_string().into())]);
    let mut payload = Vec::new();
    rmpv::encode::write_value(&mut payload, &value)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "encode cached announce"))?;
    fs::write(cached_announce_path(announce_cache_dir, packet_hash), payload)
}

fn read_cached_announce(
    announce_cache_dir: &Path,
    packet_hash: Hash,
) -> io::Result<Option<Packet>> {
    let path = cached_announce_path(announce_cache_dir, packet_hash);
    if !path.exists() {
        return Ok(None);
    }
    let payload = fs::read(path)?;
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
