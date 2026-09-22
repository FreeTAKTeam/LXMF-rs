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
