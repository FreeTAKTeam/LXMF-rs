use core::fmt;

use alloc::vec::Vec;
use sha2::Digest;

use crate::crypt::fernet::{FERNET_MAX_PADDING_SIZE, FERNET_OVERHEAD_SIZE};
use crate::error::RnsError;
use crate::hash::AddressHash;
use crate::hash::Hash;
use crate::hash::ADDRESS_HASH_SIZE;

// Match Python Reticulum default MTU (500) minus max header and IFAC sizes.
// 500 - (2 + 1 + 16*2) - 1 = 464
pub const PACKET_MDU: usize = 464usize;
#[path = "packet_flags.rs"]
mod packet_flags;
pub use packet_flags::*;

impl Default for Header {
    fn default() -> Self {
        Self {
            ifac_flag: IfacFlag::Open,
            header_type: HeaderType::Type1,
            context_flag: ContextFlag::Unset,
            propagation_type: PropagationType::Broadcast,
            destination_type: DestinationType::Single,
            packet_type: PacketType::Data,
            hops: 0,
        }
    }
}

impl Header {
    pub fn to_meta(&self) -> u8 {
        (self.ifac_flag as u8) << 7
            | (self.header_type as u8) << 6
            | (self.context_flag as u8) << 5
            | (self.propagation_type as u8) << 4
            | (self.destination_type as u8) << 2
            | (self.packet_type as u8)
    }

    pub fn from_meta(meta: u8) -> Self {
        Self {
            ifac_flag: IfacFlag::from(meta >> 7),
            header_type: HeaderType::from(meta >> 6),
            context_flag: ContextFlag::from(meta >> 5),
            propagation_type: PropagationType::from(meta >> 4),
            destination_type: DestinationType::from(meta >> 2),
            packet_type: PacketType::from(meta),
            hops: 0,
        }
    }
}

impl fmt::Display for Header {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:b}{:b}{:b}{:b}{:0>2b}{:0>2b}.{}",
            self.ifac_flag as u8,
            self.header_type as u8,
            self.context_flag as u8,
            self.propagation_type as u8,
            self.destination_type as u8,
            self.packet_type as u8,
            self.hops,
        )
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct PacketDataBuffer {
    buffer: Vec<u8>,
}

impl PacketDataBuffer {
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    pub fn new_from_slice(data: &[u8]) -> Self {
        let mut buffer = Self::new();
        buffer.safe_write(data);
        buffer
    }

    pub fn reset(&mut self) {
        self.buffer.clear();
    }

    pub fn resize(&mut self, len: usize) {
        self.buffer.resize(len.min(PACKET_DATA_MAX), 0);
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    pub fn chain_write(&mut self, data: &[u8]) -> Result<&mut Self, RnsError> {
        self.write(data)?;
        Ok(self)
    }

    pub fn finalize(self) -> Self {
        self
    }

    pub fn safe_write(&mut self, data: &[u8]) -> usize {
        let available = PACKET_DATA_MAX.saturating_sub(self.buffer.len());
        let write_len = data.len().min(available);
        self.buffer.extend_from_slice(&data[..write_len]);
        write_len
    }

    pub fn chain_safe_write(&mut self, data: &[u8]) -> &mut Self {
        self.safe_write(data);
        self
    }

    pub fn write(&mut self, data: &[u8]) -> Result<usize, RnsError> {
        if self.buffer.len().saturating_add(data.len()) > PACKET_DATA_MAX {
            return Err(RnsError::OutOfMemory);
        }
        self.buffer.extend_from_slice(data);
        Ok(data.len())
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.buffer
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.buffer
    }

    pub fn accuire_buf(&mut self, len: usize) -> &mut [u8] {
        self.buffer.resize(len.min(PACKET_DATA_MAX), 0);
        &mut self.buffer
    }

    pub fn accuire_buf_max(&mut self) -> &mut [u8] {
        self.buffer.resize(PACKET_DATA_MAX, 0);
        &mut self.buffer
    }
}

impl Default for PacketDataBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub struct PacketIfac {
    pub access_code: [u8; PACKET_IFAC_MAX_LENGTH],
    pub length: usize,
}

impl PacketIfac {
    pub fn new_from_slice(slice: &[u8]) -> Self {
        let mut access_code = [0u8; PACKET_IFAC_MAX_LENGTH];
        access_code[..slice.len()].copy_from_slice(slice);
        Self { access_code, length: slice.len() }
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.access_code[..self.length]
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Packet {
    pub header: Header,
    pub ifac: Option<PacketIfac>,
    pub destination: AddressHash,
    pub transport: Option<AddressHash>,
    pub context: PacketContext,
    pub data: PacketDataBuffer,
}

impl Packet {
    pub const LXMF_MAX_PAYLOAD: usize = LXMF_MAX_PAYLOAD;

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, RnsError> {
        let min_len = 2 + ADDRESS_HASH_SIZE + 1;
        if bytes.len() < min_len {
            return Err(RnsError::InvalidArgument);
        }

        let flags = bytes[0];
        let hops = bytes[1];

        let mut header = Header::from_meta(flags);
        header.hops = hops;

        let mut idx = 2;

        let transport = if header.header_type == HeaderType::Type2 {
            if bytes.len() < idx + ADDRESS_HASH_SIZE {
                return Err(RnsError::InvalidArgument);
            }
            let mut raw = [0u8; ADDRESS_HASH_SIZE];
            raw.copy_from_slice(&bytes[idx..idx + ADDRESS_HASH_SIZE]);
            idx += ADDRESS_HASH_SIZE;
            Some(AddressHash::new(raw))
        } else {
            None
        };

        if bytes.len() < idx + ADDRESS_HASH_SIZE + 1 {
            return Err(RnsError::InvalidArgument);
        }

        let mut dest_raw = [0u8; ADDRESS_HASH_SIZE];
        dest_raw.copy_from_slice(&bytes[idx..idx + ADDRESS_HASH_SIZE]);
        idx += ADDRESS_HASH_SIZE;
        let destination = AddressHash::new(dest_raw);

        let context = PacketContext::from(bytes[idx]);
        idx += 1;

        if bytes.len() - idx > PACKET_DATA_MAX {
            return Err(RnsError::OutOfMemory);
        }
        let data = PacketDataBuffer::new_from_slice(&bytes[idx..]);

        Ok(Self { header, ifac: None, destination, transport, context, data })
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, RnsError> {
        let mut out = Vec::with_capacity(2 + ADDRESS_HASH_SIZE + 1 + self.data.len());

        out.push(self.header.to_meta());
        out.push(self.header.hops);

        if self.header.header_type == HeaderType::Type2 {
            let transport = self.transport.ok_or(RnsError::InvalidArgument)?;
            out.extend_from_slice(transport.as_slice());
        }

        out.extend_from_slice(self.destination.as_slice());
        out.push(self.context as u8);
        out.extend_from_slice(self.data.as_slice());

        Ok(out)
    }

    /// Packet hash used for deduplication, cache keys, and proofs.
    ///
    /// The `0b00001111` mask on the header byte is **protocol-mandated**
    /// (issue #527): reference Reticulum (`RNS/Packet.py` `get_hash`)
    /// hashes `raw[0] & 0b00001111` plus destination, context, and data,
    /// so hops, propagation type, header type, context flag, and the IFAC
    /// flag are deliberately excluded. A relayed copy of the same logical
    /// packet (hops incremented, header type changed) must keep the same
    /// hash — that is exactly what lets the packet-hash list suppress
    /// rebroadcast loops. `destination/link/id.rs` relies on the same
    /// mask for link-id derivation, so changing it breaks wire
    /// compatibility in both places.
    ///
    /// Cache invariant: two wire-distinct packets that differ only in the
    /// excluded header fields intentionally collide in the hash. The
    /// dedup layer (`TransportHandler::filter_duplicate_packets`) owns
    /// disambiguation using the *unhashed* fields — announces bypass
    /// dedup, keepalive/resource/channel contexts are always allowed, and
    /// link-request proofs are allowed while the link is inactive — so
    /// this function must never be "fixed" to include more header bits.
    pub fn hash(&self) -> Hash {
        Hash::new(
            Hash::generator()
                .chain_update([self.header.to_meta() & 0b00001111])
                .chain_update(self.destination.as_slice())
                .chain_update([self.context as u8])
                .chain_update(self.data.as_slice())
                .finalize()
                .into(),
        )
    }

    pub fn fragment_for_lxmf(data: &[u8]) -> Result<Vec<Packet>, RnsError> {
        let mut out = Vec::new();
        for chunk in data.chunks(Self::LXMF_MAX_PAYLOAD) {
            let packet =
                Packet { data: PacketDataBuffer::new_from_slice(chunk), ..Default::default() };
            out.push(packet);
        }
        Ok(out)
    }
}

impl Default for Packet {
    fn default() -> Self {
        Self {
            header: Default::default(),
            destination: AddressHash::new_empty(),
            data: Default::default(),
            ifac: None,
            transport: None,
            context: crate::packet::PacketContext::None,
        }
    }
}

impl fmt::Display for Packet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}", self.header)?;

        if let Some(transport) = self.transport {
            write!(f, " {}", transport)?;
        }

        write!(f, " {}", self.destination)?;

        write!(f, " 0x[{}]]", self.data.len())?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AddressHash, ContextFlag, DestinationType, Header, HeaderType, IfacFlag, Packet,
        PacketContext, PacketDataBuffer, PacketType, PropagationType,
    };

    #[test]
    fn header_meta_roundtrip_preserves_context_and_transport_bits() {
        let header = Header {
            ifac_flag: IfacFlag::Open,
            header_type: HeaderType::Type1,
            context_flag: ContextFlag::Set,
            propagation_type: PropagationType::Transport,
            destination_type: DestinationType::Single,
            packet_type: PacketType::Announce,
            hops: 0,
        };

        let meta = header.to_meta();
        assert_eq!(meta & 0b0010_0000, 0b0010_0000);
        assert_eq!(meta & 0b0001_0000, 0b0001_0000);

        let decoded = Header::from_meta(meta);
        assert_eq!(decoded.context_flag, ContextFlag::Set);
        assert_eq!(decoded.propagation_type, PropagationType::Transport);
    }

    // Collision tests for issue #527: packets differing only in the
    // header fields excluded by the protocol-mandated 0b00001111 mask
    // MUST hash identically (that is what suppresses rebroadcast loops
    // of a relayed packet), while any change to the hashed fields must
    // change the hash.
    fn base_packet() -> Packet {
        Packet {
            header: Header {
                ifac_flag: IfacFlag::Open,
                header_type: HeaderType::Type1,
                context_flag: ContextFlag::Unset,
                propagation_type: PropagationType::Broadcast,
                destination_type: DestinationType::Single,
                packet_type: PacketType::Data,
                hops: 0,
            },
            destination: AddressHash::new_from_slice(&[0x42; 16]),
            context: PacketContext::None,
            data: PacketDataBuffer::new_from_slice(b"collision-test-payload"),
            ..Default::default()
        }
    }

    #[test]
    fn packet_hash_is_stable_across_excluded_header_fields() {
        let base = base_packet();
        let mut relayed = base_packet();
        // Everything the mask excludes: hops, header type, propagation
        // type, context flag, IFAC flag, and the transport address.
        relayed.header.hops = 7;
        relayed.header.header_type = HeaderType::Type2;
        relayed.header.propagation_type = PropagationType::Transport;
        relayed.header.context_flag = ContextFlag::Set;
        relayed.header.ifac_flag = IfacFlag::Authenticated;
        relayed.transport = Some(AddressHash::new_from_slice(&[0x99; 16]));

        assert_eq!(
            base.hash(),
            relayed.hash(),
            "a relayed copy of the same logical packet must keep the same hash"
        );
    }

    #[test]
    fn packet_hash_changes_with_hashed_fields() {
        let base = base_packet();

        let mut different_destination = base_packet();
        different_destination.destination = AddressHash::new_from_slice(&[0x43; 16]);
        assert_ne!(base.hash(), different_destination.hash());

        let mut different_context = base_packet();
        different_context.context = PacketContext::Resource;
        assert_ne!(base.hash(), different_context.hash());

        let mut different_data = base_packet();
        different_data.data = PacketDataBuffer::new_from_slice(b"collision-test-payload!");
        assert_ne!(base.hash(), different_data.hash());

        let mut different_packet_type = base_packet();
        different_packet_type.header.packet_type = PacketType::Proof;
        assert_ne!(
            base.hash(),
            different_packet_type.hash(),
            "packet_type is inside the mask and must affect the hash"
        );
    }
}
