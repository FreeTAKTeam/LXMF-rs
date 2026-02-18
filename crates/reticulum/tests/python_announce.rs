use reticulum::destination::DestinationAnnounce;
use reticulum::hash::AddressHash;
use reticulum::packet::{
    ContextFlag, DestinationType, Header, HeaderType, IfacFlag, Packet, PacketContext,
    PacketDataBuffer, PacketType, PropagationType,
};

#[test]
fn python_announce_signature_validates() {
    let dest_hex = "0808d20b72a7f968dafaeaad1c2e7d00";
    let announce_hex = "5f70fae290f868328af11f4b3f67cf00853dc5d661eec86c04637c4bc6be5406074d6ad1b0c2a379b95ac7d1e44494e4b5f2bef206c1705db9e7284c9c4ec5fb6ec60bc318e2c0f0d9089aa64edd8d006979172c509403ee83e113f2fe25292279ef08341ab215fba00e62b48fc41c64bfd7535e27ae08caad5e06f85dbae40008284d8f5fc126e9f8cb4b32d088d85d56b9fcdf0dba8d62ab6e0db0f4ab061bda3d7a3a1b76406d64828b5c7766743d9c1a640692c409507974686f6e20525808";

    let destination = AddressHash::new_from_hex_string(dest_hex).expect("dest hash");
    let announce_data = hex::decode(announce_hex).expect("announce hex");

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
        destination,
        transport: None,
        context: PacketContext::None,
        data: PacketDataBuffer::new_from_slice(&announce_data),
    };

    let info = DestinationAnnounce::validate(&packet).expect("valid python announce");
    let ratchet = info.ratchet.expect("ratchet included");
    let ratchet_start = 64 + 10 + 10;
    let ratchet_end = ratchet_start + 32;
    let expected_ratchet = &announce_data[ratchet_start..ratchet_end];
    assert_eq!(ratchet.as_slice(), expected_ratchet);
}
