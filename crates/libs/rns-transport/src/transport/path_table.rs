use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use crate::{
    destination::RAND_HASH_LENGTH,
    error::RnsError,
    hash::{AddressHash, Hash, ADDRESS_HASH_SIZE, HASH_SIZE},
    iface::InterfaceMode,
    packet::Packet,
};
use rmpv::Value as RmpValue;

const DESTINATION_TIMEOUT: Duration = Duration::from_secs(60 * 60 * 24 * 7);
const AP_PATH_TIME: Duration = Duration::from_secs(60 * 60 * 24);
const ROAMING_PATH_TIME: Duration = Duration::from_secs(60 * 60 * 6);
const MAX_RANDOM_BLOBS: usize = 64;

pub(super) type RandomBlob = [u8; RAND_HASH_LENGTH];

pub struct PathEntry {
    pub timestamp: Instant,
    pub received_from: AddressHash,
    /// Real network distance to the destination. This value is only
    /// meaningful because `apply_receive_hop_increment` (transport/jobs.rs)
    /// runs on every inbound packet before any path-table bookkeeping —
    /// see [`PathEntry::is_direct`] (issue #515).
    pub hops: u8,
    pub iface: AddressHash,
    pub packet_hash: Hash,
    random_blobs: Vec<RandomBlob>,
    state: PathState,
}

impl PathEntry {
    /// Direct-hop invariant (issue #515): a genuinely direct destination
    /// is `hops == 0`, matching reference Reticulum's `for_local_client`
    /// criterion (`Transport.path_table[dest][IDX_PT_HOPS] == 0`). The
    /// invariant holds only because inbound packets get their hop count
    /// incremented on receipt (`apply_receive_hop_increment` in
    /// transport/jobs.rs) before any routing/path decision is made —
    /// centralizing the checks here means a future refactor of that
    /// ordering has exactly one place to break, loudly, in tests.
    pub fn is_direct(&self) -> bool {
        self.hops == 0
    }

    /// Reference Reticulum `Transport.outbound()` header rule (confirmed
    /// by direct reading):
    ///
    ///   hops > 1 → Type2
    ///   hops == 1 and connected_to_shared_instance → Type2
    ///   else → Type1
    ///
    /// Like [`PathEntry::is_direct`], this assumes `hops` reflects real
    /// distance (see its doc comment).
    pub fn type1_eligible(&self, connected_to_shared_instance: bool) -> bool {
        match self.hops {
            0 => true,
            1 => !connected_to_shared_instance,
            _ => false,
        }
    }
}

pub struct PathTable {
    map: HashMap<AddressHash, PathEntry>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum PathState {
    Unknown,
    Responsive,
    Unresponsive,
}

#[derive(Debug, PartialEq)]
pub struct PythonPathEntry {
    pub destination: AddressHash,
    pub timestamp_secs: f64,
    pub received_from: AddressHash,
    pub hops: u8,
    pub expires_secs: f64,
    pub random_blobs: Vec<RandomBlob>,
    pub iface: AddressHash,
    pub interface_hash: Hash,
    pub packet_hash: Hash,
}

impl PathTable {
    pub fn new() -> Self {
        Self { map: HashMap::new() }
    }

    pub fn get(&self, destination: &AddressHash) -> Option<&PathEntry> {
        self.map.get(destination)
    }

    pub fn next_hop_full(&self, destination: &AddressHash) -> Option<(AddressHash, AddressHash)> {
        self.map.get(destination).map(|entry| (entry.received_from, entry.iface))
    }

    pub fn next_hop_iface(&self, destination: &AddressHash) -> Option<AddressHash> {
        self.map.get(destination).map(|entry| entry.iface)
    }

    pub fn expire_path(&mut self, destination: &AddressHash) -> bool {
        self.map.remove(destination).is_some()
    }

    pub fn expire_paths_via(&mut self, transport: &AddressHash) -> usize {
        let before = self.map.len();
        self.map.retain(|_, entry| entry.received_from != *transport);
        before.saturating_sub(self.map.len())
    }

    pub fn mark_path_unresponsive(&mut self, destination: &AddressHash) -> bool {
        let Some(entry) = self.map.get_mut(destination) else {
            return false;
        };
        entry.state = PathState::Unresponsive;
        true
    }

    pub fn mark_path_responsive(&mut self, destination: &AddressHash) -> bool {
        self.set_path_state(destination, PathState::Responsive)
    }

    pub fn mark_path_unknown(&mut self, destination: &AddressHash) -> bool {
        self.set_path_state(destination, PathState::Unknown)
    }

    fn set_path_state(&mut self, destination: &AddressHash, state: PathState) -> bool {
        let Some(entry) = self.map.get_mut(destination) else {
            return false;
        };
        entry.state = state;
        true
    }

    pub fn path_is_unresponsive(&self, destination: &AddressHash) -> bool {
        self.map.get(destination).is_some_and(|entry| entry.state == PathState::Unresponsive)
    }

    pub fn hops_to(&self, destination: &AddressHash) -> u8 {
        self.map.get(destination).map_or(super::PATHFINDER_M as u8, |entry| entry.hops)
    }

    pub fn handle_announce(
        &mut self,
        announce: &Packet,
        transport_id: Option<AddressHash>,
        iface: AddressHash,
        random_blob: RandomBlob,
        mode_for_iface: impl FnMut(&AddressHash) -> Option<InterfaceMode>,
    ) -> bool {
        self.handle_announce_at(
            announce,
            transport_id,
            iface,
            random_blob,
            Instant::now(),
            mode_for_iface,
        )
    }

    fn handle_announce_at(
        &mut self,
        announce: &Packet,
        transport_id: Option<AddressHash>,
        iface: AddressHash,
        random_blob: RandomBlob,
        now: Instant,
        mode_for_iface: impl FnMut(&AddressHash) -> Option<InterfaceMode>,
    ) -> bool {
        let hops = announce.header.hops;
        let announce_emitted = random_blob_timebase(&random_blob);
        let mut random_blobs = Vec::new();

        if let Some(existing_entry) = self.map.get(&announce.destination) {
            random_blobs.clone_from(&existing_entry.random_blobs);
            if !should_replace_path(
                existing_entry,
                hops,
                &random_blob,
                announce_emitted,
                now,
                mode_for_iface,
            ) {
                return false;
            }
        }

        if !random_blobs.contains(&random_blob) {
            random_blobs.push(random_blob);
            random_blobs = bounded_random_blobs(random_blobs);
        }

        let received_from = transport_id.unwrap_or(announce.destination);
        let new_entry = PathEntry {
            timestamp: now,
            received_from,
            hops,
            iface,
            packet_hash: announce.hash(),
            random_blobs,
            state: PathState::Unknown,
        };

        self.map.insert(announce.destination, new_entry);

        log::info!(
            "{} is now reachable over {} hops through {} on iface {}",
            announce.destination,
            hops,
            received_from,
            iface,
        );
        true
    }

    #[cfg(test)]
    pub fn restore_tunnel_path(
        &mut self,
        destination: AddressHash,
        received_from: AddressHash,
        hops: u8,
        iface: AddressHash,
        packet_hash: Hash,
        now: Instant,
    ) -> bool {
        if let Some(existing) = self.map.get(&destination) {
            if existing.hops < hops {
                return false;
            }
        }

        self.map.insert(
            destination,
            PathEntry {
                timestamp: now,
                received_from,
                hops,
                iface,
                packet_hash,
                random_blobs: Vec::new(),
                state: PathState::Unknown,
            },
        );
        true
    }

    pub fn remove_stale<F>(&mut self, now: Instant, mut mode_for_iface: F) -> usize
    where
        F: FnMut(&AddressHash) -> Option<InterfaceMode>,
    {
        let before = self.map.len();
        self.map.retain(|destination, entry| {
            let Some(mode) = mode_for_iface(&entry.iface) else {
                log::debug!(
                    "Path to {} timed out and was removed because iface {} is no longer active",
                    destination,
                    entry.iface,
                );
                return false;
            };
            let elapsed = now.checked_duration_since(entry.timestamp).unwrap_or_default();
            let timeout = path_timeout_for_mode(mode);
            let keep = elapsed <= timeout;
            if !keep {
                log::debug!("Path to {} timed out and was removed", destination);
            }
            keep
        });
        before - self.map.len()
    }

    pub fn restore_python_entry(
        &mut self,
        entry: PythonPathEntry,
        now: Instant,
        now_unix_secs: f64,
    ) {
        let age = Duration::from_secs_f64((now_unix_secs - entry.timestamp_secs).max(0.0));
        let timestamp = now.checked_sub(age).unwrap_or(now);
        self.map.insert(
            entry.destination,
            PathEntry {
                timestamp,
                received_from: entry.received_from,
                hops: entry.hops,
                iface: entry.iface,
                packet_hash: entry.packet_hash,
                random_blobs: bounded_random_blobs(entry.random_blobs),
                state: PathState::Unknown,
            },
        );
    }

    pub(super) fn random_blobs_for(&self, destination: &AddressHash) -> Vec<RandomBlob> {
        self.map.get(destination).map(|entry| entry.random_blobs.clone()).unwrap_or_default()
    }

    pub fn export_python_entries<F>(
        &self,
        now: Instant,
        now_unix_secs: f64,
        mut iface_info: F,
    ) -> Vec<PythonPathEntry>
    where
        F: FnMut(&AddressHash) -> Option<(InterfaceMode, Hash)>,
    {
        self.map
            .iter()
            .filter_map(|(destination, entry)| {
                let (mode, interface_hash) = iface_info(&entry.iface)?;
                let age = now.checked_duration_since(entry.timestamp).unwrap_or_default();
                let timestamp_secs = now_unix_secs - age.as_secs_f64();
                Some(PythonPathEntry {
                    destination: *destination,
                    timestamp_secs,
                    received_from: entry.received_from,
                    hops: entry.hops,
                    expires_secs: timestamp_secs + path_timeout_for_mode(mode).as_secs_f64(),
                    random_blobs: entry.random_blobs.clone(),
                    iface: entry.iface,
                    interface_hash,
                    packet_hash: entry.packet_hash,
                })
            })
            .collect()
    }

    pub fn encode_python_entries(entries: &[PythonPathEntry]) -> Result<Vec<u8>, RnsError> {
        let value = RmpValue::Array(
            entries
                .iter()
                .map(|entry| {
                    RmpValue::Array(vec![
                        RmpValue::Binary(entry.destination.as_slice().to_vec()),
                        RmpValue::F64(entry.timestamp_secs),
                        RmpValue::Binary(entry.received_from.as_slice().to_vec()),
                        RmpValue::from(u64::from(entry.hops)),
                        RmpValue::F64(entry.expires_secs),
                        RmpValue::Array(
                            entry
                                .random_blobs
                                .iter()
                                .skip(entry.random_blobs.len().saturating_sub(MAX_RANDOM_BLOBS))
                                .map(|blob| RmpValue::Binary(blob.to_vec()))
                                .collect(),
                        ),
                        RmpValue::Binary(entry.interface_hash.as_slice().to_vec()),
                        RmpValue::Binary(entry.packet_hash.as_slice().to_vec()),
                    ])
                })
                .collect(),
        );
        let mut out = Vec::new();
        rmpv::encode::write_value(&mut out, &value).map_err(|_| RnsError::InvalidArgument)?;
        Ok(out)
    }

    pub fn decode_python_entries(bytes: &[u8]) -> Result<Vec<PythonPathEntry>, RnsError> {
        let value: RmpValue = rmpv::decode::read_value(&mut std::io::Cursor::new(bytes))
            .map_err(|_| RnsError::InvalidArgument)?;
        let RmpValue::Array(entries) = value else {
            return Err(RnsError::InvalidArgument);
        };
        entries.iter().map(decode_python_entry).collect::<Result<Vec<_>, _>>()
    }
}

fn decode_python_entry(value: &RmpValue) -> Result<PythonPathEntry, RnsError> {
    let RmpValue::Array(fields) = value else {
        return Err(RnsError::InvalidArgument);
    };
    if fields.len() < 8 {
        return Err(RnsError::InvalidArgument);
    }
    let interface_hash = decode_hash(&fields[6])?;
    Ok(PythonPathEntry {
        destination: decode_address_hash(&fields[0])?,
        timestamp_secs: decode_f64(&fields[1])?,
        received_from: decode_address_hash(&fields[2])?,
        hops: decode_u8(&fields[3])?,
        expires_secs: decode_f64(&fields[4])?,
        random_blobs: decode_random_blobs(&fields[5])?,
        iface: AddressHash::new_from_hash(&interface_hash),
        interface_hash,
        packet_hash: decode_hash(&fields[7])?,
    })
}

fn should_replace_path(
    existing: &PathEntry,
    hops: u8,
    random_blob: &RandomBlob,
    announce_emitted: u64,
    now: Instant,
    mut mode_for_iface: impl FnMut(&AddressHash) -> Option<InterfaceMode>,
) -> bool {
    if existing.random_blobs.contains(random_blob) {
        let path_emitted = newest_random_blob_timebase(&existing.random_blobs);
        return existing.state == PathState::Unresponsive
            && hops > existing.hops
            && announce_emitted == path_emitted;
    }

    let path_emitted = newest_random_blob_timebase(&existing.random_blobs);
    if hops <= existing.hops {
        return announce_emitted > path_emitted;
    }

    path_expired(existing, now, &mut mode_for_iface) || announce_emitted > path_emitted
}

fn path_expired(
    entry: &PathEntry,
    now: Instant,
    mut mode_for_iface: impl FnMut(&AddressHash) -> Option<InterfaceMode>,
) -> bool {
    let mode = mode_for_iface(&entry.iface).unwrap_or(InterfaceMode::Full);
    path_expired_for_mode(entry, now, mode)
}

fn path_expired_for_mode(entry: &PathEntry, now: Instant, mode: InterfaceMode) -> bool {
    now.checked_duration_since(entry.timestamp).unwrap_or_default() >= path_timeout_for_mode(mode)
}

pub(super) fn random_blob_timebase(random_blob: &RandomBlob) -> u64 {
    let mut emitted = [0u8; 8];
    emitted[3..].copy_from_slice(&random_blob[5..]);
    u64::from_be_bytes(emitted)
}

pub(super) fn newest_random_blob_timebase(random_blobs: &[RandomBlob]) -> u64 {
    random_blobs.iter().map(random_blob_timebase).max().unwrap_or(0)
}

pub(super) fn bounded_random_blobs(mut random_blobs: Vec<RandomBlob>) -> Vec<RandomBlob> {
    let remove = random_blobs.len().saturating_sub(MAX_RANDOM_BLOBS);
    if remove > 0 {
        random_blobs.drain(..remove);
    }
    random_blobs
}

fn decode_address_hash(value: &RmpValue) -> Result<AddressHash, RnsError> {
    let bytes = decode_bytes(value)?;
    if bytes.len() != ADDRESS_HASH_SIZE {
        return Err(RnsError::IncorrectHash);
    }
    let mut out = [0u8; ADDRESS_HASH_SIZE];
    out.copy_from_slice(bytes);
    Ok(AddressHash::new(out))
}

fn decode_hash(value: &RmpValue) -> Result<Hash, RnsError> {
    let bytes = decode_bytes(value)?;
    if bytes.len() != HASH_SIZE {
        return Err(RnsError::IncorrectHash);
    }
    let mut out = [0u8; HASH_SIZE];
    out.copy_from_slice(bytes);
    Ok(Hash::new(out))
}

pub(super) fn decode_random_blobs(value: &RmpValue) -> Result<Vec<RandomBlob>, RnsError> {
    let RmpValue::Array(blobs) = value else {
        return Err(RnsError::InvalidArgument);
    };

    blobs
        .iter()
        .map(|value| {
            let bytes = decode_bytes(value)?;
            if bytes.len() != RAND_HASH_LENGTH {
                return Err(RnsError::IncorrectHash);
            }
            let mut blob = RandomBlob::default();
            blob.copy_from_slice(bytes);
            Ok(blob)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(bounded_random_blobs)
}

fn decode_bytes(value: &RmpValue) -> Result<&[u8], RnsError> {
    match value {
        RmpValue::Binary(bytes) => Ok(bytes),
        RmpValue::String(text) => text.as_str().map(str::as_bytes).ok_or(RnsError::InvalidArgument),
        _ => Err(RnsError::InvalidArgument),
    }
}

fn decode_u8(value: &RmpValue) -> Result<u8, RnsError> {
    match value {
        RmpValue::Integer(value) => value.as_u64().and_then(|value| u8::try_from(value).ok()),
        _ => None,
    }
    .ok_or(RnsError::InvalidArgument)
}

fn decode_f64(value: &RmpValue) -> Result<f64, RnsError> {
    match value {
        RmpValue::F64(value) => Some(*value),
        RmpValue::F32(value) => Some(f64::from(*value)),
        RmpValue::Integer(value) => value.as_i64().map(|value| value as f64),
        _ => None,
    }
    .ok_or(RnsError::InvalidArgument)
}

fn path_timeout_for_mode(mode: InterfaceMode) -> Duration {
    match mode {
        InterfaceMode::AccessPoint => AP_PATH_TIME,
        InterfaceMode::Roaming => ROAMING_PATH_TIME,
        InterfaceMode::Full
        | InterfaceMode::PointToPoint
        | InterfaceMode::Boundary
        | InterfaceMode::Gateway => DESTINATION_TIMEOUT,
    }
}

include!("path_table_default.rs");

include!("path_table_tunnel_restore.rs");

#[cfg(test)]
mod tests {
    include!("path_table_tests.rs");
}
