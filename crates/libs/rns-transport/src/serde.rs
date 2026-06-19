use crate::{
    buffer::{InputBuffer, OutputBuffer},
    error::RnsError,
    hash::AddressHash,
    packet::{Header, HeaderType, Packet, PacketContext, PacketDataBuffer, PACKET_DATA_MAX},
};

pub trait Serialize {
    fn serialize(&self, buffer: &mut OutputBuffer) -> Result<usize, RnsError>;
}

impl Serialize for AddressHash {
    fn serialize(&self, buffer: &mut OutputBuffer) -> Result<usize, RnsError> {
        buffer.write(self.as_slice())
    }
}

impl Serialize for Header {
    fn serialize(&self, buffer: &mut OutputBuffer) -> Result<usize, RnsError> {
        buffer.write(&[self.to_meta(), self.hops])
    }
}
impl Serialize for PacketContext {
    fn serialize(&self, buffer: &mut OutputBuffer) -> Result<usize, RnsError> {
        buffer.write(&[*self as u8])
    }
}

impl Serialize for Packet {
    fn serialize(&self, buffer: &mut OutputBuffer) -> Result<usize, RnsError> {
        self.header.serialize(buffer)?;

        if self.header.header_type == HeaderType::Type2 {
            if let Some(transport) = &self.transport {
                transport.serialize(buffer)?;
            }
        }

        self.destination.serialize(buffer)?;

        self.context.serialize(buffer)?;

        buffer.write(self.data.as_slice())
    }
}

impl Header {
    pub fn deserialize(buffer: &mut InputBuffer) -> Result<Header, RnsError> {
        let mut header = Header::from_meta(buffer.read_byte()?);
        header.hops = buffer.read_byte()?;

        Ok(header)
    }
}

impl AddressHash {
    pub fn deserialize(buffer: &mut InputBuffer) -> Result<AddressHash, RnsError> {
        let mut address = AddressHash::new_empty();

        buffer.read(address.as_mut_slice())?;

        Ok(address)
    }
}

impl PacketContext {
    pub fn deserialize(buffer: &mut InputBuffer) -> Result<PacketContext, RnsError> {
        Ok(PacketContext::from(buffer.read_byte()?))
    }
}
impl Packet {
    pub fn deserialize(buffer: &mut InputBuffer) -> Result<Packet, RnsError> {
        let header = Header::deserialize(buffer)?;

        let transport = if header.header_type == HeaderType::Type2 {
            Some(AddressHash::deserialize(buffer)?)
        } else {
            None
        };

        let destination = AddressHash::deserialize(buffer)?;

        let context = PacketContext::deserialize(buffer)?;

        let mut packet = Packet {
            header,
            ifac: None,
            destination,
            transport,
            context,
            data: PacketDataBuffer::new(),
        };

        let remaining = buffer.bytes_left();
        if remaining > PACKET_DATA_MAX {
            return Err(RnsError::OutOfMemory);
        }
        buffer.read(packet.data.accuire_buf(remaining))?;

        Ok(packet)
    }
}

#[cfg(test)]
mod tests {
    use rand_core::OsRng;

    use crate::{
        buffer::{InputBuffer, OutputBuffer},
        hash::AddressHash,
        packet::{
            ContextFlag, DestinationType, Header, HeaderType, IfacFlag, Packet, PacketContext,
            PacketDataBuffer, PacketType, PropagationType, PACKET_DATA_MAX, PACKET_MDU,
        },
    };

    use super::Serialize;

    #[test]
    fn serialize_packet() {
        let mut output_data = [0u8; 4096];

        let mut buffer = OutputBuffer::new(&mut output_data);

        let packet = Packet {
            header: Header {
                ifac_flag: IfacFlag::Open,
                header_type: HeaderType::Type1,
                context_flag: ContextFlag::Unset,
                propagation_type: PropagationType::Broadcast,
                destination_type: DestinationType::Single,
                packet_type: PacketType::Announce,
                hops: 0,
            },
            ifac: None,
            destination: AddressHash::new_from_rand(OsRng),
            transport: None,
            context: PacketContext::None,
            data: PacketDataBuffer::new(),
        };

        packet.serialize(&mut buffer).expect("serialized packet");

        log::debug!("{}", buffer);
    }

    #[test]
    fn deserialize_packet() {
        let mut output_data = [0u8; 4096];

        let mut buffer = OutputBuffer::new(&mut output_data);

        let mut packet = Packet {
            header: Header {
                ifac_flag: IfacFlag::Open,
                header_type: HeaderType::Type1,
                context_flag: ContextFlag::Unset,
                propagation_type: PropagationType::Broadcast,
                destination_type: DestinationType::Single,
                packet_type: PacketType::Announce,
                hops: 0,
            },
            ifac: None,
            destination: AddressHash::new_from_rand(OsRng),
            transport: None,
            context: PacketContext::None,
            data: PacketDataBuffer::new(),
        };

        packet.data.safe_write(b"Hello, world!");

        packet.serialize(&mut buffer).expect("serialized packet");

        let mut input_buffer = InputBuffer::new(buffer.as_slice());

        let new_packet = Packet::deserialize(&mut input_buffer).expect("deserialized packet");

        assert_eq!(packet.header, new_packet.header);
        assert_eq!(packet.destination, new_packet.destination);
        assert_eq!(packet.transport, new_packet.transport);
        assert_eq!(packet.context, new_packet.context);
        assert_eq!(packet.data.as_slice(), new_packet.data.as_slice());
    }

    #[test]
    fn deserialize_allows_large_tcp_resource_packet_data() {
        let payload_len = PACKET_MDU + 128;
        let packet = Packet {
            header: Header {
                ifac_flag: IfacFlag::Open,
                header_type: HeaderType::Type1,
                context_flag: ContextFlag::Set,
                propagation_type: PropagationType::Broadcast,
                destination_type: DestinationType::Link,
                packet_type: PacketType::Data,
                hops: 0,
            },
            ifac: None,
            destination: AddressHash::new([0x42; crate::hash::ADDRESS_HASH_SIZE]),
            transport: None,
            context: PacketContext::Resource,
            data: PacketDataBuffer::new_from_slice(&vec![0xA5; payload_len]),
        };

        let mut wire = vec![0u8; payload_len + 64];
        let mut output = OutputBuffer::new(&mut wire);
        packet.serialize(&mut output).expect("serialize large resource packet");

        let mut input = InputBuffer::new(output.as_slice());
        let decoded = Packet::deserialize(&mut input).expect("deserialize large resource packet");

        assert_eq!(decoded.context, PacketContext::Resource);
        assert_eq!(decoded.data.len(), payload_len);
        assert!(decoded.data.as_slice().iter().all(|byte| *byte == 0xA5));
    }

    #[test]
    fn deserialize_rejects_packet_data_beyond_tcp_cap() {
        let mut wire = Vec::with_capacity(PACKET_DATA_MAX + 64);
        wire.push(
            Header {
                ifac_flag: IfacFlag::Open,
                header_type: HeaderType::Type1,
                context_flag: ContextFlag::Set,
                propagation_type: PropagationType::Broadcast,
                destination_type: DestinationType::Link,
                packet_type: PacketType::Data,
                hops: 0,
            }
            .to_meta(),
        );
        wire.push(0);
        wire.extend_from_slice(&[0x24; crate::hash::ADDRESS_HASH_SIZE]);
        wire.push(PacketContext::Resource as u8);
        wire.resize(wire.len() + PACKET_DATA_MAX + 1, 0x11);

        let mut input = InputBuffer::new(&wire);
        assert!(Packet::deserialize(&mut input).is_err());
    }
}
