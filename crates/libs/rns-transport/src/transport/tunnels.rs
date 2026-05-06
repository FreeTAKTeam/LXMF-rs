use super::*;
use crate::destination::PlainInputDestination;
use crate::hash::ADDRESS_HASH_SIZE;
use crate::identity::{EmptyIdentity, PUBLIC_KEY_LENGTH};
use crate::packet::{ContextFlag, Header, HeaderType, IfacFlag, PropagationType};
use ed25519_dalek::{Signature, SIGNATURE_LENGTH};
use rmpv::Value as RmpValue;
use std::collections::HashMap;
use std::time::Instant;

const TUNNEL_TIMEOUT: Duration = Duration::from_secs(60 * 60 * 8);
const TUNNEL_PATH_TIMEOUT: Duration = Duration::from_secs(60 * 60 * 8);

pub fn create_tunnel_synthesize_destination() -> PlainInputDestination {
    PlainInputDestination::new(
        EmptyIdentity {},
        DestinationName::new("rnstransport", "tunnel.synthesize"),
    )
}

pub(super) fn synthesize_tunnel_packet(identity: &PrivateIdentity, interface_hash: Hash) -> Packet {
    let public_identity = identity.as_identity();
    let random_hash = AddressHash::new_from_rand(OsRng);

    let mut signed_data = Vec::with_capacity(PUBLIC_KEY_LENGTH * 2 + HASH_SIZE + ADDRESS_HASH_SIZE);
    signed_data.extend_from_slice(public_identity.public_key_bytes());
    signed_data.extend_from_slice(public_identity.verifying_key_bytes());
    signed_data.extend_from_slice(interface_hash.as_slice());
    signed_data.extend_from_slice(random_hash.as_slice());

    let signature = identity.sign(&signed_data);
    let mut data = PacketDataBuffer::new_from_slice(&signed_data);
    data.safe_write(&signature.to_bytes());

    Packet {
        header: Header {
            ifac_flag: IfacFlag::Open,
            header_type: HeaderType::Type1,
            context_flag: ContextFlag::Unset,
            propagation_type: PropagationType::Broadcast,
            destination_type: DestinationType::Plain,
            packet_type: PacketType::Data,
            hops: 0,
        },
        ifac: None,
        destination: create_tunnel_synthesize_destination().desc.address_hash,
        transport: None,
        context: PacketContext::None,
        data,
    }
}

#[derive(Default)]
pub(super) struct TunnelTable {
    tunnels: HashMap<Hash, TunnelEntry>,
    iface_tunnels: HashMap<AddressHash, Hash>,
}

struct TunnelEntry {
    iface: Option<AddressHash>,
    expires: Instant,
    paths: HashMap<AddressHash, TunnelPathEntry>,
}

#[derive(Clone, Copy)]
struct TunnelPathEntry {
    timestamp: Instant,
    received_from: AddressHash,
    hops: u8,
    packet_hash: Hash,
}

#[derive(Debug, PartialEq)]
pub(super) struct PythonTunnelEntry {
    pub tunnel_id: Hash,
    pub interface_hash: Option<Hash>,
    pub paths: Vec<PythonTunnelPathEntry>,
    pub expires_secs: f64,
}

#[derive(Debug, PartialEq)]
pub(super) struct PythonTunnelPathEntry {
    pub destination: AddressHash,
    pub timestamp_secs: f64,
    pub received_from: AddressHash,
    pub hops: u8,
    pub expires_secs: f64,
    pub interface_hash: Option<Hash>,
    pub packet_hash: Hash,
}

impl TunnelTable {
    pub fn new() -> Self {
        Self::default()
    }

    fn handle_tunnel(
        &mut self,
        tunnel_id: Hash,
        iface: AddressHash,
        now: Instant,
    ) -> Vec<TunnelRestorePath> {
        self.iface_tunnels.insert(iface, tunnel_id);
        let entry = self.tunnels.entry(tunnel_id).or_insert_with(|| TunnelEntry {
            iface: None,
            expires: now + TUNNEL_TIMEOUT,
            paths: HashMap::new(),
        });
        entry.iface = Some(iface);
        entry.expires = now + TUNNEL_TIMEOUT;

        entry
            .paths
            .clone()
            .into_iter()
            .filter_map(|(destination, path)| {
                if now.checked_duration_since(path.timestamp).unwrap_or_default()
                    > TUNNEL_PATH_TIMEOUT
                {
                    return None;
                }
                Some(TunnelRestorePath {
                    destination,
                    received_from: path.received_from,
                    hops: path.hops,
                    iface,
                    packet_hash: path.packet_hash,
                })
            })
            .collect()
    }

    pub fn note_path(
        &mut self,
        iface: AddressHash,
        destination: AddressHash,
        received_from: AddressHash,
        hops: u8,
        packet_hash: Hash,
        now: Instant,
    ) {
        let Some(tunnel_id) = self.iface_tunnels.get(&iface).copied() else {
            return;
        };
        let Some(tunnel) = self.tunnels.get_mut(&tunnel_id) else {
            return;
        };
        tunnel.expires = now + TUNNEL_TIMEOUT;
        tunnel.paths.insert(
            destination,
            TunnelPathEntry { timestamp: now, received_from, hops, packet_hash },
        );
    }

    pub fn remove_stale(&mut self, now: Instant) -> usize {
        let before = self.tunnels.len();
        self.tunnels.retain(|_, tunnel| {
            if now > tunnel.expires {
                return false;
            }
            tunnel.paths.retain(|_, path| {
                now.checked_duration_since(path.timestamp).unwrap_or_default()
                    <= TUNNEL_PATH_TIMEOUT
            });
            true
        });
        self.iface_tunnels.retain(|_, tunnel_id| self.tunnels.contains_key(tunnel_id));
        before - self.tunnels.len()
    }

    pub fn export_python_entries<F>(
        &self,
        now: Instant,
        now_unix_secs: f64,
        mut interface_hash_for_iface: F,
    ) -> Vec<PythonTunnelEntry>
    where
        F: FnMut(&AddressHash) -> Option<Hash>,
    {
        self.tunnels
            .iter()
            .filter_map(|(tunnel_id, tunnel)| {
                let interface_hash =
                    tunnel.iface.and_then(|iface| interface_hash_for_iface(&iface));
                let paths = tunnel
                    .paths
                    .iter()
                    .map(|(destination, path)| {
                        let age = now.checked_duration_since(path.timestamp).unwrap_or_default();
                        let timestamp_secs = now_unix_secs - age.as_secs_f64();
                        PythonTunnelPathEntry {
                            destination: *destination,
                            timestamp_secs,
                            received_from: path.received_from,
                            hops: path.hops,
                            expires_secs: timestamp_secs + TUNNEL_PATH_TIMEOUT.as_secs_f64(),
                            interface_hash,
                            packet_hash: path.packet_hash,
                        }
                    })
                    .collect::<Vec<_>>();
                if paths.is_empty() {
                    return None;
                }
                let expires_secs = now_unix_secs
                    + tunnel.expires.checked_duration_since(now).unwrap_or_default().as_secs_f64();
                Some(PythonTunnelEntry {
                    tunnel_id: *tunnel_id,
                    interface_hash,
                    paths,
                    expires_secs,
                })
            })
            .collect()
    }

    pub fn restore_python_entries(
        &mut self,
        entries: Vec<PythonTunnelEntry>,
        now: Instant,
        now_unix_secs: f64,
    ) -> usize {
        let mut restored = 0usize;
        for entry in entries {
            let expires_in = Duration::from_secs_f64((entry.expires_secs - now_unix_secs).max(0.0));
            let mut paths = HashMap::new();
            for path in entry.paths {
                let age = Duration::from_secs_f64((now_unix_secs - path.timestamp_secs).max(0.0));
                paths.insert(
                    path.destination,
                    TunnelPathEntry {
                        timestamp: now.checked_sub(age).unwrap_or(now),
                        received_from: path.received_from,
                        hops: path.hops,
                        packet_hash: path.packet_hash,
                    },
                );
            }
            if !paths.is_empty() {
                self.tunnels.insert(
                    entry.tunnel_id,
                    TunnelEntry { iface: None, expires: now + expires_in, paths },
                );
                restored += 1;
            }
        }
        restored
    }

    pub fn encode_python_entries(entries: &[PythonTunnelEntry]) -> Result<Vec<u8>, RnsError> {
        let value = RmpValue::Array(
            entries
                .iter()
                .map(|entry| {
                    RmpValue::Array(vec![
                        RmpValue::Binary(entry.tunnel_id.as_slice().to_vec()),
                        optional_hash_value(entry.interface_hash),
                        RmpValue::Array(
                            entry
                                .paths
                                .iter()
                                .map(|path| {
                                    RmpValue::Array(vec![
                                        RmpValue::Binary(path.destination.as_slice().to_vec()),
                                        RmpValue::F64(path.timestamp_secs),
                                        RmpValue::Binary(path.received_from.as_slice().to_vec()),
                                        RmpValue::from(u64::from(path.hops)),
                                        RmpValue::F64(path.expires_secs),
                                        RmpValue::Array(vec![]),
                                        optional_hash_value(path.interface_hash),
                                        RmpValue::Binary(path.packet_hash.as_slice().to_vec()),
                                    ])
                                })
                                .collect(),
                        ),
                        RmpValue::F64(entry.expires_secs),
                    ])
                })
                .collect(),
        );
        let mut out = Vec::new();
        rmpv::encode::write_value(&mut out, &value).map_err(|_| RnsError::InvalidArgument)?;
        Ok(out)
    }

    pub fn decode_python_entries(bytes: &[u8]) -> Result<Vec<PythonTunnelEntry>, RnsError> {
        let value: RmpValue = rmpv::decode::read_value(&mut std::io::Cursor::new(bytes))
            .map_err(|_| RnsError::InvalidArgument)?;
        let RmpValue::Array(entries) = value else {
            return Err(RnsError::InvalidArgument);
        };
        entries.iter().map(decode_python_tunnel_entry).collect::<Result<Vec<_>, _>>()
    }
}

struct TunnelRestorePath {
    destination: AddressHash,
    received_from: AddressHash,
    hops: u8,
    iface: AddressHash,
    packet_hash: Hash,
}

pub(super) async fn handle_tunnel_synthesize_packet<'a>(
    packet: &Packet,
    handler: &mut MutexGuard<'a, TransportHandler>,
    iface: AddressHash,
) {
    let Some(tunnel_id) = validate_tunnel_synthesize(packet.data.as_slice()) else {
        log::debug!("tp({}): ignoring invalid tunnel synth packet", handler.config.name);
        return;
    };

    let restore_paths = handler.tunnel_table.handle_tunnel(tunnel_id, iface, Instant::now());
    let mut restored = 0usize;
    for path in restore_paths {
        if handler.path_table.restore_tunnel_path(
            path.destination,
            path.received_from,
            path.hops,
            path.iface,
            path.packet_hash,
            Instant::now(),
        ) {
            restored += 1;
        }
    }
    log::debug!(
        "tp({}): tunnel {} established on iface {}, restored {} paths",
        handler.config.name,
        tunnel_id,
        iface,
        restored
    );
}

fn validate_tunnel_synthesize(data: &[u8]) -> Option<Hash> {
    let expected_len = PUBLIC_KEY_LENGTH * 2 + HASH_SIZE + ADDRESS_HASH_SIZE + SIGNATURE_LENGTH;
    if data.len() != expected_len {
        return None;
    }

    let public_identity = &data[..PUBLIC_KEY_LENGTH * 2];
    let interface_hash_start = PUBLIC_KEY_LENGTH * 2;
    let random_hash_start = interface_hash_start + HASH_SIZE;
    let signature_start = random_hash_start + ADDRESS_HASH_SIZE;

    let identity = Identity::new_from_slices(
        &public_identity[..PUBLIC_KEY_LENGTH],
        &public_identity[PUBLIC_KEY_LENGTH..PUBLIC_KEY_LENGTH * 2],
    );
    let signed_data = &data[..signature_start];
    let signature = Signature::from_slice(&data[signature_start..]).ok()?;
    identity.verify(signed_data, &signature).ok()?;

    Some(Hash::new_from_slice(&data[..random_hash_start]))
}

fn optional_hash_value(hash: Option<Hash>) -> RmpValue {
    hash.map(|hash| RmpValue::Binary(hash.as_slice().to_vec())).unwrap_or(RmpValue::Nil)
}

fn decode_python_tunnel_entry(value: &RmpValue) -> Result<PythonTunnelEntry, RnsError> {
    let RmpValue::Array(fields) = value else {
        return Err(RnsError::InvalidArgument);
    };
    if fields.len() < 4 {
        return Err(RnsError::InvalidArgument);
    }
    let RmpValue::Array(path_values) = &fields[2] else {
        return Err(RnsError::InvalidArgument);
    };
    Ok(PythonTunnelEntry {
        tunnel_id: decode_hash(&fields[0])?,
        interface_hash: decode_optional_hash(&fields[1])?,
        paths: path_values
            .iter()
            .map(decode_python_tunnel_path_entry)
            .collect::<Result<Vec<_>, _>>()?,
        expires_secs: decode_f64(&fields[3])?,
    })
}

fn decode_python_tunnel_path_entry(value: &RmpValue) -> Result<PythonTunnelPathEntry, RnsError> {
    let RmpValue::Array(fields) = value else {
        return Err(RnsError::InvalidArgument);
    };
    if fields.len() < 8 {
        return Err(RnsError::InvalidArgument);
    }
    Ok(PythonTunnelPathEntry {
        destination: decode_address_hash(&fields[0])?,
        timestamp_secs: decode_f64(&fields[1])?,
        received_from: decode_address_hash(&fields[2])?,
        hops: decode_u8(&fields[3])?,
        expires_secs: decode_f64(&fields[4])?,
        interface_hash: decode_optional_hash(&fields[6])?,
        packet_hash: decode_hash(&fields[7])?,
    })
}

fn decode_optional_hash(value: &RmpValue) -> Result<Option<Hash>, RnsError> {
    match value {
        RmpValue::Nil => Ok(None),
        _ => decode_hash(value).map(Some),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tunnel_synthesize_packet_validates_to_python_tunnel_id() {
        let identity = PrivateIdentity::new_from_name("tunnel-test");
        let interface_hash = Hash::new_from_slice(b"iface");

        let packet = synthesize_tunnel_packet(&identity, interface_hash);
        let tunnel_id = validate_tunnel_synthesize(packet.data.as_slice()).expect("valid tunnel");

        let mut expected = Vec::new();
        expected.extend_from_slice(identity.as_identity().public_key_bytes());
        expected.extend_from_slice(identity.as_identity().verifying_key_bytes());
        expected.extend_from_slice(interface_hash.as_slice());
        assert_eq!(tunnel_id, Hash::new_from_slice(&expected));
    }

    #[test]
    fn tunnel_synthesize_rejects_bad_signature() {
        let identity = PrivateIdentity::new_from_name("tunnel-test");
        let interface_hash = Hash::new_from_slice(b"iface");
        let mut packet = synthesize_tunnel_packet(&identity, interface_hash);
        let last = packet.data.len() - 1;
        packet.data.as_mut_slice()[last] ^= 0x01;

        assert!(validate_tunnel_synthesize(packet.data.as_slice()).is_none());
    }

    #[test]
    fn python_tunnel_entries_roundtrip_msgpack_shape() {
        let tunnel_id = Hash::new_from_slice(b"tunnel-id");
        let interface_hash = Hash::new_from_slice(b"iface");
        let destination = AddressHash::new_from_slice(&[1u8; 16]);
        let received_from = AddressHash::new_from_slice(&[2u8; 16]);
        let packet_hash = Hash::new_from_slice(b"packet");
        let entries = vec![PythonTunnelEntry {
            tunnel_id,
            interface_hash: Some(interface_hash),
            paths: vec![PythonTunnelPathEntry {
                destination,
                timestamp_secs: 10.0,
                received_from,
                hops: 2,
                expires_secs: 20.0,
                interface_hash: Some(interface_hash),
                packet_hash,
            }],
            expires_secs: 30.0,
        }];

        let encoded = TunnelTable::encode_python_entries(&entries).expect("encode");
        let decoded = TunnelTable::decode_python_entries(&encoded).expect("decode");

        assert_eq!(decoded, entries);
    }

    #[test]
    fn restored_python_tunnel_paths_are_returned_when_tunnel_reappears() {
        let now = Instant::now();
        let tunnel_id = Hash::new_from_slice(b"tunnel-id");
        let destination = AddressHash::new_from_slice(&[1u8; 16]);
        let received_from = AddressHash::new_from_slice(&[2u8; 16]);
        let iface = AddressHash::new_from_slice(&[3u8; 16]);
        let packet_hash = Hash::new_from_slice(b"packet");
        let mut table = TunnelTable::new();

        let restored = table.restore_python_entries(
            vec![PythonTunnelEntry {
                tunnel_id,
                interface_hash: None,
                paths: vec![PythonTunnelPathEntry {
                    destination,
                    timestamp_secs: 99.0,
                    received_from,
                    hops: 3,
                    expires_secs: 199.0,
                    interface_hash: None,
                    packet_hash,
                }],
                expires_secs: 199.0,
            }],
            now,
            100.0,
        );
        assert_eq!(restored, 1);

        let paths = table.handle_tunnel(tunnel_id, iface, now);

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].destination, destination);
        assert_eq!(paths[0].received_from, received_from);
        assert_eq!(paths[0].hops, 3);
        assert_eq!(paths[0].iface, iface);
        assert_eq!(paths[0].packet_hash, packet_hash);
    }
}
