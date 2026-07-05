use super::path::handle_link_request;

// Coverage for unicast-route learning on multicast-interface ingress.

/// Build a `LinkRequest` packet targeting `dest_desc` (as a client would send).
fn link_request_to(dest_desc: DestinationDesc) -> Packet {
    let (link_events, _keep) = tokio::sync::broadcast::channel(4);
    let mut outbound = Link::new(dest_desc, link_events);
    outbound.request()
}

/// `ingress_route_iface` on a multicast iface eagerly registers the sender's
/// virtual unicast route from its source addr and returns that virtual hash —
/// but only for a packet we accept (here, a link request to a local input
/// destination).
#[tokio::test]
async fn ingress_route_iface_registers_virtual_route_on_multicast() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &identity, true));
    let mc_iface = register_fake_multicast_iface(&transport).await;
    let handler = transport.get_handler();

    // Local destination the request targets → the request is accepted.
    let server_dest =
        SingleInputDestination::new(PrivateIdentity::new_from_rand(OsRng), DestinationName::new("lxmf", "delivery"));
    let dest_desc = server_dest.desc;
    handler
        .lock()
        .await
        .single_in_destinations
        .insert(dest_desc.address_hash, Arc::new(Mutex::new(server_dest)));
    let request_packet = link_request_to(dest_desc);

    let peer = peer_addr(4242);
    let route_iface = handler
        .lock()
        .await
        .ingress_route_iface(&request_packet, mc_iface, crate::iface::IfaceSource::Udp(peer))
        .await;

    assert_ne!(route_iface, mc_iface, "must resolve to a fresh virtual unicast iface");

    let guard = handler.lock().await;
    let routing = guard.multicast_peer_routings.get(&mc_iface).expect("routing").lock().await;
    assert_eq!(routing.hash_for_addr(&peer), Some(route_iface));
    assert_eq!(routing.addr_for_hash(&route_iface), Some(peer));
}

/// On a non-multicast iface (no PeerRouting) it is a pass-through even for an
/// accepted packet: the caller's iface is returned unchanged, nothing registers.
#[tokio::test]
async fn ingress_route_iface_passes_through_on_non_multicast() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &identity, true));
    let unicast_iface = new_unicast_iface_in(&transport).await;
    let handler = transport.get_handler();

    // Accepted (local destination) so the pass-through is due to the iface not
    // being multicast, not the acceptance gate.
    let server_dest =
        SingleInputDestination::new(PrivateIdentity::new_from_rand(OsRng), DestinationName::new("lxmf", "delivery"));
    let dest_desc = server_dest.desc;
    handler
        .lock()
        .await
        .single_in_destinations
        .insert(dest_desc.address_hash, Arc::new(Mutex::new(server_dest)));
    let request_packet = link_request_to(dest_desc);

    let peer = peer_addr(4242);
    let route_iface = handler
        .lock()
        .await
        .ingress_route_iface(&request_packet, unicast_iface, crate::iface::IfaceSource::Udp(peer))
        .await;

    assert_eq!(route_iface, unicast_iface, "non-multicast iface must pass through unchanged");
    assert!(handler.lock().await.unicast_udp_ifaces.is_empty());
}

/// An inbound `LinkRequest` to an *unknown* destination must not learn a route:
/// on a multicast iface, `handle_link_request` drops it, so registering a
/// virtual unicast iface would only leak `unicast_udp_ifaces` / `iface_manager`
/// entries (until the idle GC) for any sender on the multicast group.
#[tokio::test]
async fn inbound_link_request_to_unknown_destination_does_not_register() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &identity, true));
    let mc_iface = register_fake_multicast_iface(&transport).await;
    let handler = transport.get_handler();

    // A link request targeting a destination we neither host nor have a path to.
    let unknown_dest =
        SingleInputDestination::new(PrivateIdentity::new_from_rand(OsRng), DestinationName::new("lxmf", "delivery"));
    let request_packet = link_request_to(unknown_dest.desc);

    let peer = peer_addr(4242);
    let route_iface = handler
        .lock()
        .await
        .ingress_route_iface(&request_packet, mc_iface, crate::iface::IfaceSource::Udp(peer))
        .await;
    assert_eq!(route_iface, mc_iface, "unknown-destination request must keep the ingress iface");

    handle_link_request(&request_packet, route_iface, handler.lock().await).await;

    let guard = handler.lock().await;
    assert!(guard.unicast_udp_ifaces.is_empty(), "no virtual unicast iface may be registered");
    let routing = guard.multicast_peer_routings.get(&mc_iface).expect("routing").lock().await;
    assert_eq!(routing.hash_for_addr(&peer), None, "no peer route may be learned");
    drop(routing);
    assert!(guard.in_links.is_empty(), "dropped request must not create an in-link");
}

/// End-to-end at the handler level: an inbound `LinkRequest` from a UDP peer on a
/// multicast iface creates an in-link pinned to the peer's virtual unicast iface
/// — so its proof/data replies are unicast instead of being routed at (and
/// dropped by) the multicast iface.
#[tokio::test]
async fn inbound_link_request_pins_in_link_to_unicast_route() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &local_identity, true));
    let mc_iface = register_fake_multicast_iface(&transport).await;
    let handler = transport.get_handler();

    // Register a local service (input) destination the client will link to.
    let server_dest =
        SingleInputDestination::new(PrivateIdentity::new_from_rand(OsRng), DestinationName::new("lxmf", "delivery"));
    let dest_desc = server_dest.desc;
    handler
        .lock()
        .await
        .single_in_destinations
        .insert(dest_desc.address_hash, Arc::new(Mutex::new(server_dest)));

    // Client-side outbound link → take its LinkRequest packet.
    let request_packet = link_request_to(dest_desc);

    // Replicate the jobs.rs inbound dispatch for a LinkRequest arriving on the
    // multicast socket from `peer` before any announce from that peer.
    let peer = peer_addr(4242);
    let route_iface = handler
        .lock()
        .await
        .ingress_route_iface(&request_packet, mc_iface, crate::iface::IfaceSource::Udp(peer))
        .await;
    handle_link_request(&request_packet, route_iface, handler.lock().await).await;

    assert_ne!(route_iface, mc_iface, "link request from a fresh peer resolves to a virtual iface");

    let guard = handler.lock().await;

    // Peer route learned from the link request's source address.
    let routing = guard.multicast_peer_routings.get(&mc_iface).expect("routing").lock().await;
    assert_eq!(routing.hash_for_addr(&peer), Some(route_iface));
    drop(routing);

    // The in-link is pinned to the virtual unicast iface (not the multicast host).
    let in_link = guard.in_links.values().next().expect("in-link should be created").clone();
    drop(guard);
    assert_eq!(
        in_link.lock().await.ingress_iface(),
        Some(route_iface),
        "in-link must bind to the unicast virtual iface so replies are unicast, not dropped",
    );
}

/// Channel frames must bypass the transport-level duplicate filter. The channel
/// protocol has its own sequencing/dedup, and a retransmit is required when the
/// first copy arrives before the receiver's channel is open (a link-activation
/// race exposed once the multicast first-dial no longer forces a slow retry).
/// Deduping the retransmit drops the only copy the open channel would have seen,
/// stalling the auth handshake (`recv round1/round2: auth timeout`).
#[tokio::test]
async fn duplicate_filter_allows_repeated_channel_frames() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &identity, false));
    let handler = transport.get_handler();

    let packet = Packet {
        header: Header {
            destination_type: DestinationType::Link,
            packet_type: PacketType::Data,
            ..Default::default()
        },
        context: PacketContext::Channel,
        destination: AddressHash::new_from_rand(OsRng),
        data: PacketDataBuffer::new_from_slice(b"channel round1 frame"),
        ..Default::default()
    };

    assert!(handler.lock().await.filter_duplicate_packets(&packet).await);
    assert!(
        handler.lock().await.filter_duplicate_packets(&packet).await,
        "channel retransmit must not be suppressed as a transport-level duplicate",
    );
}
