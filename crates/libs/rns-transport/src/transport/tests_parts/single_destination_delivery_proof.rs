// meshage fork — tests for the delivery-proof-generation restoration in
// `wire.rs::handle_data`'s `DestinationType::Single` branch, gated by the
// destination's own `ProofStrategy` (mirrors Python Reticulum's
// `PROVE_NONE`/`PROVE_APP`/`PROVE_ALL`). See that file's own "meshage fork"
// comment for the full explanation of what was missing and why. Mirrors
// this crate's own existing test shapes: `packet_proof_correlation.rs`
// (capturing what a `Transport` actually sends via its `iface_channel`) and
// `encrypted_resource_control_packet.rs` (injecting a packet directly via
// `handle_data`/`handle_inbound_for_test` rather than a full simulated wire
// round trip).
//
// Every test below (other than the KeepAlive guard) sends a *real*,
// genuinely encrypted packet via a second, independent sender `Transport` —
// not a synthetic `Packet { data: b"hello", .. }` literal. `handle_data`
// attempts to decrypt any `context: None` packet before ever reaching the
// `ProofStrategy` check, so a non-ciphertext payload fails to decrypt and
// returns early regardless of what `ProofStrategy` says — a synthetic
// packet would make every "no proof" assertion vacuously true, hiding a
// broken `ProofStrategy` check rather than exercising it.

#[tokio::test]
async fn default_proof_strategy_never_generates_a_delivery_proof() {
    // No `set_proof_strategy` call at all — the crate's own out-of-box
    // default (`ProofStrategy::None`, matching Python Reticulum's own
    // default) must never prove, even for a genuinely-decryptable plain
    // opportunistic packet.
    let receiver_identity = PrivateIdentity::new_from_rand(OsRng);
    let receiver_config = TransportConfig::new("receiver", &receiver_identity, true);
    let receiver_transport = Transport::new(receiver_config);
    let mut receiver_iface = receiver_transport.iface_manager().lock().await.new_channel(8);
    let own_destination = receiver_transport
        .add_destination(receiver_identity.clone(), DestinationName::new("lxmf", "delivery"))
        .await;
    let own_hash = own_destination.lock().await.desc.address_hash;

    let sender_identity = PrivateIdentity::new_from_rand(OsRng);
    let sender_config = TransportConfig::new("sender", &sender_identity, true);
    let sender_transport = Transport::new(sender_config);
    let mut sender_iface = sender_transport.iface_manager().lock().await.new_channel(8);
    let receiver_announce = own_destination.lock().await.announce(OsRng, None).expect("valid announce");
    handle_announce(
        &receiver_announce,
        sender_transport.get_handler().lock().await,
        sender_iface.address,
        crate::iface::IfaceSource::None,
    )
    .await;

    let outbound = Packet {
        destination: own_hash,
        data: PacketDataBuffer::new_from_slice(b"hello from the sender"),
        ..Packet::default()
    };
    let trace = sender_transport.send_packet_with_trace(outbound).await;
    assert_eq!(trace.outcome, SendPacketOutcome::SentDirect);
    let sent = timeout(Duration::from_millis(200), sender_iface.tx_channel.recv())
        .await
        .expect("packet should be queued for the sender's own interface")
        .expect("tx channel open");

    let receiver_handler = receiver_transport.get_handler();
    handle_data(&sent.packet, receiver_iface.address, receiver_handler.lock().await).await;

    let outcome = timeout(Duration::from_millis(100), receiver_iface.tx_channel.recv()).await;
    assert!(outcome.is_err(), "default ProofStrategy::None must not generate a delivery proof");
}

#[tokio::test]
async fn proof_strategy_all_generates_a_valid_delivery_proof() {
    // Receiver: the side this patch changes. Registers its own destination
    // via `add_destination`, exactly as this app's own `ReticulumStack::new`
    // does for its real LXMF delivery destination, and explicitly opts
    // into `ProofStrategy::All`.
    let receiver_identity = PrivateIdentity::new_from_rand(OsRng);
    let receiver_config = TransportConfig::new("receiver", &receiver_identity, true);
    let receiver_transport = Transport::new(receiver_config);
    let mut receiver_iface = receiver_transport.iface_manager().lock().await.new_channel(8);
    let own_destination = receiver_transport
        .add_destination(receiver_identity.clone(), DestinationName::new("lxmf", "delivery"))
        .await;
    let own_hash = own_destination.lock().await.desc.address_hash;
    own_destination.lock().await.set_proof_strategy(ProofStrategy::All);

    // Sender: a second, independent `Transport` that has learned the
    // receiver's identity via a real announce (same as any real LXMF peer
    // would) — needed so its own outbound encryption is genuine, not a
    // synthetic/malformed payload the receiver would just fail to decrypt.
    let sender_identity = PrivateIdentity::new_from_rand(OsRng);
    let sender_config = TransportConfig::new("sender", &sender_identity, true);
    let mut sender_transport = Transport::new(sender_config);
    let mut sender_iface = sender_transport.iface_manager().lock().await.new_channel(8);
    let receiver_announce = own_destination.lock().await.announce(OsRng, None).expect("valid announce");
    handle_announce(
        &receiver_announce,
        sender_transport.get_handler().lock().await,
        sender_iface.address,
        crate::iface::IfaceSource::None,
    )
    .await;

    let count = Arc::new(AtomicUsize::new(0));
    sender_transport
        .set_receipt_handler(Box::new(CountingReceiptHandler { count: count.clone() }))
        .await;

    // Sender sends a plain, opportunistic LXMF-shaped packet (Data, Single,
    // context: None) — the crate's own send path handles encryption, same
    // as this app's real `send_lxmf_message` relies on.
    let outbound = Packet {
        destination: own_hash,
        data: PacketDataBuffer::new_from_slice(b"hello from the sender"),
        ..Packet::default()
    };
    let trace = sender_transport.send_packet_with_trace(outbound).await;
    assert_eq!(trace.outcome, SendPacketOutcome::SentDirect);
    let sent = timeout(Duration::from_millis(200), sender_iface.tx_channel.recv())
        .await
        .expect("packet should be queued for the sender's own interface")
        .expect("tx channel open");
    let encrypted_packet_hash = sent.packet.hash();

    // Feed exactly what the sender actually transmitted into the
    // receiver's own inbound handling — this is the restored code path.
    let receiver_handler = receiver_transport.get_handler();
    handle_data(&sent.packet, receiver_iface.address, receiver_handler.lock().await).await;

    let proof_message = timeout(Duration::from_millis(200), receiver_iface.tx_channel.recv())
        .await
        .expect("a delivery proof should be sent back on the same interface")
        .expect("tx channel open");
    assert_eq!(proof_message.packet.header.packet_type, PacketType::Proof);
    // Real Reticulum always addresses a proof to the truncated hash of the
    // packet being proved (`Packet.generate_proof_destination()`), not the
    // proving destination's own real address hash — needed for proofs that
    // traverse a Transport/relay hop back to the sender. See wire.rs's own
    // "meshage fork" comment on this proof_packet construction.
    assert_eq!(proof_message.packet.destination, AddressHash::new_from_hash(&encrypted_packet_hash));
    assert_eq!(proof_message.packet.data.len(), HASH_SIZE + ed25519_dalek::SIGNATURE_LENGTH);
    assert_eq!(&proof_message.packet.data.as_slice()[..HASH_SIZE], encrypted_packet_hash.to_bytes().as_slice());

    // The proof is only actually useful if it independently verifies
    // against the receiver's own public key — assert that directly, not
    // just "some bytes came back."
    let signature_bytes = &proof_message.packet.data.as_slice()[HASH_SIZE..];
    let signature = ed25519_dalek::Signature::from_slice(signature_bytes).expect("valid signature bytes");
    assert!(receiver_identity.as_identity().verify(&encrypted_packet_hash.to_bytes(), &signature).is_ok());

    // Close the loop: feed the proof back into the sender's own inbound
    // handling and confirm its `ReceiptHandler` actually fires — this is
    // the exact mechanism `BroadcastReceiptHandler` (meshage's own code,
    // `rust/src/reticulum/transport.rs`) depends on.
    assert_eq!(count.load(Ordering::SeqCst), 0);
    sender_transport.handle_inbound_for_test(proof_message.packet).await;
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn proof_strategy_app_with_true_callback_generates_a_delivery_proof() {
    let receiver_identity = PrivateIdentity::new_from_rand(OsRng);
    let receiver_config = TransportConfig::new("receiver", &receiver_identity, true);
    let receiver_transport = Transport::new(receiver_config);
    let mut receiver_iface = receiver_transport.iface_manager().lock().await.new_channel(8);
    let own_destination = receiver_transport
        .add_destination(receiver_identity.clone(), DestinationName::new("lxmf", "delivery"))
        .await;
    let own_hash = own_destination.lock().await.desc.address_hash;
    {
        let mut dest = own_destination.lock().await;
        dest.set_proof_strategy(ProofStrategy::App);
        dest.set_proof_requested_callback(Box::new(|_packet: &Packet| true));
    }

    let sender_identity = PrivateIdentity::new_from_rand(OsRng);
    let sender_config = TransportConfig::new("sender", &sender_identity, true);
    let sender_transport = Transport::new(sender_config);
    let mut sender_iface = sender_transport.iface_manager().lock().await.new_channel(8);
    let receiver_announce = own_destination.lock().await.announce(OsRng, None).expect("valid announce");
    handle_announce(
        &receiver_announce,
        sender_transport.get_handler().lock().await,
        sender_iface.address,
        crate::iface::IfaceSource::None,
    )
    .await;

    let outbound = Packet {
        destination: own_hash,
        data: PacketDataBuffer::new_from_slice(b"hello from the sender"),
        ..Packet::default()
    };
    let trace = sender_transport.send_packet_with_trace(outbound).await;
    assert_eq!(trace.outcome, SendPacketOutcome::SentDirect);
    let sent = timeout(Duration::from_millis(200), sender_iface.tx_channel.recv())
        .await
        .expect("packet should be queued for the sender's own interface")
        .expect("tx channel open");

    let receiver_handler = receiver_transport.get_handler();
    handle_data(&sent.packet, receiver_iface.address, receiver_handler.lock().await).await;

    let proof_message = timeout(Duration::from_millis(200), receiver_iface.tx_channel.recv())
        .await
        .expect("a proof-requested callback returning true should generate a delivery proof")
        .expect("tx channel open");
    assert_eq!(proof_message.packet.header.packet_type, PacketType::Proof);
}

#[tokio::test]
async fn proof_strategy_app_with_false_callback_never_generates_a_delivery_proof() {
    let receiver_identity = PrivateIdentity::new_from_rand(OsRng);
    let receiver_config = TransportConfig::new("receiver", &receiver_identity, true);
    let receiver_transport = Transport::new(receiver_config);
    let mut receiver_iface = receiver_transport.iface_manager().lock().await.new_channel(8);
    let own_destination = receiver_transport
        .add_destination(receiver_identity.clone(), DestinationName::new("lxmf", "delivery"))
        .await;
    let own_hash = own_destination.lock().await.desc.address_hash;
    {
        let mut dest = own_destination.lock().await;
        dest.set_proof_strategy(ProofStrategy::App);
        dest.set_proof_requested_callback(Box::new(|_packet: &Packet| false));
    }

    let sender_identity = PrivateIdentity::new_from_rand(OsRng);
    let sender_config = TransportConfig::new("sender", &sender_identity, true);
    let sender_transport = Transport::new(sender_config);
    let mut sender_iface = sender_transport.iface_manager().lock().await.new_channel(8);
    let receiver_announce = own_destination.lock().await.announce(OsRng, None).expect("valid announce");
    handle_announce(
        &receiver_announce,
        sender_transport.get_handler().lock().await,
        sender_iface.address,
        crate::iface::IfaceSource::None,
    )
    .await;

    let outbound = Packet {
        destination: own_hash,
        data: PacketDataBuffer::new_from_slice(b"hello from the sender"),
        ..Packet::default()
    };
    let trace = sender_transport.send_packet_with_trace(outbound).await;
    assert_eq!(trace.outcome, SendPacketOutcome::SentDirect);
    let sent = timeout(Duration::from_millis(200), sender_iface.tx_channel.recv())
        .await
        .expect("packet should be queued for the sender's own interface")
        .expect("tx channel open");

    let receiver_handler = receiver_transport.get_handler();
    handle_data(&sent.packet, receiver_iface.address, receiver_handler.lock().await).await;

    let outcome = timeout(Duration::from_millis(100), receiver_iface.tx_channel.recv()).await;
    assert!(outcome.is_err(), "a proof-requested callback returning false must not generate a delivery proof");
}

#[tokio::test]
async fn non_data_or_non_single_packets_never_generate_a_delivery_proof() {
    // Guards against a too-broad fix: only a plain opportunistic
    // `Data`/`Single` packet (context: None) should ever trigger this —
    // not, say, a `Resource`-context packet already handled by its own
    // dedicated resource-proof machinery elsewhere in this crate. Strategy
    // is deliberately set to `All` (the most permissive setting) so this
    // test proves the *context* gate blocks it, not just an unconfigured
    // strategy. A `KeepAlive`-context packet is one `should_encrypt_packet`
    // already exempts from decryption, so a synthetic (non-ciphertext)
    // payload is fine here, unlike the other tests in this file.
    let receiver_identity = PrivateIdentity::new_from_rand(OsRng);
    let receiver_config = TransportConfig::new("receiver", &receiver_identity, true);
    let receiver_transport = Transport::new(receiver_config);
    let mut receiver_iface = receiver_transport.iface_manager().lock().await.new_channel(8);
    let own_destination = receiver_transport
        .add_destination(receiver_identity.clone(), DestinationName::new("lxmf", "delivery"))
        .await;
    let own_hash = own_destination.lock().await.desc.address_hash;
    own_destination.lock().await.set_proof_strategy(ProofStrategy::All);

    let packet = Packet {
        header: Header { packet_type: PacketType::Data, destination_type: DestinationType::Single, ..Default::default() },
        destination: own_hash,
        context: PacketContext::KeepAlive,
        data: PacketDataBuffer::new_from_slice(&[0xFFu8]),
        ..Default::default()
    };

    let receiver_handler = receiver_transport.get_handler();
    handle_data(&packet, receiver_iface.address, receiver_handler.lock().await).await;

    let outcome = timeout(Duration::from_millis(100), receiver_iface.tx_channel.recv()).await;
    assert!(outcome.is_err(), "a KeepAlive-context packet must not generate a delivery proof, even under ProofStrategy::All");
}
