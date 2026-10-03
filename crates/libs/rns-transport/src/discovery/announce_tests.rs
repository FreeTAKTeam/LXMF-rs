use super::*;

fn backbone() -> DiscoverableInterface {
    DiscoverableInterface {
        interface_type: "BackboneInterface".to_string(),
        transport: true,
        transport_id: [0x11; 16],
        name: "  Test\nBackbone  ".to_string(),
        latitude: Some(44.0),
        longitude: Some(-63.0),
        height: Some(10.0),
        operator_lxmf_address: None,
        reachable_on: Some("relay.example".to_string()),
        port: Some(4242),
        ifac_netname: Some("field-net".to_string()),
        ifac_netkey: Some("shared-key".to_string()),
        frequency: None,
        bandwidth: None,
        spreading_factor: None,
        coding_rate: None,
        modulation: None,
        channel: None,
    }
}

#[test]
fn stamp_workblock_matches_pinned_python_lxstamper_vector() {
    let workblock = stamp_workblock(b"discovery-vector", WORKBLOCK_EXPAND_ROUNDS);
    assert_eq!(
        hex::encode(Sha256::digest(&workblock)),
        "3973a6e49267a6acbeacade2e3babd7f9fd88250876ca873d750fc1b71cfc9ef"
    );
    assert_eq!(stamp_value(&workblock, &[0; STAMP_SIZE]), 1);
}

#[test]
fn plain_announce_roundtrip_validates_stamp_and_python_fields() {
    let payload = encode_plain_announce(&backbone(), 5).expect("encode");
    let decoded = decode_plain_announce(
        &payload,
        "22222222222222222222222222222222",
        &["22222222222222222222222222222222".to_string()],
        2,
        1234.5,
        5,
    )
    .expect("decode");
    assert_eq!(decoded.interface_type, "BackboneInterface");
    assert_eq!(decoded.name, "TestBackbone");
    assert_eq!(decoded.transport_id, "11".repeat(16));
    assert_eq!(decoded.reachable_on.as_deref(), Some("relay.example"));
    assert_eq!(decoded.port, Some(4242));
    assert_eq!(decoded.hops, 2);
    assert!(decoded.value >= 5);
    assert_eq!(decoded.impl_name.as_deref(), Some("LXMF-rs"));
    assert_eq!(decoded.version.as_deref(), Some(env!("CARGO_PKG_VERSION")));
}

#[test]
fn rns_1_5_5_ifac_announcement_omits_empty_and_ambiguous_values() {
    let mut interface = backbone();
    interface.ifac_netname = Some("None".to_string());
    interface.ifac_netkey = Some("  ".to_string());
    let packed = encode_interface(&interface).expect("encode interface");
    let decoded = rmpv::decode::read_value(&mut std::io::Cursor::new(packed))
        .expect("decode interface map");
    let fields = decoded.as_map().expect("interface map");
    assert!(!fields.iter().any(|(key, _)| key.as_i64() == Some(IFAC_NETNAME)));
    assert!(!fields.iter().any(|(key, _)| key.as_i64() == Some(IFAC_NETKEY)));

    let mut legacy = fields.to_vec();
    legacy.push((Value::from(IFAC_NETNAME), Value::from("None")));
    legacy.push((Value::from(IFAC_NETKEY), Value::from(42)));
    let mut legacy_packed = Vec::new();
    rmpv::encode::write_value(&mut legacy_packed, &Value::Map(legacy)).expect("encode legacy");
    let record = decode_interface(&legacy_packed, &[0; STAMP_SIZE], 4, "22", 1, 1.0)
        .expect("legacy IFAC fields must not invalidate discovery");
    assert_eq!(record.ifac_netname, None);
    assert_eq!(record.ifac_netkey, None);
}

#[test]
fn rns_1_5_5_python_discovery_map_decodes_verified_metadata_and_legacy_none() {
    // Packed with msgpack and RNS.Discovery constants from the exact Python
    // 1.5.5 tag (7f2b3b9b524c9386316379af1313b43a5e4f7a5d).
    let packed = hex::decode(concat!(
        "8c00b14261636b626f6e65496e7465726661636501c3ccfec410",
        "11111111111111111111111111111111ccfda3524e53ccfca5312e352e35",
        "ccffac507974686f6e20312e352e3503c004c005c002ad72656c61792e",
        "6578616d706c6506cd109207a44e6f6e65"
    ))
    .expect("pinned Python discovery map hex");
    let decoded = decode_interface(&packed, &[0; STAMP_SIZE], 4, "network", 1, 1.0)
        .expect("decode Python discovery map");
    assert_eq!(decoded.impl_name.as_deref(), Some("RNS"));
    assert_eq!(decoded.version.as_deref(), Some("1.5.5"));
    assert_eq!(decoded.interface_type, "BackboneInterface");
    assert_eq!(decoded.ifac_netname, None);
    assert!(crate::discovery::lifecycle::plan_autoconnect(&decoded, &[]).is_some());
}

#[test]
fn rns_1_5_operator_lxmf_address_roundtrips() {
    let mut interface = backbone();
    interface.operator_lxmf_address = Some([0x42; 16]);
    let payload = encode_plain_announce(&interface, 4).expect("encode");
    let decoded = decode_plain_announce(&payload, "22", &[], 1, 1.0, 4).expect("decode");
    assert_eq!(decoded.operator_lxmf_address.as_deref(), Some("42424242424242424242424242424242"));
}

#[test]
fn rns_1_5_wrong_length_operator_address_is_ignored_like_python() {
    let packed = encode_interface(&backbone()).expect("encode interface");
    let decoded = rmpv::decode::read_value(&mut std::io::Cursor::new(packed))
        .expect("decode interface map");
    let mut entries = decoded.as_map().expect("interface map").to_vec();
    entries.push((Value::from(OPERATOR_LXMF_ADDRESS), Value::Binary(vec![0x42; 15])));
    let decoded = Value::Map(entries);
    let mut malformed_optional = Vec::new();
    rmpv::encode::write_value(&mut malformed_optional, &decoded).expect("encode malformed field");

    let record = decode_interface(&malformed_optional, &[0; STAMP_SIZE], 4, "22", 1, 1.0)
        .expect("wrong-length optional address must not invalidate the announce");
    assert_eq!(record.operator_lxmf_address, None);
}

#[test]
fn announce_rejects_unauthorized_source_and_tampered_stamp() {
    let mut payload = encode_plain_announce(&backbone(), 4).expect("encode");
    assert_eq!(
        decode_plain_announce(
            &payload,
            "22222222222222222222222222222222",
            &["33333333333333333333333333333333".to_string()],
            1,
            1.0,
            4,
        ),
        Err(DiscoveryAnnounceError::UnauthorizedSource)
    );
    let last = payload.len() - 1;
    payload[last] ^= 0xff;
    assert_eq!(
        decode_plain_announce(&payload, "22222222222222222222222222222222", &[], 1, 1.0, 32,),
        Err(DiscoveryAnnounceError::InvalidStamp)
    );
}

#[test]
fn encrypted_announce_requires_and_uses_decryptor() {
    let payload = encode_announce(
        &backbone(),
        4,
        Some(|body: &[u8]| Some(body.iter().map(|byte| byte ^ 0xaa).collect())),
    )
    .expect("encrypted encode");
    assert_eq!(payload[0], FLAG_ENCRYPTED);
    assert_eq!(
        decode_plain_announce(&payload, "22", &[], 1, 1.0, 4),
        Err(DiscoveryAnnounceError::EncryptedWithoutDecryptor)
    );
    let decoded = decode_announce(
        &payload,
        "22",
        &[],
        1,
        1.0,
        4,
        Some(|body: &[u8]| Some(body.iter().map(|byte| byte ^ 0xaa).collect())),
    )
    .expect("encrypted decode");
    assert_eq!(decoded.name, "TestBackbone");
}
