use lxmf_core::WireMessage;
use rns_core::{
    destination::{DestinationAnnounce, DestinationName, SingleInputDestination},
    hash::{AddressHash, Hash},
    identity::PrivateIdentity,
    packet::{ContextFlag, DestinationType, HeaderType, Packet, PacketContext, PacketType},
};

struct IdentityVector {
    label: &'static str,
    private_key_hex: &'static str,
    public_key_hex: &'static str,
    identity_hash_hex: &'static str,
    name_hash_hex: &'static str,
    destination_hash_hex: &'static str,
}

struct AnnounceVector {
    label: &'static str,
    context_flag: ContextFlag,
    wire_bytes_hex: &'static str,
    destination_hash_hex: &'static str,
    app_data_hex: &'static str,
    body_name_hash_hex: &'static str,
    body_random_hash_hex: &'static str,
    body_ratchet_pub_hex: Option<&'static str>,
}

struct LxmfVector {
    label: &'static str,
    title_utf8: &'static str,
    content_utf8: &'static str,
    lxmf_packed_hex: &'static str,
    opportunistic_plaintext_hex: &'static str,
    token_ciphertext_hex: &'static str,
    destination_hash_hex: &'static str,
    source_hash_hex: &'static str,
    signature_hex: &'static str,
    msgpack_payload_hex: &'static str,
}

struct LinkVector {
    label: &'static str,
    linkrequest_raw_hex: &'static str,
    linkrequest_body_hex: &'static str,
    link_id_hex: &'static str,
    lrproof_raw_hex: &'static str,
    lrproof_body_hex: &'static str,
    lrrtt_raw_hex: &'static str,
    lrrtt_body_hex: &'static str,
    lrrtt_plaintext_hex: &'static str,
    shared_secret_hex: &'static str,
    derived_key_hex: &'static str,
}

const IDENTITY_VECTORS: &[IdentityVector] = &[
    IdentityVector {
        label: "alice",
        private_key_hex: "587e730a70d24e971efa8c146e554996d70bff45b2033d336e2c078dc63d3645bef79d95bf6b253827a2e7e81a13ab0b10a908fd158581d1827095b788169e93",
        public_key_hex: "76fce269b2356a51b6a832a1a25099155acb20733b453f9538aaa8069e854d5a780708b44424373474ee1607c3f2b4a1cd5643de508e106e6b8cf4a10f00ec7c",
        identity_hash_hex: "28d43a11abc1094301a59ed3b44f127b",
        name_hash_hex: "6ec60bc318e2c0f0d908",
        destination_hash_hex: "c33c40a5b030596d95617dc4ca163aae",
    },
    IdentityVector {
        label: "bob",
        private_key_hex: "0f453e75d564532f2fa671aea79e9a714e4564e1ff833d1df19986fe8a36aa219a6acdad966af7d006cfd393ca8278c608978bcaefa5b5f24db867179f83a863",
        public_key_hex: "92331490ac7c5db96102f80ffc64d71330907a5aea969b8617b7b2f3e0f8352a274e3172cbb18bdb14ccc1178fd66a8a811be97690d30985c75649a2b07dc76a",
        identity_hash_hex: "c090410e5b5bf8956194c1872dccec3b",
        name_hash_hex: "6ec60bc318e2c0f0d908",
        destination_hash_hex: "9695d17f22fa6e45d2b0cd3439a7ca7e",
    },
];

const ANNOUNCE_VECTORS: &[AnnounceVector] = &[
    AnnounceVector {
        label: "alice_lxmf_no_ratchet",
        context_flag: ContextFlag::Unset,
        wire_bytes_hex: "0100d9587f0be518490591c181755404d8510076fce269b2356a51b6a832a1a25099155acb20733b453f9538aaa8069e854d5a780708b44424373474ee1607c3f2b4a1cd5643de508e106e6b8cf4a10f00ec7c8b5739ff0fe7afaf7157a1a2a3a4a5006553f1009b0f121c51fda21cbce043b5b9d89b09817f29d320d2027c0f6c67144ace9d577722791e9ca1c5d24678ced4166862d77650756a98369c48a8455865c279e20092c409416c6963655465737400",
        destination_hash_hex: "d9587f0be518490591c181755404d851",
        app_data_hex: "92c409416c6963655465737400",
        body_name_hash_hex: "8b5739ff0fe7afaf7157",
        body_random_hash_hex: "a1a2a3a4a5006553f100",
        body_ratchet_pub_hex: None,
    },
    AnnounceVector {
        label: "alice_lxmf_with_ratchet",
        context_flag: ContextFlag::Set,
        wire_bytes_hex: "2100141410d233872609cf7b9f075afb4ebb0076fce269b2356a51b6a832a1a25099155acb20733b453f9538aaa8069e854d5a780708b44424373474ee1607c3f2b4a1cd5643de508e106e6b8cf4a10f00ec7c5130f0a9b2e01f693bd0a1a2a3a4a5006553f100cd700e88f9e99b19c1a8a8dcd58182fd101e5e032a69ce317fde23e8ee265c51e4985b2edb0694b51ddcb9e1aa73f60acd297bf8dd087056f90c2c9ee1e47587feef3b5f6f18de160bad45e49abe5f8c7d74ccb893e207061136f5222434620392c409416c6963655465737400",
        destination_hash_hex: "141410d233872609cf7b9f075afb4ebb",
        app_data_hex: "92c409416c6963655465737400",
        body_name_hash_hex: "5130f0a9b2e01f693bd0",
        body_random_hash_hex: "a1a2a3a4a5006553f100",
        body_ratchet_pub_hex: Some("cd700e88f9e99b19c1a8a8dcd58182fd101e5e032a69ce317fde23e8ee265c51"),
    },
];

const LXMF_VECTORS: &[LxmfVector] = &[
    LxmfVector {
        label: "alice_to_bob_simple",
        title_utf8: "hello",
        content_utf8: "hi bob",
        lxmf_packed_hex: "9695d17f22fa6e45d2b0cd3439a7ca7ec33c40a5b030596d95617dc4ca163aaedc758d54b21a2ca01a8fcc3e21c45eb60918d2dc64508037ce640e000a295a81951e5a7d0f8fedb90ec4df0b0a05b437b43d6692c9dd7faa98c4b679935a940e94cb41d954fc40000000c40568656c6c6fc406686920626f6280",
        opportunistic_plaintext_hex: "c33c40a5b030596d95617dc4ca163aaedc758d54b21a2ca01a8fcc3e21c45eb60918d2dc64508037ce640e000a295a81951e5a7d0f8fedb90ec4df0b0a05b437b43d6692c9dd7faa98c4b679935a940e94cb41d954fc40000000c40568656c6c6fc406686920626f6280",
        token_ciphertext_hex: "21c3332b61be6a7b6ab8461e155651b17501b6e07532ecf9ab6661bd5a2ca57511223344556677889900aabbccddeeffefa9d24b76e1adf393cfd588214e236e219743697af96912b8eae84f3a1f28a3d68abd62f3e42c6944015c3d00e5e7aa8af732123d079ab10353597669c8cd3ba57cfae3a28ea1a99a44e0b492ba5deedd23232d2edab78fa037967757808c8578496aee7b21c70ce2476c54540d96d928e8ddf35c6bfb5d76261c07f1bb48af9d7bec8261cd30f3b03986614ba93173",
        destination_hash_hex: "9695d17f22fa6e45d2b0cd3439a7ca7e",
        source_hash_hex: "c33c40a5b030596d95617dc4ca163aae",
        signature_hex: "dc758d54b21a2ca01a8fcc3e21c45eb60918d2dc64508037ce640e000a295a81951e5a7d0f8fedb90ec4df0b0a05b437b43d6692c9dd7faa98c4b679935a940e",
        msgpack_payload_hex: "94cb41d954fc40000000c40568656c6c6fc406686920626f6280",
    },
    LxmfVector {
        label: "alice_to_bob_with_fields",
        title_utf8: "meeting",
        content_utf8: "see attached",
        lxmf_packed_hex: "9695d17f22fa6e45d2b0cd3439a7ca7ec33c40a5b030596d95617dc4ca163aaef23eb2c325e59493c187b8fa1cc0efb6306f3eeed159a33aeec954576cbd354451caaa176aff84b57cc154b4e4113197b5eb92f1ec6a0e7635fa3d0508c67e0194cb41d954fc40000000c4076d656574696e67c40c7365652061747461636865648201a26b31022a",
        opportunistic_plaintext_hex: "c33c40a5b030596d95617dc4ca163aaef23eb2c325e59493c187b8fa1cc0efb6306f3eeed159a33aeec954576cbd354451caaa176aff84b57cc154b4e4113197b5eb92f1ec6a0e7635fa3d0508c67e0194cb41d954fc40000000c4076d656574696e67c40c7365652061747461636865648201a26b31022a",
        token_ciphertext_hex: "21c3332b61be6a7b6ab8461e155651b17501b6e07532ecf9ab6661bd5a2ca57511223344556677889900aabbccddeeffefa9d24b76e1adf393cfd588214e236eac9dde2777640fdd62e86256d9ddac812eeda9277056b5652b9d83ca7d0da7203a5f51b69f7d35a58da4b6a13562a12145a98810d1b89fec9a70e947c50eee6482f3fda4165fd6eef25819fc5093d5f2aaaf7b8911689a3eeaf0131816bac4041923df9e64a807d9809c35cc7bed027de94dc42a7af10261f14053dc62d77d54e66f60dfb83763a6f66798c51eeabcd2",
        destination_hash_hex: "9695d17f22fa6e45d2b0cd3439a7ca7e",
        source_hash_hex: "c33c40a5b030596d95617dc4ca163aae",
        signature_hex: "f23eb2c325e59493c187b8fa1cc0efb6306f3eeed159a33aeec954576cbd354451caaa176aff84b57cc154b4e4113197b5eb92f1ec6a0e7635fa3d0508c67e01",
        msgpack_payload_hex: "94cb41d954fc40000000c4076d656574696e67c40c7365652061747461636865648201a26b31022a",
    },
];

const LINK_VECTORS: &[LinkVector] = &[LinkVector {
    label: "alice_to_bob_aes256cbc",
    linkrequest_raw_hex: "02008c670c64308e0325ea0fd7c72787449d007b4e909bbe7ffe44c465a220037d608ee35897d31ef972f07f74892cb0f73f13a09aa5f47a6759802ff955f8dc2d2a14a5c99d23be97f864127ff9383455a4f02001f4",
    linkrequest_body_hex: "7b4e909bbe7ffe44c465a220037d608ee35897d31ef972f07f74892cb0f73f13a09aa5f47a6759802ff955f8dc2d2a14a5c99d23be97f864127ff9383455a4f02001f4",
    link_id_hex: "7ee5fe3e4952c9ac4519b537f6278474",
    lrproof_raw_hex: "0f007ee5fe3e4952c9ac4519b537f6278474ff1de2168a36a816163aec0bb0749ff6792f78eb4f7b39156f8ee5c8693e83ebd67439ac28d9e4603334428713154edd04395b0b8acec2f703c05c3d38af133e0c7b0d47d93427f8311160781c7c733fd89f88970aef490d8aa0ee19a4cb8a1b142001f4",
    lrproof_body_hex: "1de2168a36a816163aec0bb0749ff6792f78eb4f7b39156f8ee5c8693e83ebd67439ac28d9e4603334428713154edd04395b0b8acec2f703c05c3d38af133e0c7b0d47d93427f8311160781c7c733fd89f88970aef490d8aa0ee19a4cb8a1b142001f4",
    lrrtt_raw_hex: "0c007ee5fe3e4952c9ac4519b537f6278474fe4444444444444444444444444444444408eed3f995972368190afe8851fcc6850767824d5c8980840a16d4e5c873a5977669bb9c488d68ecf0da3ff6111cf630",
    lrrtt_body_hex: "4444444444444444444444444444444408eed3f995972368190afe8851fcc6850767824d5c8980840a16d4e5c873a5977669bb9c488d68ecf0da3ff6111cf630",
    lrrtt_plaintext_hex: "cb3fa999999999999a",
    shared_secret_hex: "5bf22caf31c0316785b0b9bc60e56d48582ce59435ce5b3c028052be42631e0f",
    derived_key_hex: "d4c8238d23a1810c3dbe4caec15253d5a86d7fe6afa8dfa76f915579723fd88cbcd2ab3a0cd96f5b6ffd8abec8307f05cd791dc9c4fca900f706b0313a51ab65",
}];

#[test]
fn reticulum_spec_vectors_are_available_as_rust_fixtures() {
    assert_eq!(IDENTITY_VECTORS.len(), 2);
    assert_eq!(ANNOUNCE_VECTORS.len(), 2);
    assert_eq!(LXMF_VECTORS.len(), 2);
    assert_eq!(LINK_VECTORS.len(), 1);
}

#[test]
fn identity_vectors_match_rns_key_and_destination_derivation() {
    let destination_name = DestinationName::new("lxmf", "delivery");
    assert_eq!(
        bytes_to_hex(destination_name.as_name_hash_slice()),
        IDENTITY_VECTORS[0].name_hash_hex
    );

    for vector in IDENTITY_VECTORS {
        let private_key = from_hex(vector.private_key_hex);
        let identity = PrivateIdentity::from_private_key_bytes(&private_key)
            .unwrap_or_else(|err| panic!("{} private key fixture failed: {err:?}", vector.label));

        assert_eq!(
            identity.as_identity().to_hex_string(),
            vector.public_key_hex,
            "{}",
            vector.label
        );
        assert_eq!(
            identity.as_identity().address_hash.to_hex_string(),
            vector.identity_hash_hex,
            "{}",
            vector.label
        );

        let destination = SingleInputDestination::new(identity, destination_name);
        assert_eq!(
            destination.desc.address_hash.to_hex_string(),
            vector.destination_hash_hex,
            "{}",
            vector.label
        );
    }
}

#[test]
fn announce_vectors_parse_and_validate_as_reticulum_announces() {
    for vector in ANNOUNCE_VECTORS {
        let wire = from_hex(vector.wire_bytes_hex);
        let packet = Packet::from_bytes(&wire)
            .unwrap_or_else(|err| panic!("{} parse: {err:?}", vector.label));

        assert_eq!(packet.header.header_type, HeaderType::Type1, "{}", vector.label);
        assert_eq!(packet.header.packet_type, PacketType::Announce, "{}", vector.label);
        assert_eq!(packet.header.context_flag, vector.context_flag, "{}", vector.label);
        assert_eq!(
            packet.destination.to_hex_string(),
            vector.destination_hash_hex,
            "{}",
            vector.label
        );

        let info = DestinationAnnounce::validate(&packet)
            .unwrap_or_else(|err| panic!("{} validate: {err:?}", vector.label));
        assert_eq!(info.destination.desc.address_hash, packet.destination, "{}", vector.label);
        assert_eq!(info.app_data, from_hex(vector.app_data_hex), "{}", vector.label);

        let body = packet.data.as_slice();
        assert_eq!(bytes_to_hex(&body[64..74]), vector.body_name_hash_hex, "{}", vector.label);
        assert_eq!(bytes_to_hex(&body[74..84]), vector.body_random_hash_hex, "{}", vector.label);
        assert_eq!(
            info.ratchet.map(|ratchet| bytes_to_hex(&ratchet)),
            vector.body_ratchet_pub_hex.map(str::to_string),
            "{}",
            vector.label
        );
    }
}

#[test]
fn opportunistic_lxmf_vectors_unpack_and_repack() {
    for vector in LXMF_VECTORS {
        let packed = from_hex(vector.lxmf_packed_hex);
        let message = WireMessage::unpack(&packed)
            .unwrap_or_else(|err| panic!("{} unpack: {err:?}", vector.label));

        assert_eq!(
            bytes_to_hex(&message.destination),
            vector.destination_hash_hex,
            "{}",
            vector.label
        );
        assert_eq!(bytes_to_hex(&message.source), vector.source_hash_hex, "{}", vector.label);
        assert_eq!(
            message.signature.map(|signature| bytes_to_hex(&signature)).as_deref(),
            Some(vector.signature_hex),
            "{}",
            vector.label
        );
        assert_eq!(
            message.payload.title.as_ref().map(|title| title.as_ref()),
            Some(vector.title_utf8.as_bytes()),
            "{}",
            vector.label
        );
        assert_eq!(
            message.payload.content.as_ref().map(|content| content.as_ref()),
            Some(vector.content_utf8.as_bytes()),
            "{}",
            vector.label
        );
        assert!(message.payload.fields.is_some(), "{}", vector.label);
        assert_eq!(
            bytes_to_hex(&message.payload.to_msgpack().expect("payload msgpack")),
            vector.msgpack_payload_hex,
            "{}",
            vector.label
        );
        assert_eq!(
            &packed[16..],
            from_hex(vector.opportunistic_plaintext_hex).as_slice(),
            "{}",
            vector.label
        );
        assert_eq!(message.pack().expect("repack"), packed, "{}", vector.label);

        let token_ciphertext = from_hex(vector.token_ciphertext_hex);
        assert!(
            token_ciphertext.len() > 32,
            "{} token fixture must include ephemeral key",
            vector.label
        );
    }
}

#[test]
fn link_vectors_parse_linkrequest_lrproof_and_lrrtt_packets() {
    for vector in LINK_VECTORS {
        let linkrequest = Packet::from_bytes(&from_hex(vector.linkrequest_raw_hex))
            .unwrap_or_else(|err| panic!("{} linkrequest: {err:?}", vector.label));
        assert_eq!(linkrequest.header.packet_type, PacketType::LinkRequest, "{}", vector.label);
        assert_eq!(
            linkrequest.header.destination_type,
            DestinationType::Single,
            "{}",
            vector.label
        );
        assert_eq!(
            bytes_to_hex(linkrequest.data.as_slice()),
            vector.linkrequest_body_hex,
            "{}",
            vector.label
        );
        assert_eq!(
            link_id_from_request(&linkrequest).to_hex_string(),
            vector.link_id_hex,
            "{}",
            vector.label
        );

        let lrproof = Packet::from_bytes(&from_hex(vector.lrproof_raw_hex))
            .unwrap_or_else(|err| panic!("{} lrproof: {err:?}", vector.label));
        assert_eq!(lrproof.header.packet_type, PacketType::Proof, "{}", vector.label);
        assert_eq!(lrproof.header.destination_type, DestinationType::Link, "{}", vector.label);
        assert_eq!(lrproof.context, PacketContext::LinkRequestProof, "{}", vector.label);
        assert_eq!(lrproof.destination.to_hex_string(), vector.link_id_hex, "{}", vector.label);
        assert_eq!(
            bytes_to_hex(lrproof.data.as_slice()),
            vector.lrproof_body_hex,
            "{}",
            vector.label
        );

        let lrrtt = Packet::from_bytes(&from_hex(vector.lrrtt_raw_hex))
            .unwrap_or_else(|err| panic!("{} lrrtt: {err:?}", vector.label));
        assert_eq!(lrrtt.header.packet_type, PacketType::Data, "{}", vector.label);
        assert_eq!(lrrtt.header.destination_type, DestinationType::Link, "{}", vector.label);
        assert_eq!(lrrtt.context, PacketContext::LinkRTT, "{}", vector.label);
        assert_eq!(lrrtt.destination.to_hex_string(), vector.link_id_hex, "{}", vector.label);
        assert_eq!(bytes_to_hex(lrrtt.data.as_slice()), vector.lrrtt_body_hex, "{}", vector.label);

        assert_eq!(from_hex(vector.shared_secret_hex).len(), 32, "{}", vector.label);
        assert_eq!(from_hex(vector.derived_key_hex).len(), 64, "{}", vector.label);
        assert_eq!(from_hex(vector.lrrtt_plaintext_hex).len(), 9, "{}", vector.label);
    }
}

fn link_id_from_request(packet: &Packet) -> AddressHash {
    let body = packet.data.as_slice();
    let key_material_len = 64usize.min(body.len());
    let mut hash_input = Vec::with_capacity(1 + 16 + 1 + key_material_len);
    hash_input.push(packet.header.to_meta() & 0b0000_1111);
    hash_input.extend_from_slice(packet.destination.as_slice());
    hash_input.push(packet.context as u8);
    hash_input.extend_from_slice(&body[..key_material_len]);
    AddressHash::new_from_hash(&Hash::new_from_slice(&hash_input))
}

fn from_hex(hex: &str) -> Vec<u8> {
    assert!(hex.len() % 2 == 0, "hex string must have even length");
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = hex_value(pair[0]);
            let low = hex_value(pair[1]);
            (high << 4) | low
        })
        .collect()
}

fn hex_value(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => panic!("invalid hex byte: {byte}"),
    }
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
