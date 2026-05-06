use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use crate::{
    error::RnsError,
    hash::{AddressHash, Hash, ADDRESS_HASH_SIZE, HASH_SIZE},
    iface::InterfaceMode,
    packet::Packet,
};
use rmp::encode::write_array_len;
use rmpv::Value as RmpValue;

const DESTINATION_TIMEOUT: Duration = Duration::from_secs(60 * 60 * 24 * 7);
const AP_PATH_TIME: Duration = Duration::from_secs(60 * 60 * 24);
const ROAMING_PATH_TIME: Duration = Duration::from_secs(60 * 60 * 6);

pub struct PathEntry {
    pub timestamp: Instant,
    pub received_from: AddressHash,
    pub hops: u8,
    pub iface: AddressHash,
    pub packet_hash: Hash,
}

pub struct PathTable {
    map: HashMap<AddressHash, PathEntry>,
}

#[derive(Debug, PartialEq)]
pub struct PythonPathEntry {
    pub destination: AddressHash,
    pub timestamp_secs: f64,
    pub received_from: AddressHash,
    pub hops: u8,
    pub expires_secs: f64,
    pub iface: AddressHash,
    pub interface_hash: Hash,
    pub packet_hash: Hash,
}

impl PathTable {
    pub fn new() -> Self {
        Self { map: HashMap::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn to_msgpack(&self) -> Result<Vec<u8>, RnsError> {
        if !self.map.is_empty() {
            return Err(RnsError::InvalidArgument);
        }

        let mut out = Vec::new();
        write_array_len(&mut out, 0).map_err(|_| RnsError::InvalidArgument)?;
        Ok(out)
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

    pub fn next_hop(&self, destination: &AddressHash) -> Option<AddressHash> {
        self.map.get(destination).map(|entry| entry.received_from)
    }

    pub fn handle_announce(
        &mut self,
        announce: &Packet,
        transport_id: Option<AddressHash>,
        iface: AddressHash,
    ) {
        let hops = announce.header.hops;

        if let Some(existing_entry) = self.map.get(&announce.destination) {
            if hops >= existing_entry.hops {
                return;
            }
        }

        let received_from = transport_id.unwrap_or(announce.destination);
        let new_entry = PathEntry {
            timestamp: Instant::now(),
            received_from,
            hops,
            iface,
            packet_hash: announce.hash(),
        };

        self.map.insert(announce.destination, new_entry);

        log::info!(
            "{} is now reachable over {} hops through {} on iface {}",
            announce.destination,
            hops,
            received_from,
            iface,
        );
    }

    pub fn refresh(&mut self, destination: &AddressHash) {
        if let Some(entry) = self.map.get_mut(destination) {
            entry.timestamp = Instant::now();
        }
    }

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
            PathEntry { timestamp: now, received_from, hops, iface, packet_hash },
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
            },
        );
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
                        RmpValue::Array(vec![]),
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
        iface: AddressHash::new_from_hash(&interface_hash),
        interface_hash,
        packet_hash: decode_hash(&fields[7])?,
    })
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

impl Default for PathTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_stale_uses_shorter_access_point_timeout() {
        let now = Instant::now();
        let destination = AddressHash::new_from_hash(&Hash::new_from_slice(b"destination"));
        let iface = AddressHash::new_from_hash(&Hash::new_from_slice(b"iface"));
        let mut table = PathTable::new();
        table.map.insert(
            destination,
            PathEntry {
                timestamp: now - AP_PATH_TIME - Duration::from_secs(1),
                received_from: destination,
                hops: 1,
                iface,
                packet_hash: Hash::new_from_slice(b"packet"),
            },
        );

        assert_eq!(table.remove_stale(now, |_| Some(InterfaceMode::AccessPoint)), 1);
        assert!(table.get(&destination).is_none());
    }

    #[test]
    fn remove_stale_keeps_full_mode_until_destination_timeout() {
        let now = Instant::now();
        let destination = AddressHash::new_from_hash(&Hash::new_from_slice(b"destination"));
        let iface = AddressHash::new_from_hash(&Hash::new_from_slice(b"iface"));
        let mut table = PathTable::new();
        table.map.insert(
            destination,
            PathEntry {
                timestamp: now - AP_PATH_TIME - Duration::from_secs(1),
                received_from: destination,
                hops: 1,
                iface,
                packet_hash: Hash::new_from_slice(b"packet"),
            },
        );

        assert_eq!(table.remove_stale(now, |_| Some(InterfaceMode::Full)), 0);
        assert!(table.get(&destination).is_some());
    }

    #[test]
    fn remove_stale_uses_roaming_timeout() {
        let now = Instant::now();
        let destination = AddressHash::new_from_hash(&Hash::new_from_slice(b"destination"));
        let iface = AddressHash::new_from_hash(&Hash::new_from_slice(b"iface"));
        let mut table = PathTable::new();
        table.map.insert(
            destination,
            PathEntry {
                timestamp: now - ROAMING_PATH_TIME - Duration::from_secs(1),
                received_from: destination,
                hops: 1,
                iface,
                packet_hash: Hash::new_from_slice(b"packet"),
            },
        );

        assert_eq!(table.remove_stale(now, |_| Some(InterfaceMode::Roaming)), 1);
        assert!(table.get(&destination).is_none());
    }

    #[test]
    fn remove_stale_drops_paths_for_missing_iface() {
        let now = Instant::now();
        let destination = AddressHash::new_from_hash(&Hash::new_from_slice(b"destination"));
        let iface = AddressHash::new_from_hash(&Hash::new_from_slice(b"iface"));
        let mut table = PathTable::new();
        table.map.insert(
            destination,
            PathEntry {
                timestamp: now,
                received_from: destination,
                hops: 1,
                iface,
                packet_hash: Hash::new_from_slice(b"packet"),
            },
        );

        assert_eq!(table.remove_stale(now, |_| None), 1);
        assert!(table.get(&destination).is_none());
    }
}
