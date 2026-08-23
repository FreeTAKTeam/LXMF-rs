use crate::hash::AddressHash;
use crate::iface::RxMessage;
use crate::packet::PacketType;

use super::super::InboundTrafficClass;

pub(super) fn inbound_traffic_class(
    message: &RxMessage,
    path_request: AddressHash,
) -> InboundTrafficClass {
    if message.packet.header.packet_type == PacketType::Announce {
        InboundTrafficClass::Announce
    } else if message.packet.destination == path_request {
        InboundTrafficClass::PathRequest
    } else {
        InboundTrafficClass::Data
    }
}
