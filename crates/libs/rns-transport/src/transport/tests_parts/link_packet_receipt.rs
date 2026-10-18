struct OwnedLinkProofRecording(Arc<StdMutex<Vec<[u8; HASH_SIZE]>>>);
impl ReceiptHandler for OwnedLinkProofRecording {
    fn on_receipt(&self, receipt: &DeliveryReceipt) {
        self.0.lock().expect("receipt recording").push(receipt.message_id);
    }
}

#[tokio::test]
async fn declining_duplicate_owned_packet_preserves_the_original_proof_observer_and_timer() {
    let (transport, link, peer, mut channel, _) = lifecycle_fixture(false).await;
    let packet = link.lock().await.data_packet(b"immutable native packet").expect("packet");
    let mut original = None;
    transport.send_link_packet_on_bound_iface_observed(&link, packet.clone(), |receipt| {
        original = Some(receipt); true
    }).await.expect("original handoff");
    channel.tx_channel.try_recv().expect("one native packet");
    let original = original.expect("original receipt");
    let sent_at = original.sent_at().expect("original timer");
    let mut duplicate = None;
    assert_eq!(transport.send_link_packet_on_bound_iface_observed(&link, packet.clone(), |receipt| {
        duplicate = Some(receipt); false
    }).await.expect("duplicate observation"), None);
    assert!(channel.tx_channel.try_recv().is_err(), "declined duplicate cannot hand off");
    let duplicate = duplicate.expect("duplicate token");
    assert_eq!(duplicate.sent_at(), Some(sent_at), "duplicate cannot rearm its timer");
    handle_proof(peer.prove_packet(&packet), transport.get_handler(), *channel.address()).await;
    assert!(original.is_proved(), "declining reuse cannot erase the accepted native context");
    assert!(duplicate.is_proved(), "all clones see one sticky native result");
}

#[tokio::test]
async fn owned_link_packet_timer_arms_at_handoff_before_blocked_link_bookkeeping() {
    let (transport, link, _, mut channel, _) = lifecycle_fixture(false).await;
    let packet = link.lock().await.data_packet(b"clock boundary").expect("packet");
    let manager = transport.iface_manager();
    let interface_gate = StdMutex::new(None);
    let bookkeeping_gate = StdMutex::new(None);
    let observed = StdMutex::new(None);
    let mut sending =
        Box::pin(transport.send_link_packet_on_bound_iface_observed(&link, packet, |receipt| {
            *interface_gate.lock().expect("interface gate") =
                Some(manager.try_lock().expect("gate I/O"));
            *bookkeeping_gate.lock().expect("Link gate") =
                Some(link.try_lock().expect("Link released before observer"));
            *observed.lock().expect("token") = Some(receipt);
            true
        }));
    std::future::poll_fn(|cx| {
        assert!(sending.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    let receipt = observed.lock().expect("token").as_ref().expect("native registration").clone();
    assert!(receipt.sent_at().is_none(), "blocked interface is not successful handoff");
    drop(interface_gate.lock().expect("interface gate").take());
    std::future::poll_fn(|cx| {
        assert!(sending.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    channel.tx_channel.try_recv().expect("packet has reached interface");
    let sent_at = receipt.sent_at().expect("timer starts before blocked Link bookkeeping");
    drop(bookkeeping_gate.lock().expect("Link gate").take());
    assert_eq!(sending.await.expect("send"), Some(SendPacketOutcome::SentDirect));
    assert_eq!(receipt.sent_at(), Some(sent_at), "completion must not rearm timing");
}

#[tokio::test]
async fn owned_link_packet_proof_survives_timeout_teardown_and_cache_expiry() {
    let (transport, link, peer, mut channel, signer) = lifecycle_fixture(false).await;
    let seen = Arc::new(StdMutex::new(Vec::new()));
    transport.set_receipt_handler(Box::new(OwnedLinkProofRecording(seen.clone()))).await;
    let destination = *link.lock().await.destination();
    let packet = link.lock().await.data_packet(b"original Link payload").expect("packet");
    let proof = peer.prove_packet(&packet);
    let hash = packet.hash();
    let mut observed = None;
    assert_eq!(
        transport
            .send_link_packet_on_bound_iface_observed(&link, packet, |receipt| {
                assert_eq!(receipt.packet_hash(), hash);
                assert!(!receipt.is_proved());
                assert!(receipt.sent_at().is_none(), "handoff has not occurred");
                assert!(channel.tx_channel.try_recv().is_err(), "observe before I/O");
                observed = Some(receipt);
                true
            })
            .await
            .expect("observed packet"),
        Some(SendPacketOutcome::SentDirect)
    );
    channel.tx_channel.try_recv().expect("packet handoff");
    let mut receipt = observed.expect("native token");
    let sent_at = receipt.sent_at().expect("successful handoff arms timer");
    assert!(!receipt.expired(sent_at + receipt.timeout()));
    assert!(receipt.expired(sent_at + receipt.timeout() + Duration::from_nanos(1)));
    transport.teardown_link(&link).await.expect("teardown");
    channel.tx_channel.try_recv().expect("teardown handoff");
    let (mut replacement, _) = activated_link_pair(destination, &signer);
    replacement.set_ingress_iface(*channel.address());
    transport
        .handler
        .lock()
        .await
        .out_links
        .insert(destination.address_hash, Arc::new(Mutex::new(replacement)));
    transport.handler.lock().await.packet_cache.lock().await.release(Duration::ZERO);
    handle_proof(proof, transport.get_handler(), *channel.address()).await;
    receipt.wait_proved().await.expect("real signed proof through native ingress handler");
    assert_eq!(seen.lock().expect("receipts").as_slice(), &[hash.to_bytes()]);
    assert!(!receipt.expired(sent_at + receipt.timeout() + Duration::from_secs(1)));
}

#[tokio::test]
async fn owned_link_packet_context_rejects_other_known_peer_and_wrong_generation() {
    let (transport, link, peer, mut channel, _) = lifecycle_fixture(false).await;
    let seen = Arc::new(StdMutex::new(Vec::new()));
    transport.set_receipt_handler(Box::new(OwnedLinkProofRecording(seen.clone()))).await;
    let packet = link.lock().await.data_packet(b"meaning bound to Link A").expect("packet");
    let mut observed = None;
    transport
        .send_link_packet_on_bound_iface_observed(&link, packet.clone(), |receipt| {
            observed = Some(receipt);
            true
        })
        .await
        .expect("send");
    channel.tx_channel.try_recv().expect("packet");
    let mut receipt = observed.expect("token");
    let (_, other, other_peer, _, _) = lifecycle_fixture(false).await;
    let other_destination = other.lock().await.destination().address_hash;
    let other_id = *other.lock().await.id();
    transport.handler.lock().await.out_links.insert(other_destination, other);
    let forged = other_peer.prove_packet(&packet);
    assert_eq!(forged.destination, other_id);
    handle_proof(forged.clone(), transport.get_handler(), *channel.address()).await;
    let mut wrong_key = forged;
    wrong_key.destination = *link.lock().await.id();
    handle_proof(wrong_key, transport.get_handler(), *channel.address()).await;
    let mut wrong_generation = peer.prove_packet(&packet);
    wrong_generation.destination = other_id;
    handle_proof(wrong_generation, transport.get_handler(), *channel.address()).await;
    assert!(!receipt.is_proved());
    assert!(seen.lock().expect("receipts").is_empty());
    handle_proof(peer.prove_packet(&packet), transport.get_handler(), *channel.address()).await;
    receipt.wait_proved().await.expect("original peer still proves original packet");
    assert_eq!(seen.lock().expect("receipts").as_slice(), &[packet.hash().to_bytes()]);
}

#[tokio::test]
async fn owned_link_packet_failed_or_declined_handoff_has_no_timer() {
    for decline in [false, true] {
        let (transport, link, _, mut channel, _) = lifecycle_fixture(false).await;
        let packet = link.lock().await.data_packet(b"bounded send").expect("packet");
        let mut observed = None;
        if !decline {
            channel.tx_channel.close();
        }
        let result = transport
            .send_link_packet_on_bound_iface_observed(&link, packet, |receipt| {
                observed = Some(receipt);
                !decline
            })
            .await
            .expect("observed handoff");
        assert_eq!(result, if decline { None } else { Some(SendPacketOutcome::DroppedNoRoute) });
        let receipt = observed.expect("observed native registration");
        assert!(receipt.sent_at().is_none());
        assert!(!receipt.expired(std::time::Instant::now() + Duration::from_secs(100)));
        assert!(channel.tx_channel.try_recv().is_err());
        if decline {
            let mut receipt = receipt;
            assert!(receipt.wait_proved().await.is_err(), "rejected registration is retired");
        }
    }
}

#[tokio::test]
async fn owned_link_signal_observes_authenticated_ordinary_data_and_stays_creator_bound() {
    let (_, link, peer, channel, _) = lifecycle_fixture(false).await;
    let mut signal = link.lock().await.observe_msgpack_signal(245).expect("creator observer");
    assert!(link.lock().await.observe_msgpack_signal(245).is_none(), "reuse cannot rebind creator");
    let mut request = peer.data_packet(&hex::decode("91ccf5").expect("hex")).expect("data");
    request.context = PacketContext::Request;
    assert!(matches!(
        link.lock().await.handle_packet(&request, *channel.address()),
        LinkHandleResult::None
    ));
    assert!(!*signal.borrow(), "request envelopes are not ordinary signalling data");
    for bytes in ["92ccf5", "92ccf5c1", "92ccf5a1ff", "91ccf4"] {
        let packet = peer.data_packet(&hex::decode(bytes).expect("hex")).expect("encrypted data");
        assert!(matches!(
            link.lock().await.handle_packet(&packet, *channel.address()),
            LinkHandleResult::Proof(_)
        ));
        assert!(!*signal.borrow(), "malformed/nonmatching {bytes} must leave observer armed");
    }
    let (_, _, wrong_peer, _, _) = lifecycle_fixture(false).await;
    let mut wrong =
        wrong_peer.data_packet(&hex::decode("91ccf5").expect("hex")).expect("other key");
    wrong.destination = *link.lock().await.id();
    assert!(matches!(
        link.lock().await.handle_packet(&wrong, *channel.address()),
        LinkHandleResult::None
    ));
    assert!(!*signal.borrow(), "different Link's ciphertext cannot signal rejection");
    let packet = peer
        .data_packet(&hex::decode("92cd00f5c0").expect("hex"))
        .expect("alternate numeric encoding");
    assert!(matches!(
        link.lock().await.handle_packet(&packet, *channel.address()),
        LinkHandleResult::Proof(_)
    ));
    signal.changed().await.expect("authenticated signal");
    assert!(*signal.borrow());
    link.lock().await.close();
    assert!(*signal.borrow(), "closure preserves a latched result");
    assert!(
        link.lock().await.observe_msgpack_signal(245).is_none(),
        "Closed has no live generation"
    );
    link.lock().await.restart();
    let fresh = link.lock().await.observe_msgpack_signal(245).expect("new generation");
    assert!(!*fresh.borrow());
    link.lock().await.request(); // repeated Pending request preserves its observer
    assert!(fresh.has_changed().is_ok());
}
