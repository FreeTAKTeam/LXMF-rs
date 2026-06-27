use super::*;

pub(super) fn should_encrypt_packet(packet: &Packet) -> bool {
    if packet.header.packet_type != PacketType::Data {
        return false;
    }
    if packet.header.destination_type != DestinationType::Single {
        return false;
    }
    !matches!(
        packet.context,
        PacketContext::Resource
            | PacketContext::ResourceAdvrtisement
            | PacketContext::ResourceRequest
            | PacketContext::ResourceHashUpdate
            | PacketContext::ResourceProof
            | PacketContext::ResourceInitiatorCancel
            | PacketContext::ResourceReceiverCancel
            | PacketContext::KeepAlive
            | PacketContext::CacheRequest
    )
}
