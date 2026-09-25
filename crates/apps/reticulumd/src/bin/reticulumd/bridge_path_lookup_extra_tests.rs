#[test]
fn path_lookup_bridge_dispatches_scoped_request_path() {
    let (bridge, iface) = bridge_with_iface();

    bridge
        .request_path_scoped(
            "00112233445566778899aabbccddeeff",
            Some(&hex::encode(iface.as_slice())),
            Some(&[1, 2, 3, 4]),
        )
        .expect("dispatch scoped path request");
}

#[test]
fn path_lookup_bridge_rejects_unknown_scoped_iface() {
    let bridge = bridge();

    let err = bridge
        .request_path_scoped(
            "00112233445566778899aabbccddeeff",
            Some("aabbccddeeff00112233445566778899"),
            Some(&[1, 2, 3, 4]),
        )
        .expect_err("unknown scoped iface should fail");

    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    assert!(err.to_string().contains("scoped path request interface"));
}

#[test]
fn path_lookup_bridge_rejects_invalid_scoped_iface() {
    let bridge = bridge();

    let err = bridge
        .request_path_scoped(
            "00112233445566778899aabbccddeeff",
            Some("abcd"),
            Some(&[1, 2, 3, 4]),
        )
        .expect_err("invalid scoped iface should fail");

    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("on_iface"));
}
#[test]
fn path_table_bridge_serializes_python_shaped_route_fields() {
    let destination = AddressHash::new_from_hex_string("00112233445566778899aabbccddeeff")
        .expect("destination hash");
    let next_hop = AddressHash::new_from_hex_string("8899aabbccddeeff0011223344556677")
        .expect("next-hop hash");
    let interface = AddressHash::new_from_hex_string("fedcba98765432100123456789abcdef")
        .expect("interface hash");
    let interface_hash = rns_transport::hash::Hash::new_from_slice(b"interface");

    let row = DaemonPathLookupBridge::path_table_entry_json(
        rns_transport::transport::TransportPathTableEntry {
            destination,
            timestamp_secs: 12.5,
            next_hop,
            hops: 2,
            expires_secs: 42.25,
            interface,
            interface_name: Some("uplink".to_owned()),
            interface_hash,
        },
        None,
    );

    assert_eq!(row["hash"], destination.to_hex_string());
    assert_eq!(row["timestamp"], 12.5);
    assert_eq!(row["via"], next_hop.to_hex_string());
    assert_eq!(row["hops"], 2);
    assert_eq!(row["expires"], 42.25);
    assert_eq!(row["interface"], "uplink");
    assert_eq!(row["interface_hash"], hex::encode(interface_hash.as_slice()));
}

#[test]
fn path_table_bridge_prefers_reference_tcp_client_representation() {
    let destination = AddressHash::new_from_hex_string("00112233445566778899aabbccddeeff")
        .expect("destination hash");
    let next_hop = AddressHash::new_from_hex_string("8899aabbccddeeff0011223344556677")
        .expect("next-hop hash");
    let interface = AddressHash::new_from_hex_string("fedcba98765432100123456789abcdef")
        .expect("interface hash");
    let interface_hash = rns_transport::hash::Hash::new_from_slice(b"interface");

    let row = DaemonPathLookupBridge::path_table_entry_json(
        rns_transport::transport::TransportPathTableEntry {
            destination,
            timestamp_secs: 12.5,
            next_hop,
            hops: 1,
            expires_secs: 42.25,
            interface,
            interface_name: Some("configured-name".to_owned()),
            interface_hash,
        },
        Some("TCPInterface[configured-name/127.0.0.1:4242]".to_owned()),
    );

    assert_eq!(row["interface"], "TCPInterface[configured-name/127.0.0.1:4242]");
}

#[test]
fn path_table_bridge_keeps_hash_when_interface_display_name_is_unavailable() {
    let destination = AddressHash::new_from_hex_string("00112233445566778899aabbccddeeff")
        .expect("destination hash");
    let next_hop = AddressHash::new_from_hex_string("8899aabbccddeeff0011223344556677")
        .expect("next-hop hash");
    let interface = AddressHash::new_from_hex_string("fedcba98765432100123456789abcdef")
        .expect("interface hash");
    let interface_hash = rns_transport::hash::Hash::new_from_slice(b"interface");
    let row = DaemonPathLookupBridge::path_table_entry_json(
        rns_transport::transport::TransportPathTableEntry {
            destination,
            timestamp_secs: 12.5,
            next_hop,
            hops: 2,
            expires_secs: 42.25,
            interface,
            interface_name: None,
            interface_hash,
        },
        None,
    );

    assert_eq!(row["interface"], JsonValue::Null);
    assert_eq!(row["interface_hash"], hex::encode(interface_hash.as_slice()));
}
