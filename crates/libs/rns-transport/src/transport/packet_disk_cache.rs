use crate::hash::{AddressHash, Hash, HASH_SIZE};
use crate::packet::Packet;
use rmpv::Value;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedPacket {
    pub packet: Packet,
    pub interface_reference: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ReticulumPacketDiskCache {
    root: PathBuf,
}

impl ReticulumPacketDiskCache {
    pub fn new(reticulum_storage_path: impl AsRef<Path>) -> Self {
        Self { root: reticulum_storage_path.as_ref().join("cache") }
    }

    pub async fn write(
        &self,
        packet: &Packet,
        interface_reference: Option<&str>,
        announce: bool,
    ) -> io::Result<Hash> {
        let hash = packet.hash();
        let path = self.path(hash, announce);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let raw = packet
            .to_bytes()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "encode cached packet"))?;
        let interface =
            interface_reference.map_or(Value::Nil, |value| Value::from(value.to_owned()));
        let value = Value::Array(vec![Value::Binary(raw), interface]);
        let mut payload = Vec::new();
        rmpv::encode::write_value(&mut payload, &value)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "encode cached packet"))?;
        tokio::fs::write(path, payload).await?;
        Ok(hash)
    }

    pub async fn read(&self, hash: Hash, announce: bool) -> io::Result<Option<CachedPacket>> {
        let payload = match tokio::fs::read(self.path(hash, announce)).await {
            Ok(payload) => payload,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let value = rmpv::decode::read_value(&mut io::Cursor::new(payload))
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let fields = value.as_array().filter(|fields| fields.len() >= 2).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid packet cache row")
        })?;
        let raw = match &fields[0] {
            Value::Binary(raw) => raw.as_slice(),
            Value::String(raw) => raw.as_str().map(str::as_bytes).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid packet bytes")
            })?,
            _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid packet bytes")),
        };
        let packet = Packet::from_bytes(raw)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "decode cached packet"))?;
        let interface_reference = if fields[1].is_nil() {
            None
        } else {
            Some(
                fields[1]
                    .as_str()
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "invalid interface reference")
                    })?
                    .to_owned(),
            )
        };
        Ok(Some(CachedPacket { packet, interface_reference }))
    }

    pub async fn clean_announces(
        &self,
        active_path_hashes: &[Hash],
        tunnel_path_hashes: &[Hash],
    ) -> io::Result<usize> {
        let mut scan = None;
        let (_, removed, _, _) = self
            .clean_announces_batch(
                active_path_hashes,
                tunnel_path_hashes,
                &mut scan,
                usize::MAX,
                Duration::ZERO,
            )
            .await?;
        Ok(removed)
    }

    /// Examine at most `max_entries` cache rows, retaining the directory cursor
    /// between calls so maintenance work stays bounded across large caches.
    pub async fn clean_announces_batch(
        &self,
        active_path_hashes: &[Hash],
        tunnel_path_hashes: &[Hash],
        scan: &mut Option<tokio::fs::ReadDir>,
        max_entries: usize,
        grace: Duration,
    ) -> io::Result<(usize, usize, u64, bool)> {
        let keep =
            active_path_hashes.iter().chain(tunnel_path_hashes).copied().collect::<HashSet<_>>();
        self.clean_announces_batch_for_hashes(&keep, scan, max_entries, grace).await
    }

    pub(crate) async fn clean_announces_batch_for_hashes(
        &self,
        keep: &HashSet<Hash>,
        scan: &mut Option<tokio::fs::ReadDir>,
        max_entries: usize,
        grace: Duration,
    ) -> io::Result<(usize, usize, u64, bool)> {
        self.clean_announces_batch_matching(|hash| keep.contains(&hash), scan, max_entries, grace)
            .await
    }

    pub(crate) async fn clean_announces_batch_for_fingerprints(
        &self,
        fingerprints: &HashMap<Hash, [u8; 32]>,
        scan: &mut Option<tokio::fs::ReadDir>,
        max_entries: usize,
        grace: Duration,
    ) -> io::Result<(usize, usize, u64, bool)> {
        self.clean_announces_batch_matching(
            |hash| fingerprints.contains_key(&hash),
            scan,
            max_entries,
            grace,
        )
        .await
    }

    async fn clean_announces_batch_matching(
        &self,
        is_retained: impl Fn(Hash) -> bool,
        scan: &mut Option<tokio::fs::ReadDir>,
        max_entries: usize,
        grace: Duration,
    ) -> io::Result<(usize, usize, u64, bool)> {
        if max_entries == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "announce cache cleanup batch must examine at least one entry",
            ));
        }
        if scan.is_none() {
            *scan = match tokio::fs::read_dir(self.root.join("announces")).await {
                Ok(entries) => Some(entries),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    return Ok((0, 0, 0, true))
                }
                Err(error) => return Err(error),
            };
        }
        let cutoff = match SystemTime::now().checked_sub(grace) {
            Some(cutoff) => cutoff,
            None => SystemTime::UNIX_EPOCH,
        };
        let mut examined = 0;
        let mut removed = 0;
        let mut bytes_removed = 0u64;
        while examined < max_entries {
            let Some(entries) = scan.as_mut() else {
                return Err(io::Error::other("announce cache scan cursor was not initialized"));
            };
            let Some(entry) = entries.next_entry().await? else {
                *scan = None;
                return Ok((examined, removed, bytes_removed, true));
            };
            examined += 1;
            let name = entry.file_name().to_string_lossy().to_string();
            if hex::decode(&name)
                .ok()
                .and_then(|bytes| <[u8; HASH_SIZE]>::try_from(bytes).ok())
                .is_some_and(|bytes| is_retained(Hash::new(bytes)))
            {
                continue;
            }
            let path = entry.path();
            let metadata = match tokio::fs::symlink_metadata(&path).await {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            if !metadata.file_type().is_file() || metadata.modified()? > cutoff {
                continue;
            }
            // Recheck after the first metadata read: a concurrent writer refreshes
            // the mtime before replacing the cache entry.
            let current = match tokio::fs::symlink_metadata(&path).await {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            let current_modified = current.modified()?;
            if current.file_type().is_file()
                && current_modified <= cutoff
                && current_modified == metadata.modified()?
            {
                match tokio::fs::remove_file(path).await {
                    Ok(()) => {
                        removed += 1;
                        bytes_removed = bytes_removed.saturating_add(current.len());
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
        }
        Ok((examined, removed, bytes_removed, false))
    }

    pub async fn save_packet_hashlist(
        &self,
        storage_path: impl AsRef<Path>,
        hashes: &[Hash],
    ) -> io::Result<()> {
        let payload = Self::encode_packet_hashlist(hashes.iter());
        self.save_packet_hashlist_payload(storage_path, payload).await
    }

    pub(super) fn encode_packet_hashlist<'a>(
        hashes: impl ExactSizeIterator<Item = &'a Hash>,
    ) -> Vec<u8> {
        let mut payload = Vec::with_capacity(hashes.len().saturating_mul(HASH_SIZE));
        for hash in hashes {
            payload.extend_from_slice(hash.as_slice());
        }
        payload
    }

    pub(super) async fn save_packet_hashlist_payload(
        &self,
        storage_path: impl AsRef<Path>,
        payload: Vec<u8>,
    ) -> io::Result<()> {
        tokio::fs::write(storage_path.as_ref().join("packet_hashlist.raw"), payload).await
    }

    pub async fn load_packet_hashlist(
        &self,
        storage_path: impl AsRef<Path>,
    ) -> io::Result<Vec<Hash>> {
        let raw_path = storage_path.as_ref().join("packet_hashlist.raw");
        match tokio::fs::read(&raw_path).await {
            Ok(payload) => {
                return Ok(payload
                    .as_chunks::<HASH_SIZE>()
                    .0
                    .iter()
                    .map(|bytes| Hash::new(*bytes))
                    .collect());
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }

        // Read the MessagePack file emitted by earlier Rust builds when no
        // pinned-Python-format packet_hashlist.raw exists yet.
        let legacy_path = storage_path.as_ref().join("packet_hashlist");
        let payload = match tokio::fs::read(legacy_path).await {
            Ok(payload) => payload,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let value = rmpv::decode::read_value(&mut io::Cursor::new(payload))
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let values = value
            .as_array()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid packet hashlist"))?;
        values
            .iter()
            .map(|value| {
                let bytes = match value {
                    Value::Binary(bytes) => bytes.as_slice(),
                    Value::String(bytes) => bytes.as_str().map(str::as_bytes).ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "invalid packet hash")
                    })?,
                    _ => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "invalid packet hash",
                        ))
                    }
                };
                let bytes: [u8; HASH_SIZE] = bytes.try_into().map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "invalid packet hash length")
                })?;
                Ok(Hash::new(bytes))
            })
            .collect()
    }

    pub fn interface_hash(reference: &str) -> Option<AddressHash> {
        AddressHash::new_from_hex_string(reference).ok()
    }

    fn path(&self, hash: Hash, announce: bool) -> PathBuf {
        let directory = if announce { self.root.join("announces") } else { self.root.clone() };
        directory.join(hex::encode(hash.as_slice()))
    }
}

#[cfg(test)]
mod tests;
