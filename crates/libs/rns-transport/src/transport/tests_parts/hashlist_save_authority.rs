#[tokio::test]
async fn hashlist_save_disabled_or_attached_does_no_disk_io() {
    for (enabled, attached) in [(false, false), (false, true), (true, true)] {
        let temp = tempfile::tempdir().expect("storage directory");
        let mut config = TransportConfig::default();
        config.set_transport_enabled(enabled);
        config.set_connected_to_shared_instance(attached);
        let transport = Transport::new(config);
        // Use an actual owner's writer so the preservation assertion covers the
        // production storage path and representation, including an existing file.
        let existing = temp.path().join("existing");
        std::fs::create_dir(&existing).expect("existing owner storage");
        let disk = ReticulumPacketDiskCache::new(&existing);
        disk.save_packet_hashlist(&existing, &[Hash::new_from_slice(b"owner membership")])
            .await
            .expect("owner writes membership");
        let files = std::fs::read_dir(&existing)
            .expect("owner directory")
            .map(|entry| entry.expect("owner entry").path())
            .collect::<Vec<_>>();
        assert_eq!(files.len(), 1);
        let before = std::fs::read(&files[0]).expect("owner bytes");
        assert_eq!(transport.save_packet_hashlist(&existing).await.expect("inactive save"), 0);
        assert_eq!(
            std::fs::read(&files[0]).expect("owner bytes remain"),
            before,
            "non-owner save must leave the owner's file byte-identical"
        );
        let absent = temp.path().join("absent");
        assert_eq!(transport.save_packet_hashlist(&absent).await.expect("absent save"), 0);
        assert!(!absent.exists(), "non-owner save must not create directories or files");
        transport.cancel.cancel();
    }
}

#[tokio::test]
async fn attached_outbound_and_recursive_dispatch_keep_proof_context_without_membership() {
    for attached in [false, true] {
        for recursive in [false, true] {
            let mut config = TransportConfig::default();
            config.set_connected_to_shared_instance(attached);
            let transport = Transport::new(config);
            let mut channel = transport.iface_manager().lock().await.new_channel(8);
            let packet = Packet {
                destination: AddressHash::new_from_rand(OsRng),
                data: PacketDataBuffer::new_from_slice(b"native outbound proof binding"),
                ..Default::default()
            };
            let hash = packet.hash();
            let handler = transport.get_handler();
            let handler = handler.lock().await;
            let message = crate::iface::TxMessage {
                tx_type: crate::iface::TxMessageType::Broadcast(None),
                packet: packet.clone(),
            };
            let dispatch = if recursive {
                handler.send_recursive_path_request_with_modes(message, None).await
            } else {
                handler.send(message).await
            };
            assert_eq!(dispatch.sent_ifaces, 1);
            assert_eq!(
                channel.tx_channel.recv().await.expect("native carrier handoff").packet.hash(),
                hash
            );
            let cache = handler.packet_cache.lock().await;
            assert_eq!(cache.contains_hash(&hash), !attached);
            assert_eq!(
                cache
                    .proof_context_for_destination(&AddressHash::new_from_hash(&hash))
                    .expect("destination binding retained before carrier handoff")
                    .1,
                packet.destination
            );
            transport.cancel.cancel();
        }
    }
}
