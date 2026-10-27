use super::*;
use crate::packet::HeaderType;
use rmpv::Value as RmpValue;
use sha2::{Digest, Sha256};
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

    #[cfg(test)]
    pub(super) async fn write(
        &self,
        packet_hash: Hash,
        iface: AddressHash,
        packet: Packet,
    ) -> io::Result<()> {
        self.write_with_fingerprint(packet_hash, iface, &packet, None, true).await.map(|_| ())
    }

    pub(super) async fn write_with_fingerprint(
        &self,
        packet_hash: Hash,
        iface: AddressHash,
        packet: &Packet,
        previous_fingerprint: Option<[u8; 32]>,
        verify_existing: bool,
    ) -> io::Result<([u8; 32], CacheWriteOutcome)> {
        let fingerprint = cached_announce_fingerprint(iface, packet)?;
        if !verify_existing && previous_fingerprint == Some(fingerprint) {
            return Ok((fingerprint, CacheWriteOutcome::SkippedKnownBytes));
        }
        let payload = encode_cached_announce(iface, packet)?;
        let path = self.path(packet_hash);
        // Unknown fingerprints verify disk before taking the unchanged-file fast path.
        match tokio::fs::File::open(&path).await {
            Ok(file) => {
                let mut existing = Vec::with_capacity(payload.len() + 1);
                // Bound reads even if an existing cache file is corrupt/oversized.
                file.take((payload.len() + 1) as u64).read_to_end(&mut existing).await?;
                if existing == payload {
                    return Ok((fingerprint, CacheWriteOutcome::VerifiedUnchanged));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        tokio::fs::write(path, payload).await?;
        Ok((fingerprint, CacheWriteOutcome::Written))
    }

    pub(super) async fn restore_classified(
        &self,
        packet_hash: Hash,
    ) -> io::Result<CachedAnnounceRestore> {
        let (packet, fingerprint) = match self.read_packet(packet_hash).await? {
            CachedAnnouncePacketRestore::Restored(packet, fingerprint) => (packet, fingerprint),
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
        Ok(CachedAnnounceRestore::Restored(Box::new(CachedAnnounce {
            packet,
            destination,
            fingerprint,
        })))
    }

    async fn read_packet(&self, packet_hash: Hash) -> io::Result<CachedAnnouncePacketRestore> {
        let payload = match tokio::fs::read(self.path(packet_hash)).await {
            Ok(payload) => payload,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                return Ok(CachedAnnouncePacketRestore::Missing)
            }
            Err(err) => return Err(err),
        };
        let value: RmpValue = match rmpv::decode::read_value(&mut std::io::Cursor::new(&payload)) {
            Ok(value) => value,
            Err(_) => return Ok(CachedAnnouncePacketRestore::Invalid),
        };
        let RmpValue::Array(fields) = value else {
            return Ok(CachedAnnouncePacketRestore::Invalid);
        };
        let Some(raw) = fields.first().and_then(rmp_bytes) else {
            return Ok(CachedAnnouncePacketRestore::Invalid);
        };
        let Ok(packet) = Packet::from_bytes(raw) else {
            return Ok(CachedAnnouncePacketRestore::Invalid);
        };
        // Seed only canonical bytes, using the interface actually stored on disk.
        let fingerprint = match fields
            .get(1)
            .and_then(RmpValue::as_str)
            .and_then(|value| value.strip_prefix('/')?.strip_suffix('/'))
            .and_then(|value| AddressHash::new_from_hex_string(value).ok())
        {
            Some(iface) if encode_cached_announce(iface, &packet)? == payload => {
                Some(cached_announce_fingerprint(iface, &packet)?)
            }
            _ => None,
        };
        Ok(CachedAnnouncePacketRestore::Restored(packet, fingerprint))
    }

    fn path(&self, packet_hash: Hash) -> PathBuf {
        self.dir.join(hex::encode(packet_hash.as_slice()))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CacheWriteOutcome {
    SkippedKnownBytes,
    VerifiedUnchanged,
    Written,
}

pub(super) struct CachedAnnounce {
    pub(super) packet: Packet,
    pub(super) fingerprint: Option<[u8; 32]>,
    pub(super) destination: SingleOutputDestination,
}

pub(super) enum CachedAnnounceRestore {
    Restored(Box<CachedAnnounce>),
    Missing,
    Invalid,
}

enum CachedAnnouncePacketRestore {
    Restored(Packet, Option<[u8; 32]>),
    Missing,
    Invalid,
}

fn encode_cached_announce(iface: AddressHash, packet: &Packet) -> io::Result<Vec<u8>> {
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

fn cached_announce_fingerprint(iface: AddressHash, packet: &Packet) -> io::Result<[u8; 32]> {
    // Packet hashes omit mutable headers, but cached bytes include them.
    let serialized_len = packet
        .serialized_len()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "encode cached announce"))?;
    let mut fingerprint = Sha256::new();
    fingerprint.update((serialized_len as u64).to_be_bytes());
    fingerprint.update([packet.header.to_meta(), packet.header.hops]);
    if packet.header.header_type == HeaderType::Type2 {
        let transport = packet
            .transport
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "encode cached announce"))?;
        fingerprint.update(transport.as_slice());
    }
    fingerprint.update(packet.destination.as_slice());
    fingerprint.update([packet.context as u8]);
    fingerprint.update(packet.data.as_slice());
    fingerprint.update(iface.as_slice());
    Ok(fingerprint.finalize().into())
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
