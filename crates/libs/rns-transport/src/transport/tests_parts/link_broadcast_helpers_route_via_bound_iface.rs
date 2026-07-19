// Regression coverage for the bug fixed alongside this file: `send_to_*_links`/
// `send_channel_to_*_links` built a correct Link-context packet but dispatched
// it through `TransportHandler::send_packet`, which routes via
// `route_outbound_packet`'s path-table lookup keyed by `packet.destination` —
// for a Link-context packet that's the link's own ephemeral id, never a real
// path-table entry, so for any `Transport` configured with `broadcast: false`
// (the common client case — see `TransportConfig::new`) the packet silently
// resolved to `SendPacketOutcome::DroppedNoRoute` and never reached an
// interface. Confirmed against a downstream consumer's real third-party
// NomadNet node before this fix (see the linked issue).
//
// Each test below builds a `Transport` with a *real* registered interface
// channel and a manually-activated Link (mirroring
// `transport_register_channel_handler_dispatches_inbound_channel_message`'s
// existing shape for constructing an already-Active link without a full
// path-discovery dance), then asserts the helper actually enqueues a
// `TxMessage` on that same interface — not just that the call returns without
// panicking, which the pre-fix code already did.

use crate::iface::{IfaceRole, TxMessageType};

fn activated_link_pair(destination: DestinationDesc, signer: &PrivateIdentity) -> (Link, Link) {
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut outbound = Link::new(destination, tx.clone());
    let request = outbound.request();
    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(
        outbound.handle_packet(&inbound.prove(), iface),
        crate::destination::link::LinkHandleResult::Activated
    ));
    (outbound, inbound)
}

#[tokio::test]
async fn send_to_out_links_reaches_the_links_bound_interface() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &local_identity, false));
    let mut channel = transport.iface_manager().lock().await.new_channel_with_role(8, IfaceRole::Unicast);
    let iface = *channel.address();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination =
        DestinationDesc { identity, address_hash: identity.address_hash, name: DestinationName::new("lxmf", "delivery") };
    let (mut outbound, _inbound) = activated_link_pair(destination, &signer);
    // The Active handshake above used a synthetic iface id, not the real
    // registered `channel` — rebind to the real one so the send below has
    // somewhere concrete to land, matching how a live handshake would set
    // `ingress_iface` from whichever interface the proof actually arrived on.
    outbound.set_ingress_iface(iface);
    let link_id = *outbound.id();
    transport.get_handler().lock().await.out_links.insert(destination.address_hash, Arc::new(Mutex::new(outbound)));

    let report = transport
        .send_to_out_links_with_report(&destination.address_hash, b"broadcast payload")
        .await;

    assert!(report.is_complete());
    assert_eq!(report.matched_links, 1);
    assert_eq!(report.sent_links, 1);
    assert_eq!(report.failed_links, 0);

    let sent = timeout(Duration::from_millis(200), channel.tx_channel.recv())
        .await
        .expect("send_to_out_links must deliver a TxMessage on the link's bound iface — it was silently dropped before this fix")
        .expect("tx channel should not have closed");
    assert_eq!(sent.tx_type, TxMessageType::Direct(iface));
    assert_eq!(sent.packet.destination, link_id, "packet must be addressed to the link itself");
}

#[tokio::test]
async fn send_to_out_links_reports_dispatch_failure_for_unregistered_bound_iface() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &local_identity, false));

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (outbound, _inbound) = activated_link_pair(destination, &signer);
    transport
        .get_handler()
        .lock()
        .await
        .out_links
        .insert(destination.address_hash, Arc::new(Mutex::new(outbound)));

    let report = transport
        .send_to_out_links_with_report(&destination.address_hash, b"undeliverable")
        .await;

    assert!(!report.is_complete());
    assert_eq!(report.matched_links, 1);
    assert_eq!(report.sent_links, 0);
    assert_eq!(report.failed_links, 1);
}

#[tokio::test]
async fn send_channel_to_out_links_reaches_the_links_bound_interface() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &local_identity, false));
    let mut channel = transport.iface_manager().lock().await.new_channel_with_role(8, IfaceRole::Unicast);
    let iface = *channel.address();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination =
        DestinationDesc { identity, address_hash: identity.address_hash, name: DestinationName::new("lxmf", "delivery") };
    let (mut outbound, _inbound) = activated_link_pair(destination, &signer);
    outbound.set_ingress_iface(iface);
    transport.get_handler().lock().await.out_links.insert(destination.address_hash, Arc::new(Mutex::new(outbound)));

    transport.send_channel_to_out_links(&destination.address_hash, b"channel payload").await;

    let sent = timeout(Duration::from_millis(200), channel.tx_channel.recv())
        .await
        .expect("send_channel_to_out_links must deliver a TxMessage on the link's bound iface")
        .expect("tx channel should not have closed");
    assert_eq!(sent.tx_type, TxMessageType::Direct(iface));
    assert_eq!(sent.packet.context, PacketContext::Channel);
}

#[tokio::test]
async fn send_to_in_links_reaches_the_links_bound_interface() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &local_identity, false));
    let mut channel = transport.iface_manager().lock().await.new_channel_with_role(8, IfaceRole::Unicast);
    let iface = *channel.address();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination =
        DestinationDesc { identity, address_hash: identity.address_hash, name: DestinationName::new("lxmf", "delivery") };
    let (_outbound, mut inbound) = activated_link_pair(destination, &signer);
    inbound.set_ingress_iface(iface);
    let link_id = *inbound.id();
    transport.get_handler().lock().await.in_links.insert(link_id, Arc::new(Mutex::new(inbound)));

    transport.send_to_in_links(&destination.address_hash, b"broadcast to clients").await;

    let sent = timeout(Duration::from_millis(200), channel.tx_channel.recv())
        .await
        .expect("send_to_in_links must deliver a TxMessage on the link's bound iface")
        .expect("tx channel should not have closed");
    assert_eq!(sent.tx_type, TxMessageType::Direct(iface));
    assert_eq!(sent.packet.destination, link_id);
}

#[tokio::test]
async fn send_to_all_out_links_skips_inactive_links_and_reaches_active_ones() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &local_identity, false));
    let mut channel = transport.iface_manager().lock().await.new_channel_with_role(8, IfaceRole::Unicast);
    let iface = *channel.address();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination =
        DestinationDesc { identity, address_hash: identity.address_hash, name: DestinationName::new("lxmf", "delivery") };
    let (mut outbound, _inbound) = activated_link_pair(destination, &signer);
    outbound.set_ingress_iface(iface);
    transport.get_handler().lock().await.out_links.insert(destination.address_hash, Arc::new(Mutex::new(outbound)));

    // A second, never-activated (Pending) link must be skipped, not just
    // silently dropped at send time — it should never even build a packet.
    let other_signer = PrivateIdentity::new_from_rand(OsRng);
    let other_identity = *other_signer.as_identity();
    let other_destination = DestinationDesc {
        identity: other_identity,
        address_hash: other_identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let pending = Link::new(other_destination, tx);
    transport.get_handler().lock().await.out_links.insert(other_destination.address_hash, Arc::new(Mutex::new(pending)));

    transport.send_to_all_out_links(b"fan-out payload").await;

    let sent = timeout(Duration::from_millis(200), channel.tx_channel.recv())
        .await
        .expect("send_to_all_out_links must deliver to the active link's bound iface")
        .expect("tx channel should not have closed");
    assert_eq!(sent.tx_type, TxMessageType::Direct(iface));

    // Only the one Active link's packet should have been sent — nothing
    // further should be queued.
    assert!(timeout(Duration::from_millis(50), channel.tx_channel.recv()).await.is_err());
}
