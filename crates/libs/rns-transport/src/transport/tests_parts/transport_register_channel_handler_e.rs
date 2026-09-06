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

    // A peer links to our destination and identifies over it. Python's
    // `LXMRouter.backchannel_links` then holds that link under the PEER's
    // delivery hash, derived from the identified identity — which is what
    // a later send to the peer looks up. Our own destination is what the
    // link was made to, and is never what is being asked about.
    let (inbound, peer_delivery) = identified_inbound_link(&transport, &signer, destination).await;

    assert!(
        transport.delivery_link_available(&peer_delivery).await,
        "an identified peer's inbound link is their backchannel, like Python LXMRouter.backchannel_links"
    );
    assert!(
        !transport.delivery_link_available(&destination.address_hash).await,
        "the link's own destination is ours, not a peer we can reach"
    );

    inbound.lock().await.close();
    assert!(!transport.delivery_link_available(&peer_delivery).await);
}

/// A peer opens a link to `destination` (ours) and identifies over it, and the
/// link is registered as inbound. Returns that link and the peer's own
/// `lxmf.delivery` hash, which is what a later send to them looks up.
async fn identified_inbound_link(
    transport: &Transport,
    signer: &PrivateIdentity,
    destination: crate::destination::DestinationDesc,
) -> (Arc<Mutex<Link>>, AddressHash) {
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let peer = PrivateIdentity::new_from_rand(OsRng);
    let mut peer_side = Link::new(destination, tx.clone());
    let mut inbound =
        Link::new_from_request(&peer_side.request(), signer.sign_key().clone(), destination, tx)
            .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(peer_side.handle_packet(&inbound.prove(), iface), LinkHandleResult::Activated));
    let identify = peer_side
        .identify_packet(&identify_payload_for(&peer, inbound.id()))
        .expect("identify packet");
    inbound.handle_packet(&identify, iface);
    assert!(inbound.identified_peer_identity().is_some(), "the peer identified over the link");
    let peer_delivery = crate::destination::SingleOutputDestination::new(
        *peer.as_identity(),
        DestinationName::new("lxmf", "delivery"),
    )
    .desc
    .address_hash;

    let inbound = Arc::new(Mutex::new(inbound));
    let inbound_id = *inbound.lock().await.id();
    transport.get_handler().lock().await.in_links.insert(inbound_id, inbound.clone());
    (inbound, peer_delivery)
}

/// Knowing a backchannel exists is not enough to send on one. `Transport::link`
/// reads `out_links` alone, so a caller that asks it for the peer's destination
/// gets a fresh outbound link and leaves the usable inbound one untouched —
/// which is a wasted handshake at best, and a timeout against a peer who cannot
/// accept a link at all.
#[tokio::test]
async fn delivery_link_returns_the_backchannel_transport_link_would_miss() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (inbound, peer_delivery) = identified_inbound_link(&transport, &signer, destination).await;

    let reusable = transport.delivery_link(&peer_delivery).await.expect("the peer's backchannel");
    assert!(
        Arc::ptr_eq(&reusable, &inbound),
        "the peer is reachable on the link they opened, so that is the link to send on"
    );
    assert!(
        !transport.get_handler().lock().await.out_links.contains_key(&peer_delivery),
        "the map `Transport::link` consults holds nothing for this peer"
    );

    inbound.lock().await.close();
    assert!(transport.delivery_link(&peer_delivery).await.is_none());
}

/// `public_key ++ verifying_key ++ sign(link_id ++ public_key ++ verifying_key)`,
/// what `RNS.Link.identify` sends.
fn identify_payload_for(identity: &PrivateIdentity, link_id: &AddressHash) -> Vec<u8> {
    let public = identity.as_identity();
    let mut signed = Vec::new();
    signed.extend_from_slice(link_id.as_slice());
    signed.extend_from_slice(public.public_key_bytes());
    signed.extend_from_slice(public.verifying_key_bytes());
    let mut payload = Vec::new();
    payload.extend_from_slice(public.public_key_bytes());
    payload.extend_from_slice(public.verifying_key_bytes());
    payload.extend_from_slice(&identity.sign(&signed).to_bytes());
    payload
}
