use super::*;

#[tokio::test]
async fn disabled_shared_data_and_proof_roundtrip_preserves_local_boundary_policy() {
    for source_local in [false, true] {
        for destination_local in [false, true] {
            for explicit in [false, true] {
                for wrong_first in [false, true] {
                    let transport = Transport::new(TransportConfig::default());
                    let (
                        mut source_channel,
                        source,
                        mut destination_channel,
                        destination_iface,
                        wrong_channel,
                    ) = {
                        let mut manager = transport.iface_manager.lock().await;
                        let source_channel = manager.new_channel(8);
                        let source_parent = *source_channel.address();
                        let source = if source_local {
                            manager.set_shared_instance(source_parent, true);
                            manager
                                .register_virtual_iface(source_parent, IfaceRole::Unicast)
                                .expect("source child")
                        } else {
                            source_parent
                        };
                        let destination_channel = manager.new_channel(8);
                        let destination_parent = *destination_channel.address();
                        let destination_iface = if destination_local {
                            manager.set_shared_instance(destination_parent, true);
                            manager
                                .register_virtual_iface(destination_parent, IfaceRole::Unicast)
                                .expect("destination child")
                        } else {
                            destination_parent
                        };
                        let wrong_channel = manager.new_channel(8);
                        (
                            source_channel,
                            source,
                            destination_channel,
                            destination_iface,
                            wrong_channel,
                        )
                    };
                    let destination = AddressHash::new_from_slice(&[42; 16]);
                    transport.handler.lock().await.path_table.restore_tunnel_path(
                        destination,
                        destination,
                        if destination_local { 0 } else { 1 },
                        destination_iface,
                        Hash::new_from_slice(b"local-data-route"),
                        std::time::Instant::now(),
                    );
                    let data = Packet {
                        header: Header {
                            destination_type: DestinationType::Single,
                            packet_type: PacketType::Data,
                            ..Header::default()
                        },
                        destination,
                        data: PacketDataBuffer::new_from_slice(b"opaque encrypted data"),
                        ..Packet::default()
                    };
                    handle_data(&data, source, transport.get_handler().lock().await).await;
                    if !source_local && !destination_local {
                        assert!(
                            destination_channel.tx_channel.try_recv().is_err(),
                            "ordinary transit stays disabled"
                        );
                        transport.cancel.cancel();
                        continue;
                    }
                    let forwarded = destination_channel
                        .tx_channel
                        .try_recv()
                        .expect("local-boundary Data must forward");
                    assert_eq!(forwarded.tx_type, TxMessageType::Direct(destination_iface));
                    assert_eq!(forwarded.packet.destination, data.destination);
                    assert_eq!(forwarded.packet.data.as_slice(), data.data.as_slice());
                    assert!(
                        destination_channel.tx_channel.try_recv().is_err(),
                        "exactly one Data frame"
                    );
                    assert!(source_channel.tx_channel.try_recv().is_err(), "no source echo");
                    let mut proof_data = Vec::new();
                    if explicit {
                        proof_data.extend_from_slice(data.hash().as_slice());
                    }
                    proof_data.extend_from_slice(&[0; ed25519_dalek::SIGNATURE_LENGTH]);
                    let proof = Packet {
                        header: Header {
                            packet_type: PacketType::Proof,
                            destination_type: DestinationType::Single,
                            ..Header::default()
                        },
                        destination: AddressHash::new_from_hash(&data.hash()),
                        data: PacketDataBuffer::new_from_slice(&proof_data),
                        ..Packet::default()
                    };
                    if explicit {
                        let mut mismatched = proof.clone();
                        mismatched.destination = AddressHash::new_from_slice(&[99; 16]);
                        handle_proof(mismatched, transport.get_handler(), destination_iface).await;
                        assert!(
                            source_channel.tx_channel.try_recv().is_err(),
                            "embedded packet hash cannot authorize a different proof destination"
                        );
                    }
                    if wrong_first {
                        handle_proof(
                            proof.clone(),
                            transport.get_handler(),
                            *wrong_channel.address(),
                        )
                        .await;
                        assert!(
                            source_channel.tx_channel.try_recv().is_err(),
                            "wrong proof ingress must not use reverse route"
                        );
                    }
                    handle_proof(proof.clone(), transport.get_handler(), destination_iface).await;
                    if wrong_first && source_local {
                        assert!(
                            source_channel.tx_channel.try_recv().is_err(),
                            "eligible wrong ingress consumes route"
                        );
                    } else {
                        let returned =
                            source_channel.tx_channel.try_recv().expect("one reverse proof");
                        assert_eq!(returned.tx_type, TxMessageType::Direct(source));
                        assert_eq!(
                            returned.packet.to_bytes().expect("proof frame"),
                            proof.to_bytes().expect("original proof")
                        );
                    }
                    handle_proof(proof, transport.get_handler(), destination_iface).await;
                    assert!(
                        source_channel.tx_channel.try_recv().is_err(),
                        "reverse route is consumed once"
                    );
                    transport.cancel.cancel();
                }
            }
        }
    }
}

#[tokio::test]
async fn rejected_local_data_dispatch_does_not_create_reverse_proof_authority() {
    let transport = Transport::new(TransportConfig::default());
    let (mut source_channel, source, mut outbound_channel) = {
        let mut manager = transport.iface_manager.lock().await;
        let source_channel = manager.new_channel(8);
        let parent = *source_channel.address();
        assert!(manager.set_shared_instance(parent, true));
        let source =
            manager.register_virtual_iface(parent, IfaceRole::Unicast).expect("source child");
        (source_channel, source, manager.new_channel(1))
    };
    let outbound = *outbound_channel.address();
    outbound_channel.tx_channel.close();
    let data = Packet {
        destination: AddressHash::new_from_slice(&[42; 16]),
        data: PacketDataBuffer::new_from_slice(b"rejected local Data"),
        ..Packet::default()
    };
    transport.handler.lock().await.path_table.restore_tunnel_path(
        data.destination,
        data.destination,
        1,
        outbound,
        Hash::new_from_slice(b"closed-data-route"),
        std::time::Instant::now(),
    );
    handle_data(&data, source, transport.get_handler().lock().await).await;
    assert!(outbound_channel.tx_channel.try_recv().is_err());
    let proof = Packet {
        header: Header { packet_type: PacketType::Proof, ..Header::default() },
        destination: AddressHash::new_from_hash(&data.hash()),
        data: PacketDataBuffer::new_from_slice(&[0; ed25519_dalek::SIGNATURE_LENGTH]),
        ..Packet::default()
    };
    handle_proof(proof, transport.get_handler(), outbound).await;
    assert!(
        source_channel.tx_channel.try_recv().is_err(),
        "failed Data handoff grants no proof routing authority"
    );
    transport.cancel.cancel();
}

#[tokio::test]
async fn enabled_transport_preserves_same_interface_data_routes() {
    let mut config = TransportConfig::default();
    config.set_transport_enabled(true);
    let transport = Transport::new(config);
    let mut channel = transport.iface_manager.lock().await.new_channel(8);
    let iface = *channel.address();
    let data = Packet {
        destination: AddressHash::new_from_slice(&[42; 16]),
        data: PacketDataBuffer::new_from_slice(b"same bus Data"),
        ..Packet::default()
    };
    transport.handler.lock().await.path_table.restore_tunnel_path(
        data.destination,
        data.destination,
        1,
        iface,
        Hash::new_from_slice(b"same-interface-route"),
        std::time::Instant::now(),
    );
    handle_data(&data, iface, transport.get_handler().lock().await).await;
    let sent = channel.tx_channel.try_recv().expect("same-interface route stays eligible");
    assert_eq!(sent.tx_type, TxMessageType::Direct(iface));
    assert_eq!(sent.packet.data.as_slice(), data.data.as_slice());
    assert!(channel.tx_channel.try_recv().is_err());
    transport.cancel.cancel();
}
