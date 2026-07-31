// Issue #516: the same-iface known-path suppression is specific to
// InterfaceMode::Roaming (reference Transport.py ~line 3044). Other
// interface modes must still answer when the known next hop is attached
// to the requesting interface.

#[tokio::test]
async fn roaming_suppression_only_applies_to_roaming_mode() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let iface = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        *manager
            .new_channel_with_role_and_mode(
                16,
                crate::iface::IfaceRole::Unicast,
                crate::iface::InterfaceMode::AccessPoint,
            )
            .address()
    };

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let mut announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    announce.header.hops = 2;
    let destination = announce.destination;

    handle_announce(&announce, handler.lock().await, iface, crate::iface::IfaceSource::None).await;

    let path_request = {
        let mut guard = handler.lock().await;
        guard.path_requests.generate(&destination, Some(vec![0x67; crate::hash::ADDRESS_HASH_SIZE]))
    };

    {
        let mut guard = handler.lock().await;
        handle_path_request(&path_request, &mut guard, iface).await;
    }

    let guard = handler.lock().await;
    let response = guard
        .announce_table
        .pending_response_for_destination(&destination)
        .expect("access-point iface must answer a known-path request even on the learned iface");
    assert_eq!(response.response_to_iface, Some(iface));
    assert_eq!(response.packet.context, PacketContext::PathResponse);
}
