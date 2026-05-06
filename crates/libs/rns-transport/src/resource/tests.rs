#[cfg(test)]
mod tests {
    use super::*;
    use crate::destination::link::LinkHandleResult;
    use crate::destination::{DestinationDesc, DestinationName};
    use crate::identity::PrivateIdentity;
    use rand_core::OsRng;

    #[test]
    fn resource_sender_rejects_oversized_metadata() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "resource"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(1);
        let link = Link::new(destination, tx);
        let data = vec![0u8; 4];
        let metadata = vec![0u8; METADATA_MAX_SIZE + 1];

        let result = ResourceSender::new(&link, data, Some(metadata));
        assert!(matches!(result, Err(RnsError::InvalidArgument)));
    }

    #[test]
    fn resource_sender_marks_request_and_response_advertisements() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "resource"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(4);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        let request_id = vec![0xAA; ADDRESS_HASH_SIZE];
        let request_sender = ResourceSender::new_with_options(
            &outbound,
            b"request".to_vec(),
            None,
            Some(request_id.clone()),
            false,
        )
        .expect("request sender");
        let request_adv = decrypt_advertisement(&outbound, &request_sender.advertisement_packet());
        assert_eq!(
            request_adv.request_id.as_ref().map(|id| id.as_ref()),
            Some(request_id.as_slice())
        );
        assert!(request_adv.is_request());
        assert!(!request_adv.is_response());

        let response_sender = ResourceSender::new_with_options(
            &outbound,
            b"response".to_vec(),
            None,
            Some(request_id.clone()),
            true,
        )
        .expect("response sender");
        let response_adv =
            decrypt_advertisement(&outbound, &response_sender.advertisement_packet());
        assert_eq!(
            response_adv.request_id.as_ref().map(|id| id.as_ref()),
            Some(request_id.as_slice())
        );
        assert!(response_adv.is_response());
        assert!(!response_adv.is_request());
    }

    #[test]
    fn resource_manager_rejects_split_flag() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "resource"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(1);
        let mut link = Link::new(destination, tx);
        link.request();

        let adv = ResourceAdvertisement {
            transfer_size: 1,
            data_size: 1,
            parts: 1,
            hash: Hash::new_from_slice(&[1, 2, 3, 4]),
            random_hash: [0u8; RANDOM_HASH_SIZE],
            original_hash: Hash::new_from_slice(&[1, 2, 3, 4]),
            segment_index: 1,
            total_segments: 1,
            request_id: None,
            flags: FLAG_SPLIT,
            hashmap: vec![0u8; MAPHASH_LEN],
        };

        let packet =
            resource_packet(PacketContext::ResourceAdvrtisement, &adv.pack().expect("advertisement"), *link.id());

        let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 1);
        let responses = manager.handle_packet(&packet, &mut link);

        assert!(responses.is_empty());
        assert!(manager.incoming.is_empty());
    }

    #[test]
    fn resource_manager_ignores_duplicate_active_advertisement() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "resource"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(1);
        let mut link = Link::new(destination, tx);
        link.request();

        let part = b"hello-resource";
        let random_hash = [7u8; RANDOM_HASH_SIZE];
        let mut hashmap = Vec::with_capacity(MAPHASH_LEN);
        hashmap.extend_from_slice(&map_hash(part, &random_hash));
        let adv = ResourceAdvertisement {
            transfer_size: part.len() as u64,
            data_size: part.len() as u64,
            parts: 1,
            hash: Hash::new_from_slice(&[9u8; 32]),
            random_hash,
            original_hash: Hash::new_from_slice(&[9u8; 32]),
            segment_index: 1,
            total_segments: 1,
            request_id: None,
            flags: 0,
            hashmap,
        };

        let packet =
            resource_packet(PacketContext::ResourceAdvrtisement, &adv.pack().expect("advertisement"), *link.id());

        let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 1);
        let first = manager.handle_packet(&packet, &mut link);
        assert_eq!(first.len(), 1);
        assert_eq!(manager.incoming.len(), 1);
        assert_eq!(
            manager.incoming.get(&adv.hash).expect("receiver").retry_count,
            1
        );

        let second = manager.handle_packet(&packet, &mut link);
        assert!(second.is_empty());
        assert_eq!(manager.incoming.len(), 1);
        assert_eq!(
            manager.incoming.get(&adv.hash).expect("receiver").retry_count,
            1
        );
    }

    #[test]
    fn resource_completion_preserves_request_response_metadata() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "resource"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(1);
        let mut link = Link::new(destination, tx);
        link.request();

        let data = b"file-response";
        let random_hash = [0x66; RANDOM_HASH_SIZE];
        let mut part = Vec::with_capacity(RANDOM_HASH_SIZE + data.len());
        part.extend_from_slice(&random_hash);
        part.extend_from_slice(data);
        let resource_hash = Hash::new_from_slice(&[data.as_slice(), &random_hash].concat());
        let request_id = vec![0x44; ADDRESS_HASH_SIZE];
        let mut hashmap = Vec::with_capacity(MAPHASH_LEN);
        hashmap.extend_from_slice(&map_hash(&part, &random_hash));
        let adv = ResourceAdvertisement {
            transfer_size: part.len() as u64,
            data_size: data.len() as u64,
            parts: 1,
            hash: resource_hash,
            random_hash,
            original_hash: resource_hash,
            segment_index: 1,
            total_segments: 1,
            request_id: Some(ByteBuf::from(request_id.clone())),
            flags: FLAG_RESPONSE,
            hashmap,
        };
        let packet =
            resource_packet(PacketContext::ResourceAdvrtisement, &adv.pack().expect("advertisement"), *link.id());

        let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 1);
        let request_packets = manager.handle_packet(&packet, &mut link);
        assert_eq!(request_packets.len(), 1);
        let part_packet = resource_packet(PacketContext::Resource, &part, *link.id());
        let proof_packets = manager.handle_packet(&part_packet, &mut link);
        assert_eq!(proof_packets.len(), 1);

        let events = manager.drain_events();
        let complete = events
            .into_iter()
            .find_map(|event| match event.kind {
                ResourceEventKind::Complete(complete) => Some(complete),
                _ => None,
            })
            .expect("resource complete event");
        assert_eq!(complete.data, data);
        assert_eq!(complete.request_id.as_deref(), Some(request_id.as_slice()));
        assert!(complete.is_response);
        assert!(!complete.is_request);
    }

    #[test]
    fn resource_manager_removes_failed_receiver_without_followup_request() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "resource"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(1);
        let mut link = Link::new(destination, tx);
        link.request();

        let part = b"not-bzip";
        let random_hash = [5u8; RANDOM_HASH_SIZE];
        let resource_hash = Hash::new_from_slice(&[8u8; 32]);
        let mut hashmap = Vec::with_capacity(MAPHASH_LEN);
        hashmap.extend_from_slice(&map_hash(part, &random_hash));
        let adv = ResourceAdvertisement {
            transfer_size: part.len() as u64,
            data_size: part.len() as u64,
            parts: 1,
            hash: resource_hash,
            random_hash,
            original_hash: resource_hash,
            segment_index: 1,
            total_segments: 1,
            request_id: None,
            flags: FLAG_COMPRESSED,
            hashmap,
        };

        let adv_packet =
            resource_packet(PacketContext::ResourceAdvrtisement, &adv.pack().expect("advertisement"), *link.id());

        let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 1);
        let first = manager.handle_packet(&adv_packet, &mut link);
        assert_eq!(first.len(), 1);
        assert_eq!(manager.incoming.len(), 1);

        let part_packet = resource_packet(PacketContext::Resource, part, *link.id());
        let responses = manager.handle_packet(&part_packet, &mut link);
        assert!(responses.is_empty());
        assert!(manager.incoming.is_empty());
    }

    #[test]
    fn resource_receiver_rejects_unreasonable_advertised_parts() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "resource"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(1);
        let mut link = Link::new(destination, tx);
        link.request();

        let adv = ResourceAdvertisement {
            transfer_size: 1,
            data_size: 1,
            parts: 2,
            hash: Hash::new_from_slice(&[3u8; 32]),
            random_hash: [0u8; RANDOM_HASH_SIZE],
            original_hash: Hash::new_from_slice(&[3u8; 32]),
            segment_index: 1,
            total_segments: 1,
            request_id: None,
            flags: 0,
            hashmap: vec![0u8; MAPHASH_LEN * 2],
        };

        let packet =
            resource_packet(PacketContext::ResourceAdvrtisement, &adv.pack().expect("advertisement"), *link.id());

        let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 1);
        let responses = manager.handle_packet(&packet, &mut link);

        assert!(responses.is_empty());
        assert!(manager.incoming.is_empty());
    }

    #[test]
    fn resource_manager_retries_advertisement_until_budget_exhausted() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "resource"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(1);
        let link = Link::new(destination, tx);

        let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 2);
        let (resource_hash, _) =
            manager.start_send(&link, b"retry me".to_vec(), None).expect("start sender");
        manager.confirm_outbound_dispatch(resource_hash, true);

        let now = Instant::now() + Duration::from_secs(2);
        let first = manager.poll_outgoing(now);
        assert_eq!(first.len(), 1);
        assert!(manager.outgoing.contains_key(&resource_hash));

        let second = manager.poll_outgoing(now + Duration::from_secs(2));
        assert_eq!(second.len(), 1);
        assert!(manager.outgoing.contains_key(&resource_hash));

        let third = manager.poll_outgoing(now + Duration::from_secs(4));
        assert!(third.is_empty());
        assert!(!manager.outgoing.contains_key(&resource_hash));
    }

    #[test]
    fn resource_manager_times_out_transferring_sender_after_retry_budget() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "resource"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(1);
        let mut link = Link::new(destination, tx);
        link.request();

        let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 1);
        let payload = vec![0x42; PACKET_MDU + 32];
        let (resource_hash, _) = manager.start_send(&link, payload, None).expect("start sender");
        manager.confirm_outbound_dispatch(resource_hash, true);

        let first_map_hash = manager
            .outgoing
            .get(&resource_hash)
            .expect("outgoing sender")
            .map_hashes[0];
        let request = ResourceRequest {
            hashmap_exhausted: false,
            last_map_hash: None,
            resource_hash,
            requested_hashes: vec![first_map_hash],
        };
        let request_packet =
            resource_packet(PacketContext::ResourceRequest, &request.encode(), *link.id());
        let responses = manager.handle_packet(&request_packet, &mut link);

        assert_eq!(responses.len(), 1);
        assert_eq!(
            manager.outgoing.get(&resource_hash).expect("sender").status,
            ResourceStatus::Transferring
        );

        let now = Instant::now() + Duration::from_secs(2);
        let first = manager.poll_outgoing(now);
        assert!(first.is_empty());
        assert!(manager.outgoing.contains_key(&resource_hash));

        let second = manager.poll_outgoing(now + Duration::from_secs(2));
        assert!(second.is_empty());
        assert!(!manager.outgoing.contains_key(&resource_hash));
    }

    #[test]
    fn resource_manager_times_out_awaiting_proof_after_retry_budget() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "resource"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(1);
        let mut link = Link::new(destination, tx);
        link.request();

        let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 1);
        let (resource_hash, _) =
            manager.start_send(&link, b"proof please".to_vec(), None).expect("start sender");
        manager.confirm_outbound_dispatch(resource_hash, true);

        let first_map_hash = manager
            .outgoing
            .get(&resource_hash)
            .expect("outgoing sender")
            .map_hashes[0];
        let request = ResourceRequest {
            hashmap_exhausted: false,
            last_map_hash: None,
            resource_hash,
            requested_hashes: vec![first_map_hash],
        };
        let request_packet =
            resource_packet(PacketContext::ResourceRequest, &request.encode(), *link.id());
        let responses = manager.handle_packet(&request_packet, &mut link);

        assert_eq!(responses.len(), 1);
        assert_eq!(
            manager.outgoing.get(&resource_hash).expect("sender").status,
            ResourceStatus::AwaitingProof
        );

        let now = Instant::now() + Duration::from_secs(2);
        let first = manager.poll_outgoing(now);
        assert!(first.is_empty());
        assert!(manager.outgoing.contains_key(&resource_hash));

        let second = manager.poll_outgoing(now + Duration::from_secs(2));
        assert!(second.is_empty());
        assert!(!manager.outgoing.contains_key(&resource_hash));
    }

    #[test]
    fn resource_manager_removes_link_scoped_state_on_link_close() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "resource"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(1);
        let mut link = Link::new(destination, tx);
        link.request();

        let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 2);
        let (resource_hash, _) =
            manager.start_send(&link, b"cleanup".to_vec(), None).expect("start sender");
        manager.confirm_outbound_dispatch(resource_hash, true);

        let adv = ResourceAdvertisement {
            transfer_size: 1,
            data_size: 1,
            parts: 1,
            hash: Hash::new_from_slice(&[0x33; 32]),
            random_hash: [0u8; RANDOM_HASH_SIZE],
            original_hash: Hash::new_from_slice(&[0x33; 32]),
            segment_index: 1,
            total_segments: 1,
            request_id: None,
            flags: 0,
            hashmap: vec![0u8; MAPHASH_LEN],
        };
        let packet =
            resource_packet(PacketContext::ResourceAdvrtisement, &adv.pack().expect("advertisement"), *link.id());
        let _ = manager.handle_packet(&packet, &mut link);

        manager.remove_link_state(*link.id());

        assert!(manager.pending_outgoing.is_empty());
        assert!(manager.outgoing.is_empty());
        assert!(manager.incoming.is_empty());
    }

    fn resource_packet(context: PacketContext, payload: &[u8], destination: AddressHash) -> Packet {
        Packet {
            header: Header {
                destination_type: DestinationType::Link,
                packet_type: PacketType::Data,
                ..Default::default()
            },
            destination,
            context,
            data: PacketDataBuffer::new_from_slice(payload),
            ..Default::default()
        }
    }

    fn decrypt_advertisement(link: &Link, packet: &Packet) -> ResourceAdvertisement {
        let mut buffer = PacketDataBuffer::new();
        let plain_len = {
            let plain =
                link.decrypt(packet.data.as_slice(), buffer.accuire_buf_max()).expect("decrypt adv");
            plain.len()
        };
        buffer.resize(plain_len);
        ResourceAdvertisement::unpack(buffer.as_slice()).expect("advertisement")
    }
}
