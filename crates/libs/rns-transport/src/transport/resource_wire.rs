use super::*;
use crate::resource::{build_link_packet, ResourceAdvertisement};

pub(super) fn is_link_resource_packet(packet: &Packet) -> bool {
    packet.header.destination_type == DestinationType::Link
        && matches!(
            packet.context,
            PacketContext::Resource
                | PacketContext::ResourceAdvrtisement
                | PacketContext::ResourceRequest
                | PacketContext::ResourceHashUpdate
                | PacketContext::ResourceProof
                | PacketContext::ResourceInitiatorCancel
                | PacketContext::ResourceReceiverCancel
        )
}

pub(super) fn is_link_resource_proof(packet: &Packet) -> bool {
    packet.context == PacketContext::ResourceProof
        && packet.header.destination_type == DestinationType::Link
}

pub(super) async fn handle_resource_proof(
    packet: Packet,
    handler: Arc<Mutex<TransportHandler>>,
    iface: AddressHash,
) {
    let mut handler = handler.lock().await;
    let link = handler.local_link_for_packet(&packet).await;
    if let Some(link) = link {
        let mut link = link.lock().await;
        // Resource proofs are handled here instead of through
        // `Link::handle_packet`; keep the link watchdog's inbound activity
        // anchor in sync with the wire traffic.
        link.note_inbound(packet.context);
        let mut responses = std::mem::take(&mut handler.resource_response_packets);
        handler.resource_manager.handle_packet_into(&packet, &mut link, &mut responses);
        publish_resource_events_for_link(&mut handler, *link.id(), Some(&mut link));
        drop(link);
        for response in responses.drain(..) {
            handler.send_packet(response).await;
        }
        handler.resource_response_packets = responses;
    } else {
        let forwarded_packet = if handler.config.transport_enabled {
            handler.link_table.handle_link_packet(&packet, iface)
        } else {
            None
        };
        if let Some((packet, target_iface)) = forwarded_packet {
            log::debug!(
                "[tp-diag] resource_proof_forward node={} link={} iface={}",
                handler.config.name,
                packet.destination,
                target_iface
            );
            handler.send(TxMessage { tx_type: TxMessageType::Direct(target_iface), packet }).await;
        }
    }
}

pub(super) async fn handle_link_resource_packet<'a>(
    packet: &Packet,
    iface: AddressHash,
    handler: &mut MutexGuard<'a, TransportHandler>,
) -> bool {
    let link = handler.local_link_for_packet(packet).await;
    let Some(link) = link else {
        log::debug!(
            "[resource-diag] wire_resource_no_link node={} link={} ctx={:02x}",
            handler.config.name,
            packet.destination,
            packet.context as u8
        );
        return false;
    };

    let mut link = link.lock().await;
    // Resource advertisements, requests, hash updates and parts are routed
    // through the ResourceManager rather than `Link::handle_packet`. They
    // are still live link traffic and must prevent the active-link watchdog
    // from closing a long transfer.
    link.note_inbound(packet.context);
    log::debug!(
        "[resource-diag] wire_resource_packet node={} link={} ctx={:02x} has_ingress={}",
        handler.config.name,
        packet.destination,
        packet.context as u8,
        link.ingress_iface().is_some()
    );
    let packet_for_manager = match packet_for_resource_manager(packet, &mut link) {
        Ok(packet) => packet,
        Err(_) => return true,
    };
    if packet.context == PacketContext::ResourceAdvrtisement {
        if let Ok(advertisement) = ResourceAdvertisement::unpack(packet_for_manager.data.as_slice())
        {
            if advertisement.is_request() {
                let limit = handler
                    .single_in_destinations
                    .get(&link.destination().address_hash)
                    .cloned()
                    .map(|destination| async move { destination.lock().await.max_request_size() });
                if let Some(limit) = limit {
                    if let Some(limit) = limit.await {
                        if advertisement.transfer_size > u64::try_from(limit).unwrap_or(u64::MAX) {
                            log::warn!(
                                "[resource-diag] reject_request_advertisement link={} hash={} size={} limit={}",
                                link.id(),
                                advertisement.hash,
                                advertisement.transfer_size,
                                limit
                            );
                            reject_resource_advertisement(handler, link, iface, advertisement.hash)
                                .await;
                            return true;
                        }
                    }
                }
            }
        }
    }
    let response_iface = link.ingress_iface().unwrap_or(iface);
    // The smaller of what this node's interface can carry and what the link
    // actually negotiated. The interface alone is not enough: it describes
    // the first hop, while a resource fragment has to survive the whole
    // path, and the negotiated value is the only number that knows about
    // the far end.
    let interface_mtu = handler
        .iface_manager
        .lock()
        .await
        .mtu(&response_iface)
        .unwrap_or(crate::resource::DEFAULT_RESOURCE_INTERFACE_MTU)
        .min(link.link_mtu());
    let mut responses = std::mem::take(&mut handler.resource_response_packets);
    handler.resource_manager.handle_packet_into_with_mtu(
        &packet_for_manager,
        &mut link,
        &mut responses,
        interface_mtu,
    );
    let cleanup =
        super::requests::poll_request_cleanup(handler, &mut link, std::time::Instant::now());
    if !responses.is_empty() {
        log::debug!(
            "[resource-diag] wire_resource_responses node={} link={} ctx={:02x} responses={} iface={}",
            handler.config.name,
            packet.destination,
            packet.context as u8,
            responses.len(),
            response_iface
        );
    }
    drop(link);
    for message in cleanup {
        handler.send(message).await;
    }
    for response in responses.drain(..) {
        handler
            .send(TxMessage { tx_type: TxMessageType::Direct(response_iface), packet: response })
            .await;
    }
    handler.resource_response_packets = responses;
    true
}

async fn reject_resource_advertisement(
    handler: &mut TransportHandler,
    mut link: MutexGuard<'_, Link>,
    ingress: AddressHash,
    resource: Hash,
) {
    let link_id = *link.id();
    let iface = link.ingress_iface().unwrap_or(ingress);
    // Native terminal cleanup cannot depend on a cancellation frame's crypto or I/O.
    let mut messages =
        super::requests::poll_request_cleanup(handler, &mut link, std::time::Instant::now());
    let reject = build_link_packet(
        &link,
        PacketType::Data,
        PacketContext::ResourceReceiverCancel,
        resource.as_slice(),
    );
    drop(link);
    match reject {
        Ok(packet) => messages.push(TxMessage { tx_type: TxMessageType::Direct(iface), packet }),
        Err(error) => log::warn!(
            "Resource rejection frame failed link={} resource={} error={}",
            link_id,
            resource,
            error
        ),
    }
    for message in messages {
        if handler.send(message).await.sent_ifaces == 0 {
            log::warn!(
                "Resource rejection frame handoff failed link={} resource={}",
                link_id,
                resource
            );
        }
    }
}

fn packet_for_resource_manager(packet: &Packet, link: &mut Link) -> Result<Packet, RnsError> {
    let needs_decrypt = matches!(
        packet.context,
        PacketContext::ResourceAdvrtisement
            | PacketContext::ResourceRequest
            | PacketContext::ResourceHashUpdate
            | PacketContext::ResourceInitiatorCancel
            | PacketContext::ResourceReceiverCancel
    );
    if !needs_decrypt {
        return Ok(packet.clone());
    }

    let mut buffer = PacketDataBuffer::new();
    let plain_len = match link.decrypt(packet.data.as_slice(), buffer.accuire_buf_max()) {
        Ok(plain) => plain.len(),
        Err(err) => {
            log::debug!(
                "[resource-diag] wire_resource_decrypt_failed link={} ctx={:02x} err={:?}",
                packet.destination,
                packet.context as u8,
                err
            );
            log::warn!("failed to decrypt packet: {:?}", err);
            return Err(RnsError::CryptoError);
        }
    };
    buffer.resize(plain_len);
    let mut plain_packet = packet.clone();
    plain_packet.data = buffer;
    Ok(plain_packet)
}

pub(super) async fn publish_resource_events(handler: &mut TransportHandler) {
    while let Some(link_id) = handler.resource_manager.pending_event_link_id() {
        // Cancellation during lookup leaves the events in their existing owner.
        if let Some(link) = handler.local_link_for_id(&link_id).await {
            let mut link = link.lock().await;
            publish_resource_events_for_link(handler, link_id, Some(&mut link));
        } else {
            publish_resource_events_for_link(handler, link_id, None);
        }
    }
}

pub(super) fn publish_resource_events_for_link(
    handler: &mut TransportHandler,
    link_id: AddressHash,
    mut link: Option<&mut Link>,
) {
    link = link.filter(|link| *link.id() == link_id);
    for event in handler.resource_manager.drain_link_events(link_id) {
        if let Some(link) = link.as_deref_mut() {
            if !link.request_resource_event(&event) {
                continue;
            }
        }
        let hash = event.hash;
        if handler.resource_events_tx.send(event).is_err() {
            log::trace!(
                "[resource-diag] event has no active subscribers resource={} link={}",
                hash,
                link_id
            );
        }
    }
}
