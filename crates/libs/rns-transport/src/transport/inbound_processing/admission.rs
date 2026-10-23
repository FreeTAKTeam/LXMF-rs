use super::*;

/// The frozen filter's order is significant. This checks only membership;
/// callers commit remembering at accepted queue admission or Link processing.
pub(in crate::transport) fn filter_duplicate_packet(
    cache: &PacketCache,
    packet: &Packet,
    wire_hops: u8,
    attached: bool,
    transport_identity: AddressHash,
) -> bool {
    if attached {
        return true;
    }
    if packet.header.packet_type != PacketType::Announce
        && packet.transport.is_some_and(|transport| transport != transport_identity)
    {
        return false;
    }
    if matches!(
        packet.context,
        PacketContext::KeepAlive
            | PacketContext::ResourceRequest
            | PacketContext::ResourceProof
            | PacketContext::Resource
            | PacketContext::CacheRequest
            | PacketContext::Channel
    ) {
        return true;
    }
    if matches!(packet.header.destination_type, DestinationType::Plain | DestinationType::Group) {
        return packet.header.packet_type != PacketType::Announce && wire_hops <= 1;
    }
    !cache.contains_hash(&packet.hash())
        || (packet.header.packet_type == PacketType::Announce
            && packet.header.destination_type == DestinationType::Single)
}

/// Link state can change while a candidate is queued. Defer this whole native
/// packet-shape class rather than carrying a stale routing decision to drain.
pub(super) fn defers_duplicate_remember(packet: &Packet) -> bool {
    packet.header.destination_type == DestinationType::Link
        || packet.context == PacketContext::LinkRequestProof
}

pub(in crate::transport) enum AdmissionFailure {
    Filtered(Box<QueuedInbound>),
    Full(Box<super::super::inbound_queues::InboundQueueFull<QueuedInbound>>),
}

/// Lock order is handler -> cache -> synchronous queue. No callback or I/O
/// occurs under the cache guard, and a failed enqueue changes no cache state.
pub(in crate::transport) async fn try_enqueue_inbound_message(
    handler: &TransportHandler,
    queues: &InboundQueues<QueuedInbound>,
    class: InboundTrafficClass,
    candidate: QueuedInbound,
) -> Result<(), AdmissionFailure> {
    let mut cache = handler.packet_cache.lock().await;
    if !filter_duplicate_packet(
        &cache,
        &candidate.message.packet,
        candidate.wire_hops,
        handler.config.connected_to_shared_instance,
        *handler.config.identity.address_hash(),
    ) {
        return Err(AdmissionFailure::Filtered(Box::new(candidate)));
    }
    let remember = !handler.config.connected_to_shared_instance
        && candidate.path_request.is_none()
        && !defers_duplicate_remember(&candidate.message.packet);
    let hash = candidate.message.packet.hash();
    queues.enqueue(class, candidate).map_err(|full| AdmissionFailure::Full(Box::new(full)))?;
    if remember {
        cache.remember_hash(hash);
    }
    Ok(())
}

pub(in crate::transport) async fn enqueue_inbound_message(
    handler_arc: &Arc<Mutex<TransportHandler>>,
    queues: &InboundQueues<QueuedInbound>,
    class: InboundTrafficClass,
    candidate: QueuedInbound,
) -> bool {
    let outcome = {
        let handler = handler_arc.lock().await;
        try_enqueue_inbound_message(&handler, queues, class, candidate).await
    };
    match outcome {
        Ok(()) => true,
        Err(AdmissionFailure::Filtered(candidate)) => {
            rollback_rejected_inbound(handler_arc, &candidate).await;
            record_filtered_packet(handler_arc, &candidate.message).await;
            false
        }
        Err(AdmissionFailure::Full(full)) => {
            rollback_rejected_inbound(handler_arc, &full.item).await;
            log::warn!(
                "dropping inbound packet because {:?} queue is full iface={} hash={}",
                full.class,
                full.item.message.address,
                full.item.message.packet.hash()
            );
            false
        }
    }
}

pub(super) async fn record_filtered_packet(
    handler_arc: &Arc<Mutex<TransportHandler>>,
    message: &RxMessage,
) {
    let manager = handler_arc.lock().await.iface_manager.clone();
    manager.lock().await.record_packet_filter_hit(message.address);
    log::debug!(
        "dropping filtered packet iface={} dst={} ctx={:?} type={:?}",
        message.address,
        message.packet.destination,
        message.packet.context,
        message.packet.header.packet_type
    );
}
