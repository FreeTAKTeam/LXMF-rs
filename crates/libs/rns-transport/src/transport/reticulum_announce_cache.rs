use super::*;
use rmpv::Value as RmpValue;
use std::io;
use std::path::{Path, PathBuf};
use tokio::io::AsyncReadExt;

pub(super) struct ReticulumAnnounceCache {
    dir: PathBuf,
}

impl ReticulumAnnounceCache {
    pub(super) fn new(storage_path: &Path) -> Self {
        Self { dir: storage_path.join("cache").join("announces") }
    }

    pub(super) async fn ensure_dir(&self) -> io::Result<()> {
        tokio::fs::create_dir_all(&self.dir).await
    }

    pub(super) async fn write(
        &self,
        packet_hash: Hash,
        iface: AddressHash,
        packet: Packet,
    ) -> io::Result<()> {
        let payload = encode_cached_announce(iface, packet)?;
        let path = self.path(packet_hash);
        // Path-table saves revisit every cached announce. Compare exact bytes:
        // a hash alone does not bind the interface reference or mutable headers.
        match tokio::fs::File::open(&path).await {
            Ok(file) => {
                let mut existing = Vec::with_capacity(payload.len() + 1);
                // Bound reads even if an existing cache file is corrupt/oversized.
                file.take((payload.len() + 1) as u64).read_to_end(&mut existing).await?;
                if existing == payload {
                    return Ok(());
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        tokio::fs::write(path, payload).await
    }

    pub(super) async fn restore_classified(
        &self,
        packet_hash: Hash,
    ) -> io::Result<CachedAnnounceRestore> {
        let packet = match self.read_packet(packet_hash).await? {
            CachedAnnouncePacketRestore::Restored(packet) => packet,
            CachedAnnouncePacketRestore::Missing => return Ok(CachedAnnounceRestore::Missing),
            CachedAnnouncePacketRestore::Invalid => return Ok(CachedAnnounceRestore::Invalid),
        };
        if packet.header.packet_type != PacketType::Announce {
            return Ok(CachedAnnounceRestore::Invalid);
        }
        let Ok(announce) = DestinationAnnounce::validate(&packet) else {
            return Ok(CachedAnnounceRestore::Invalid);
        };
        let destination = announce.destination;
        Ok(CachedAnnounceRestore::Restored(Box::new(CachedAnnounce { packet, destination })))
    }

    async fn read_packet(&self, packet_hash: Hash) -> io::Result<CachedAnnouncePacketRestore> {
        let payload = match tokio::fs::read(self.path(packet_hash)).await {
            Ok(payload) => payload,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                return Ok(CachedAnnouncePacketRestore::Missing)
            }
            Err(err) => return Err(err),
        };
        let value: RmpValue = match rmpv::decode::read_value(&mut std::io::Cursor::new(payload)) {
            Ok(value) => value,
            Err(_) => return Ok(CachedAnnouncePacketRestore::Invalid),
        };
        let RmpValue::Array(fields) = value else {
            return Ok(CachedAnnouncePacketRestore::Invalid);
        };
        let Some(raw) = fields.first().and_then(rmp_bytes) else {
            return Ok(CachedAnnouncePacketRestore::Invalid);
        };
        Ok(Packet::from_bytes(raw)
            .map(CachedAnnouncePacketRestore::Restored)
            .unwrap_or(CachedAnnouncePacketRestore::Invalid))
    }

    fn path(&self, packet_hash: Hash) -> PathBuf {
        self.dir.join(hex::encode(packet_hash.as_slice()))
    }
}

pub(super) struct CachedAnnounce {
    pub(super) packet: Packet,
    pub(super) destination: SingleOutputDestination,
}

pub(super) enum CachedAnnounceRestore {
    Restored(Box<CachedAnnounce>),
    Missing,
    Invalid,
}

enum CachedAnnouncePacketRestore {
    Restored(Packet),
    Missing,
    Invalid,
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

fn rmp_bytes(value: &RmpValue) -> Option<&[u8]> {
    match value {
        RmpValue::Binary(bytes) => Some(bytes),
        RmpValue::String(text) => text.as_str().map(str::as_bytes),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
