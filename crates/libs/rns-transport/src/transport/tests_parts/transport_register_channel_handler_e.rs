// ---------------------------------------------------------------------
// Per-peer virtual unicast iface registration
// (see TransportHandler::unicast_iface_for_source)
// ---------------------------------------------------------------------
//
// On receiving an announce from a UDP peer over a multicast iface, the
// transport registers a *virtual* iface pinned to that peer's
// SocketAddr in the iface's PeerRouting map. The virtual iface shares
// its tx channel with the host multicast iface; the host's tx task
// resolves the virtual hash to a unicast send on the same socket.
// This is what stops the 22 Mb/s LAN flood without creating separate
// per-peer sockets (which would bind to ephemeral ports and confuse
// ingress attribution).

fn peer_addr(port: u16) -> std::net::SocketAddr {
    use std::net::{IpAddr, Ipv4Addr};
    std::net::SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 112)), port)
}

/// Register a fake multicast iface (role-tagged only — no real socket)
/// plus a shared `PeerRouting` map, and hand the routing map to the
/// handler so `unicast_iface_for_source` can use it. Returns the
/// iface's `AddressHash`.
///
/// Mirrors what `Transport::add_multicast_udp_interface` would do,
/// but without spawning the real UdpInterface task (which needs real
/// sockets). Tests can still exercise the handler's registration /
/// cache / GC logic in isolation this way.
async fn register_fake_multicast_iface(transport: &Transport) -> AddressHash {
    let routing = Arc::new(Mutex::new(crate::iface::udp::PeerRouting::new()));
    let iface_hash = {
        let mgr = transport.iface_manager();
        let mut mgr = mgr.lock().await;
        let channel = mgr.new_channel_with_role(16, crate::iface::IfaceRole::Multicast);
        *channel.address()
    };
    transport.get_handler().lock().await.register_multicast_peer_routing(iface_hash, routing);
    iface_hash
}

async fn new_unicast_iface_in(transport: &Transport) -> AddressHash {
    let mgr = transport.iface_manager();
    let mut mgr = mgr.lock().await;
    let channel = mgr.new_channel(16);
    *channel.address()
}
#[tokio::test]
async fn delivery_link_available_tracks_python_router_direct_and_backchannel_maps() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let outbound = Arc::new(Mutex::new(Link::new(destination, tx.clone())));

    assert!(!transport.delivery_link_available(&destination.address_hash).await);

    handler.lock().await.out_links.insert(destination.address_hash, outbound.clone());
    assert!(
        transport.delivery_link_available(&destination.address_hash).await,
        "pending direct links count as available like Python LXMRouter.direct_links membership"
    );

    outbound.lock().await.close();
    assert!(
        !transport.delivery_link_available(&destination.address_hash).await,
        "closed direct links must not leak availability while awaiting cleanup"
    );

    let inbound_request = Link::new(destination, tx.clone()).request();
    let inbound =
        Link::new_from_request(&inbound_request, signer.sign_key().clone(), destination, tx)
            .expect("link request should parse");
    let inbound = Arc::new(Mutex::new(inbound));
    let inbound_id = *inbound.lock().await.id();
    handler.lock().await.in_links.insert(inbound_id, inbound.clone());

    assert!(
        transport.delivery_link_available(&destination.address_hash).await,
        "backchannel links count as available like Python LXMRouter.backchannel_links membership"
    );

    inbound.lock().await.close();
    assert!(!transport.delivery_link_available(&destination.address_hash).await);
}
