#[test]
fn rns_1_5_channel_packet_uses_negotiated_link_mdu_above_legacy_packet_mdu() {
    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut outbound = Link::new(destination, tx.clone());
    let request = outbound.request_with_mtu(1024);
    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(
        outbound.handle_packet(&inbound.prove(), iface),
        LinkHandleResult::Activated
    ));
    let payload = vec![0x5a; 600];
    let (_sequence, packet) =
        inbound.send_channel_message(0xCAFE, payload).expect("large channel packet");
    assert!(packet.data.len() > crate::packet::PACKET_MDU);
}
