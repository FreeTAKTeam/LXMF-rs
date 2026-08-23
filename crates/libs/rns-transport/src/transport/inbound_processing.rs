use super::announce::{
    admit_announce_before_queue, handle_announce, handle_ingress_limited_announce,
};
use super::jobs::apply_receive_hop_increment;
use super::path::{handle_fixed_destinations, handle_link_request};
use super::wire::{handle_data, handle_proof};
use super::*;

mod ifac_admission;
use ifac_admission::violates_ifac_policy;
mod traffic_class;
use traffic_class::inbound_traffic_class;

async fn filter_duplicate_packet(
    packet_cache: Arc<Mutex<PacketCache>>,
    in_link: Option<Arc<Mutex<Link>>>,
    node_name: &str,
    packet: &Packet,
) -> (bool, bool) {
    let mut allow_duplicate = false;
    match packet.header.packet_type {
        PacketType::Announce => return (true, false),
        PacketType::LinkRequest => allow_duplicate = true,
        PacketType::Data => {
            allow_duplicate = matches!(
                packet.context,
                PacketContext::KeepAlive
                    | PacketContext::LinkClose
                    | PacketContext::ResourceRequest
                    | PacketContext::Channel
            );
        }
        PacketType::Proof => {
            if packet.context == PacketContext::LinkRequestProof {
                if let Some(link) = in_link {
                    if link.lock().await.status().not_yet_active() {
                        allow_duplicate = true;
                    }
                }
            }
        }
    }

    let is_new = packet_cache.lock().await.update(packet);
    if !is_new
        && packet.header.destination_type == DestinationType::Link
        && matches!(
            packet.context,
            PacketContext::Resource
                | PacketContext::ResourceAdvrtisement
                | PacketContext::ResourceRequest
                | PacketContext::ResourceHashUpdate
                | PacketContext::ResourceProof
        )
    {
        log::debug!(
            "[resource-diag] duplicate_drop_candidate node={} link={} ctx={:02x}",
            node_name,
            packet.destination,
            packet.context as u8
        );
    }
    (is_new || allow_duplicate, is_new)
}

pub(super) async fn preprocess_inbound_message(
    handler_arc: &Arc<Mutex<TransportHandler>>,
    iface_messages_tx: &broadcast::Sender<RxMessage>,
    mut message: RxMessage,
) -> Option<(InboundTrafficClass, QueuedInbound)> {
    if iface_messages_tx.send(message.clone()).is_err() {
        log::trace!(
            "[tp-diag] interface message has no active subscribers iface={}",
            message.address
        );
    }

    let received_hops = message.packet.header.hops;
    let (configured_hops_delta, iface_manager, path_request, packet_cache, in_link, node_name) = {
        let handler = handler_arc.lock().await;
        (
            handler.local_hops_delta_for_packet(&message.packet),
            handler.iface_manager.clone(),
            handler.fixed_dest_path_requests,
            handler.packet_cache.clone(),
            handler.in_links.get(&message.packet.destination).cloned(),
            handler.config.name.clone(),
        )
    };
    if violates_ifac_policy(&iface_manager, message.address, message.packet.header.ifac_flag).await
    {
        return None;
    }
    if let Some(delta) = configured_hops_delta {
        if !iface_manager.lock().await.is_shared_instance(&message.address) {
            message.packet.header.hops = message.packet.header.hops.saturating_add(delta);
        }
    }
    apply_receive_hop_increment(&mut message.packet);
    let wire_len = message.packet.serialized_len().unwrap_or_else(|_| message.packet.data.len());
    let is_path_request = message.packet.destination == path_request;
    iface_manager.lock().await.record_inbound_traffic(
        message.address,
        message.packet.header.packet_type,
        is_path_request,
        wire_len,
    );

    if usize::from(message.packet.header.hops) > PATHFINDER_M {
        iface_manager
            .lock()
            .await
            .record_protocol_violation(message.address, "hop count exceeds PATHFINDER_M");
        log::warn!(
            "dropping packet over hop limit iface={} hops={} hash={}",
            message.address,
            message.packet.header.hops,
            message.packet.hash()
        );
        return None;
    }

    let destination_type = message.packet.header.destination_type;
    if message.packet.header.packet_type == PacketType::Announce
        && destination_type != DestinationType::Single
    {
        iface_manager
            .lock()
            .await
            .record_protocol_violation(message.address, "announce destination is not Single");
        log::warn!(
            "dropping announce with invalid destination type iface={} type={destination_type:?} hash={}",
            message.address,
            message.packet.hash()
        );
        return None;
    }

    // Reject transported Plain/Group packets before path-request decoding and admission. Path
    // requests use a Plain destination, and admitting an invalid transported request would leave
    // duplicate-cache and in-flight discovery state behind until its timeout.
    if matches!(destination_type, DestinationType::Plain | DestinationType::Group)
        && received_hops > 1
    {
        iface_manager.lock().await.record_protocol_violation(
            message.address,
            "Plain or Group packet was transported beyond one hop",
        );
        log::warn!(
            "dropping transported {destination_type:?} packet iface={} hops={} hash={}",
            message.address,
            received_hops,
            message.packet.hash()
        );
        return None;
    }

    let mut decoded_path_request = None;
    let mut traffic_class = inbound_traffic_class(&message, path_request);
    if is_path_request {
        let data_len = message.packet.data.len();
        if data_len <= crate::hash::ADDRESS_HASH_SIZE {
            iface_manager
                .lock()
                .await
                .record_protocol_violation(message.address, "tagless path request");
            return None;
        }
        let tag_start = if data_len > crate::hash::ADDRESS_HASH_SIZE * 2 {
            crate::hash::ADDRESS_HASH_SIZE * 2
        } else {
            crate::hash::ADDRESS_HASH_SIZE
        };
        if data_len.saturating_sub(tag_start) > crate::hash::ADDRESS_HASH_SIZE {
            iface_manager
                .lock()
                .await
                .record_protocol_violation(message.address, "excessive path request tag size");
        }
        let request = {
            let mut handler = handler_arc.lock().await;
            handler.path_requests.decode(message.packet.data.as_slice(), message.address)
        }?;
        let discovery_candidate = {
            let mode = iface_manager.lock().await.mode(&message.address);
            let handler = handler_arc.lock().await;
            handler.config.transport_enabled
                && !handler.single_in_destinations.contains_key(&request.destination)
                && handler.path_table.get(&request.destination).is_none()
                && mode.is_some_and(|mode| {
                    mode.discovers_unknown_paths() || mode == crate::iface::InterfaceMode::Boundary
                })
        };
        if discovery_candidate
            && !handler_arc
                .lock()
                .await
                .path_requests
                .register_discovery_before_queue(&request.destination, message.address)
        {
            return None;
        }
        if iface_manager.lock().await.should_ingress_limit_path_request(message.address) {
            traffic_class = InboundTrafficClass::IngressLimited;
        }
        decoded_path_request = Some(request);
    }
    if message.packet.header.packet_type == PacketType::Announce
        && !admit_announce_before_queue(
            &message.packet,
            handler_arc,
            message.address,
            message.source,
        )
        .await
    {
        return None;
    }

    // Path requests use their own exact (destination, tag) replay key, independent of requester
    // and ingress interface. Bypass the global packet hash cache so the path-request handler can
    // apply that RNS 1.5 replay rule before deciding whether to batch or forward the request.
    let (accepted, packet_cache_inserted) = if is_path_request {
        (true, false)
    } else {
        filter_duplicate_packet(packet_cache, in_link, &node_name, &message.packet).await
    };
    if !accepted {
        iface_manager.lock().await.record_packet_filter_hit(message.address);
        log::debug!(
            "dropping duplicate packet: dst={}, ctx={:?}, type={:?}",
            message.packet.destination,
            message.packet.context,
            message.packet.header.packet_type
        );
        return None;
    }

    Some((
        traffic_class,
        QueuedInbound {
            message,
            ingress_limited: false,
            packet_cache_inserted,
            path_request: decoded_path_request,
        },
    ))
}

pub(super) async fn rollback_rejected_inbound(
    handler_arc: &Arc<Mutex<TransportHandler>>,
    queued: &QueuedInbound,
) {
    if let Some(request) = queued.path_request.as_ref() {
        handler_arc.lock().await.path_requests.rollback_admission(request, queued.message.address);
    }
    if queued.packet_cache_inserted {
        let packet_cache = handler_arc.lock().await.packet_cache.clone();
        packet_cache.lock().await.remove(&queued.message.packet.hash());
    }
}

pub(super) async fn process_inbound_message(
    handler_arc: Arc<Mutex<TransportHandler>>,
    queued: QueuedInbound,
) {
    let message = queued.message;
    let path_request = queued.path_request;
    let packet = message.packet;
    let mut handler = handler_arc.lock().await;

    if PACKET_TRACE {
        log::debug!("<< rx({}) = {} {}", message.address, packet, packet.hash());
    }
    log::info!(
        "[tp-diag] inbound_packet node={} iface={} src={:?} dst={} type={:?} dest_type={:?} propagation={:?} ctx={:?} len={} hash={}",
        handler.config.name,
        message.address,
        message.source,
        packet.destination,
        packet.header.packet_type,
        packet.header.destination_type,
        packet.header.propagation_type,
        packet.context,
        packet.data.len(),
        packet.hash()
    );

    if handle_fixed_destinations(&packet, &mut handler, message.address, path_request).await {
        return;
    }
    match packet.header.packet_type {
        PacketType::Announce => {
            if queued.ingress_limited {
                handle_ingress_limited_announce(&packet, handler, message.address, message.source)
                    .await
            } else {
                handle_announce(&packet, handler, message.address, message.source).await
            }
        }
        PacketType::LinkRequest => {
            let route_iface =
                handler.ingress_route_iface(&packet, message.address, message.source).await;
            handle_link_request(&packet, route_iface, handler).await
        }
        PacketType::Proof => {
            let route_iface =
                handler.ingress_route_iface(&packet, message.address, message.source).await;
            drop(handler);
            handle_proof(packet, handler_arc, route_iface).await;
        }
        PacketType::Data => {
            let route_iface =
                handler.ingress_route_iface(&packet, message.address, message.source).await;
            handle_data(&packet, route_iface, handler).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iface::InterfaceMode;
    use rand_core::OsRng;

    fn message(packet_type: PacketType, destination: AddressHash) -> RxMessage {
        RxMessage {
            address: AddressHash::new_from_rand(OsRng),
            packet: Packet {
                header: crate::packet::Header { packet_type, ..Default::default() },
                destination,
                ..Default::default()
            },
            source: IfaceSource::None,
        }
    }

    #[test]
    fn rns_1_5_ingress_classifies_management_traffic_before_queueing() {
        let path_request = AddressHash::new_from_rand(OsRng);
        let ordinary = AddressHash::new_from_rand(OsRng);

        assert_eq!(
            inbound_traffic_class(&message(PacketType::Data, ordinary), path_request),
            InboundTrafficClass::Data
        );
        assert_eq!(
            inbound_traffic_class(&message(PacketType::Announce, ordinary), path_request),
            InboundTrafficClass::Announce
        );
        assert_eq!(
            inbound_traffic_class(&message(PacketType::Data, path_request), path_request),
            InboundTrafficClass::PathRequest
        );
    }

    #[tokio::test]
    async fn rns_1_5_ingress_plain_and_group_hop_filter_uses_wire_hops() {
        let transport = Transport::new(TransportConfig::default());
        let handler = transport.get_handler();
        let iface = *transport.iface_manager().lock().await.new_channel(8).address();

        for destination_type in [DestinationType::Plain, DestinationType::Group] {
            for wire_hops in [0, 1] {
                let mut inbound = message(PacketType::Data, AddressHash::new_from_rand(OsRng));
                inbound.address = iface;
                inbound.packet.header.destination_type = destination_type;
                inbound.packet.header.hops = wire_hops;
                assert!(
                    preprocess_inbound_message(&handler, &transport.iface_messages_tx, inbound)
                        .await
                        .is_some(),
                    "wire hops {wire_hops} must be accepted for {destination_type:?}"
                );
            }

            let mut transported = message(PacketType::Data, AddressHash::new_from_rand(OsRng));
            transported.address = iface;
            transported.packet.header.destination_type = destination_type;
            transported.packet.header.hops = 2;
            assert!(
                preprocess_inbound_message(&handler, &transport.iface_messages_tx, transported,)
                    .await
                    .is_none(),
                "wire hops 2 must be rejected for {destination_type:?}"
            );
        }
    }

    #[tokio::test]
    async fn rns_1_5_ingress_full_queue_does_not_poison_packet_retry() {
        let transport = Transport::new(TransportConfig::default());
        let handler = transport.get_handler();
        let iface = *transport.iface_manager().lock().await.new_channel(8).address();
        let queues = InboundQueues::new(InboundQueueLimits {
            data: 1,
            announce: 1,
            path_request: 1,
            ingress_limited: 1,
        });

        let mut first = message(PacketType::Data, AddressHash::new_from_rand(OsRng));
        first.address = iface;
        let (first_class, first) =
            preprocess_inbound_message(&handler, &transport.iface_messages_tx, first)
                .await
                .expect("first packet");
        queues.enqueue(first_class, first).expect("queue first packet");

        let mut retry = message(PacketType::Data, AddressHash::new_from_rand(OsRng));
        retry.address = iface;
        retry.packet.data = PacketDataBuffer::new_from_slice(b"retry after queue pressure");
        let (retry_class, rejected) =
            preprocess_inbound_message(&handler, &transport.iface_messages_tx, retry.clone())
                .await
                .expect("first retry attempt");
        let full = queues.enqueue(retry_class, rejected).expect_err("data queue must be full");
        rollback_rejected_inbound(&handler, &full.item).await;
        let _ = queues.try_dequeue().expect("free queue capacity");

        assert!(
            preprocess_inbound_message(&handler, &transport.iface_messages_tx, retry)
                .await
                .is_some(),
            "queue-full rejection must not poison the packet hash cache"
        );
    }

    #[tokio::test]
    async fn rns_1_5_ingress_path_request_deduplication_reaches_the_scoped_cache() {
        let local_identity = PrivateIdentity::new_from_rand(OsRng);
        let mut config = TransportConfig::new("test", &local_identity, true);
        config.set_transport_enabled(true);
        let transport = Transport::new(config);
        let handler = transport.get_handler();
        let path_request_destination = handler.lock().await.fixed_dest_path_requests;

        let (iface_a_channel, iface_b_channel) = {
            let manager = transport.iface_manager();
            let mut manager = manager.lock().await;
            (
                manager.new_channel_with_role_and_mode(
                    16,
                    IfaceRole::Unicast,
                    InterfaceMode::AccessPoint,
                ),
                manager.new_channel_with_role_and_mode(
                    16,
                    IfaceRole::Unicast,
                    InterfaceMode::AccessPoint,
                ),
            )
        };
        let iface_a = *iface_a_channel.address();
        let iface_b = *iface_b_channel.address();

        let destination = AddressHash::new_from_rand(OsRng);
        let mut generator = PathRequests::new("", None, 16, 16, 30);

        let first_packet =
            generator.generate(&destination, Some(vec![0x5a; crate::hash::ADDRESS_HASH_SIZE]));
        let queued = preprocess_inbound_message(
            &handler,
            &transport.iface_messages_tx,
            RxMessage { address: iface_a, packet: first_packet, source: IfaceSource::None },
        )
        .await
        .expect("first path request must be queued");
        assert_eq!(queued.0, InboundTrafficClass::PathRequest);
        process_inbound_message(handler.clone(), queued.1).await;

        let second_packet =
            generator.generate(&destination, Some(vec![0x5b; crate::hash::ADDRESS_HASH_SIZE]));
        assert!(
            preprocess_inbound_message(
                &handler,
                &transport.iface_messages_tx,
                RxMessage { address: iface_b, packet: second_packet, source: IfaceSource::None },
            )
            .await
            .is_none(),
            "same-destination request must batch before entering the queue"
        );

        let guard = handler.lock().await;
        assert_eq!(
            guard.iface_manager.lock().await.mode(&iface_a),
            Some(InterfaceMode::AccessPoint)
        );
        drop(guard);

        assert_eq!(
            handler.lock().await.path_requests.discovery_requesters(&destination),
            vec![iface_a, iface_b]
        );
        assert_eq!(generator.generate(&destination, None).destination, path_request_destination);
    }
}
