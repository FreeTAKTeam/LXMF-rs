use lxmf::message::{MessageMethod, MessageState, TransportMethod, UnverifiedReason};

#[test]
fn enum_values_match_python() {
    assert_eq!(MessageState::Generating.as_u8(), 0x00);
    assert_eq!(MessageState::Outbound.as_u8(), 0x01);
    assert_eq!(MessageState::Sending.as_u8(), 0x02);
    assert_eq!(MessageState::Sent.as_u8(), 0x04);
    assert_eq!(MessageState::Delivered.as_u8(), 0x08);
    assert_eq!(MessageState::Rejected.as_u8(), 0xFD);
    assert_eq!(MessageState::Cancelled.as_u8(), 0xFE);
    assert_eq!(MessageState::Failed.as_u8(), 0xFF);

    assert_eq!(MessageMethod::Unknown.as_u8(), 0x00);
    assert_eq!(MessageMethod::Packet.as_u8(), 0x01);
    assert_eq!(MessageMethod::Resource.as_u8(), 0x02);
    assert_eq!(MessageMethod::Paper.as_u8(), 0x05);

    assert_eq!(TransportMethod::Opportunistic.as_u8(), 0x01);
    assert_eq!(TransportMethod::Direct.as_u8(), 0x02);
    assert_eq!(TransportMethod::Propagated.as_u8(), 0x03);
    assert_eq!(TransportMethod::Paper.as_u8(), 0x05);

    assert_eq!(UnverifiedReason::SourceUnknown.as_u8(), 0x01);
    assert_eq!(UnverifiedReason::SignatureInvalid.as_u8(), 0x02);
}
