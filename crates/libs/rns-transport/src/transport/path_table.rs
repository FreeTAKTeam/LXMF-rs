use std::{collections::HashMap, time::Instant};

use crate::{
    error::RnsError,
    hash::{AddressHash, Hash},
    packet::{DestinationType, Header, HeaderType, Packet, PacketType, PropagationType},
};
use rmp::encode::write_array_len;

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
        let hops = announce.header.hops + 1;

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

    pub fn handle_inbound_packet(
        &self,
        original_packet: &Packet,
        lookup: Option<AddressHash>,
    ) -> (Packet, Option<AddressHash>) {
        let lookup = lookup.unwrap_or(original_packet.destination);

        let entry = match self.map.get(&lookup) {
            Some(entry) => entry,
            None => return (*original_packet, None),
        };

        (
            Packet {
                header: Header {
                    ifac_flag: original_packet.header.ifac_flag,
                    header_type: HeaderType::Type2,
                    context_flag: original_packet.header.context_flag,
                    propagation_type: PropagationType::Transport,
                    destination_type: original_packet.header.destination_type,
                    packet_type: original_packet.header.packet_type,
                    hops: original_packet.header.hops + 1,
                },
                ifac: None,
                destination: original_packet.destination,
                transport: Some(entry.received_from),
                context: original_packet.context,
                data: original_packet.data,
            },
            Some(entry.iface),
        )
    }

    pub fn refresh(&mut self, destination: &AddressHash) {
        if let Some(entry) = self.map.get_mut(destination) {
            entry.timestamp = Instant::now();
        }
    }

    pub fn handle_packet(&mut self, original_packet: &Packet) -> (Packet, Option<AddressHash>) {
        if original_packet.header.header_type == HeaderType::Type2 {
            return (*original_packet, None);
        }

        if original_packet.header.packet_type == PacketType::Announce {
            return (*original_packet, None);
        }

        if original_packet.header.destination_type == DestinationType::Plain
            || original_packet.header.destination_type == DestinationType::Group
        {
            return (*original_packet, None);
        }

        let entry = match self.map.get(&original_packet.destination) {
            Some(entry) => entry,
            None => return (*original_packet, None),
        };

        if entry.hops <= 1 {
            return (*original_packet, Some(entry.iface));
        }

        (
            Packet {
                header: Header {
                    ifac_flag: original_packet.header.ifac_flag,
                    header_type: HeaderType::Type2,
                    context_flag: original_packet.header.context_flag,
                    propagation_type: PropagationType::Transport,
                    destination_type: original_packet.header.destination_type,
                    packet_type: original_packet.header.packet_type,
                    hops: original_packet.header.hops,
                },
                ifac: original_packet.ifac,
                destination: original_packet.destination,
                transport: Some(entry.received_from),
                context: original_packet.context,
                data: original_packet.data,
            },
            Some(entry.iface),
        )
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
    use crate::buffer::StaticBuffer;
    use crate::packet::{ContextFlag, DestinationType, IfacFlag, PacketType, PropagationType};

    #[test]
    fn handle_packet_direct_hop_preserves_type1_and_ifac_flag() {
        let destination = AddressHash::new_from_hash(&Hash::new_from_slice(b"destination"));
        let iface = AddressHash::new_from_hash(&Hash::new_from_slice(b"iface"));
        let mut table = PathTable::new();
        table.map.insert(
            destination,
            PathEntry {
                timestamp: Instant::now(),
                received_from: destination,
                hops: 1,
                iface,
                packet_hash: Hash::new_from_slice(b"packet"),
            },
        );

        let packet = Packet {
            header: Header {
                ifac_flag: IfacFlag::Open,
                header_type: HeaderType::Type1,
                context_flag: ContextFlag::Unset,
                propagation_type: PropagationType::Broadcast,
                destination_type: DestinationType::Single,
                packet_type: PacketType::Data,
                hops: 0,
            },
            ifac: None,
            destination,
            transport: None,
            context: crate::packet::PacketContext::None,
            data: StaticBuffer::new(),
        };

        let (forwarded, next_iface) = table.handle_packet(&packet);
        assert_eq!(next_iface, Some(iface));
        assert_eq!(forwarded.header.ifac_flag, IfacFlag::Open);
        assert_eq!(forwarded.header.header_type, HeaderType::Type1);
        assert_eq!(forwarded.transport, None);
    }

    #[test]
    fn handle_packet_multihop_promotes_to_type2_transport() {
        let destination = AddressHash::new_from_hash(&Hash::new_from_slice(b"destination"));
        let iface = AddressHash::new_from_hash(&Hash::new_from_slice(b"iface"));
        let next_hop = AddressHash::new_from_hash(&Hash::new_from_slice(b"next_hop"));
        let mut table = PathTable::new();
        table.map.insert(
            destination,
            PathEntry {
                timestamp: Instant::now(),
                received_from: next_hop,
                hops: 2,
                iface,
                packet_hash: Hash::new_from_slice(b"packet"),
            },
        );

        let packet = Packet {
            header: Header {
                ifac_flag: IfacFlag::Open,
                header_type: HeaderType::Type1,
                context_flag: ContextFlag::Unset,
                propagation_type: PropagationType::Broadcast,
                destination_type: DestinationType::Single,
                packet_type: PacketType::Data,
                hops: 0,
            },
            ifac: None,
            destination,
            transport: None,
            context: crate::packet::PacketContext::None,
            data: StaticBuffer::new(),
        };

        let (forwarded, next_iface) = table.handle_packet(&packet);
        assert_eq!(next_iface, Some(iface));
        assert_eq!(forwarded.header.ifac_flag, IfacFlag::Open);
        assert_eq!(forwarded.header.header_type, HeaderType::Type2);
        assert_eq!(forwarded.header.propagation_type, PropagationType::Transport);
        assert_eq!(forwarded.transport, Some(next_hop));
    }
}
