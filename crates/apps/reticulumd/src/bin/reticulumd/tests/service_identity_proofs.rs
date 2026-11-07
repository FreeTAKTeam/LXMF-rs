use super::*;
use rns_rpc::ServiceIdentityBridge;
use rns_transport::iface::{IfaceSource, InterfaceChannel, RxMessage};
use rns_transport::packet::{Packet, PacketDataBuffer, PacketType};
use tokio::time::timeout;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn imported_created_and_restored_services_defer_delivery_proofs() {
    let (_daemon, bridge) = crate::tests::test_transport_bridge_fixture().await;
    let identity = PrivateIdentity::new_from_rand(rand_core::OsRng);
    let imported = bridge
        .import_service_identity(&identity.to_private_key_bytes(), ServiceIdentitySpec::default())
        .expect("import");
    let created = bridge.create_service_identity(ServiceIdentitySpec::default()).expect("create");
    for record in [&imported, &created] {
        let registered = bridge
            .service_identity_for_destination(&record.delivery_destination)
            .expect("registered destination");
        assert!(registered.destination.lock().await.deferred_proofs());
    }
    let directory = bridge.service_identity_storage_dir();
    let (_, restarted, _, _) =
        crate::tests::test_transport_bridge_fixture_with_peer_at(directory).await;
    assert_eq!(restarted.load_persisted_service_identities().await.expect("restore"), 2);
    for record in [&imported, &created] {
        let restored = restarted
            .service_identity_for_destination(&record.delivery_destination)
            .expect("restored destination");
        assert!(restored.destination.lock().await.deferred_proofs());
    }
}

// Exercise registration, routing, admission and proof dispatch together. A policy
// flag alone cannot show that a service's proof is signed by the addressed key
// rather than the daemon's default key, especially after identity restoration.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn imported_created_and_restored_services_return_bound_delivery_proofs() {
    let directory = tempfile::tempdir().expect("service identity directory");
    let (daemon, bridge, _, _) =
        crate::tests::test_transport_bridge_fixture_with_peer_at(directory.path().to_path_buf())
            .await;
    let imported_identity = PrivateIdentity::new_from_rand(OsRng);
    let imported = bridge
        .import_service_identity(
            &imported_identity.to_private_key_bytes(),
            ServiceIdentitySpec::default(),
        )
        .expect("import service");
    let created = bridge.create_service_identity(ServiceIdentitySpec::default()).expect("create");
    let services: Vec<_> = [&imported, &created]
        .into_iter()
        .map(|record| {
            bridge
                .service_identity_for_destination(&record.delivery_destination)
                .expect("registered service")
        })
        .collect();
    let packets: Vec<_> = services.iter().map(service_packet).collect();
    let mut iface = start_service_intake(&daemon, &bridge).await;

    for (index, (service, packet)) in services.iter().zip(&packets).enumerate() {
        let malformed = encrypted_service_packet(service, b"malformed LXMF");
        inject_service_packet(&iface, &malformed).await;
        timeout(Duration::from_secs(3), async {
            loop {
                if daemon.take_event().is_some_and(|event| {
                    event.event_type == "inbound_dropped"
                        && event.payload["reason"] == "decode_failed"
                }) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("malformed service packet rejected");
        assert!(timeout(Duration::from_millis(100), iface.tx_channel.recv()).await.is_err());

        for _ in 0..2 {
            inject_service_packet(&iface, packet).await;
            validate_service_proof(
                &mut iface,
                packet,
                service,
                &[&bridge.signer, &services[1 - index].identity],
            )
            .await;
            assert_service_messages(&daemon, &services[..=index]);
        }
    }

    bridge.transport.stop_interface(iface.address).await;
    iface = bridge.transport.iface_manager().lock().await.new_channel(32);
    for (index, (service, packet)) in services.iter().zip(&packets).enumerate() {
        inject_service_packet(&iface, packet).await;
        validate_service_proof(
            &mut iface,
            packet,
            service,
            &[&bridge.signer, &services[1 - index].identity],
        )
        .await;
    }
    assert_service_messages(&daemon, &services);
    bridge.transport.stop_interface(iface.address).await;

    // This fixture restores service keys into a fresh in-memory message store.
    // Reopened durable-message deduplication is covered by delivery_proofs.rs.
    let (restored_daemon, restored_bridge, _, _) =
        crate::tests::test_transport_bridge_fixture_with_peer_at(directory.path().to_path_buf())
            .await;
    assert_eq!(restored_bridge.load_persisted_service_identities().await.expect("restore"), 2);
    let mut restored_iface = start_service_intake(&restored_daemon, &restored_bridge).await;
    for (index, (service, packet)) in services.iter().zip(&packets).enumerate() {
        let restored = restored_bridge
            .service_identity_for_destination(&service.delivery_destination)
            .expect("restored service");
        assert_eq!(restored.identity_hash, service.identity_hash);
        for _ in 0..2 {
            inject_service_packet(&restored_iface, packet).await;
            validate_service_proof(
                &mut restored_iface,
                packet,
                &restored,
                &[&restored_bridge.signer, &services[1 - index].identity],
            )
            .await;
            assert_service_messages(&restored_daemon, &services[..=index]);
        }
    }
    restored_bridge.transport.stop_interface(restored_iface.address).await;
}

async fn start_service_intake(
    daemon: &Arc<RpcDaemon>,
    bridge: &TransportBridge,
) -> InterfaceChannel {
    crate::inbound_worker::spawn_inbound_worker(
        daemon.clone(),
        bridge.transport.clone(),
        crate::bootstrap::PropagationControlContext {
            enabled: false,
            local_identity_hash: [0; 16],
            propagation_destination_hash_hex: None,
            control_destination_hash_hex: None,
            delivery_destination: Some(bridge.announce_destination.clone()),
            allowed_control_identities: Vec::new(),
            validated_peer_links: Arc::new(Mutex::new(Default::default())),
            identified_peer_links: Arc::new(Mutex::new(Default::default())),
        },
        None,
    );
    bridge.transport.iface_manager().lock().await.new_channel(32)
}

fn service_packet(service: &RegisteredServiceIdentity) -> Packet {
    let source = PrivateIdentity::new_from_rand(OsRng);
    let source_destination = rns_transport::destination::SingleInputDestination::new(
        rns_transport::identity_bridge::to_transport_private_identity(&source),
        DestinationName::new("lxmf", "delivery"),
    );
    let destination: [u8; 16] = hex::decode(&service.delivery_destination)
        .expect("destination hex")
        .try_into()
        .expect("destination hash");
    let wire = reticulum_daemon::lxmf_bridge::build_wire_message_with_options(
        source_destination.desc.address_hash.as_slice().try_into().expect("source hash"),
        destination,
        "service receipt",
        &service.identity_hash,
        None,
        &source,
        None,
        None,
        None,
    )
    .expect("signed service message");
    encrypted_service_packet(service, &wire[16..])
}

fn encrypted_service_packet(service: &RegisteredServiceIdentity, payload: &[u8]) -> Packet {
    let identity =
        rns_transport::identity_bridge::to_transport_identity(service.identity.as_identity());
    let ciphertext = rns_transport::ratchets::encrypt_for_public_key(
        &identity.public_key,
        identity.address_hash.as_slice(),
        payload,
        OsRng,
    )
    .expect("service packet encryption");
    Packet {
        destination: AddressHash::new(
            hex::decode(&service.delivery_destination).expect("hex").try_into().expect("hash"),
        ),
        data: PacketDataBuffer::new_from_slice(&ciphertext),
        ..Packet::default()
    }
}

async fn inject_service_packet(iface: &InterfaceChannel, packet: &Packet) {
    iface
        .rx_channel
        .send(RxMessage {
            address: iface.address,
            packet: packet.clone(),
            source: IfaceSource::None,
        })
        .await
        .expect("service packet ingress");
}

async fn validate_service_proof(
    iface: &mut InterfaceChannel,
    packet: &Packet,
    service: &RegisteredServiceIdentity,
    other_identities: &[&PrivateIdentity],
) {
    let proof = timeout(Duration::from_secs(3), iface.tx_channel.recv())
        .await
        .expect("accepted service proof timeout")
        .expect("service proof output")
        .packet;
    let hash = packet.hash();
    assert_eq!(proof.header.packet_type, PacketType::Proof);
    assert_eq!(proof.destination, AddressHash::new_from_hash(&hash));
    assert_eq!(proof.data.as_slice().len(), 96);
    assert_eq!(&proof.data.as_slice()[..32], hash.as_slice());
    let signature = ed25519_dalek::Signature::from_slice(&proof.data.as_slice()[32..])
        .expect("proof signature");
    let identity =
        rns_transport::identity_bridge::to_transport_identity(service.identity.as_identity());
    identity.verify(hash.as_slice(), &signature).expect("addressed service validates proof");
    for other in other_identities {
        let other = rns_transport::identity_bridge::to_transport_identity(other.as_identity());
        assert!(
            other.verify(hash.as_slice(), &signature).is_err(),
            "proof must reject other hub keys"
        );
    }
}

fn assert_service_messages(daemon: &RpcDaemon, expected: &[RegisteredServiceIdentity]) {
    let result = daemon
        .handle_rpc(rns_rpc::RpcRequest { id: 1, method: "list_messages".into(), params: None })
        .expect("list service messages")
        .result
        .expect("messages result");
    let messages = result["messages"].as_array().expect("message list");
    assert_eq!(messages.len(), expected.len());
    for service in expected {
        assert!(
            messages.iter().any(|message| {
                message["destination"] == service.delivery_destination
                    && message["content"] == service.identity_hash
            }),
            "admitted message must retain its service destination"
        );
    }
}
