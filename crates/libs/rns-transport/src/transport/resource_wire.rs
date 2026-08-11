use super::path::send_to_next_hop;
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
    let link = link_for_resource_packet(&handler, &packet).await;
    if let Some(link) = link {
        let mut link = link.lock().await;
        let mut responses = std::mem::take(&mut handler.resource_response_packets);
        handler.resource_manager.handle_packet_into(&packet, &mut link, &mut responses);
        let events = handler.resource_manager.drain_events();
        drop(link);
        for response in responses.drain(..) {
            handler.send_packet(response).await;
        }
        handler.resource_response_packets = responses;
        publish_resource_events(&handler, events);
    } else {
        let reverse_packet = if handler.config.transport_enabled {
            handler.link_table.handle_reverse_link_packet(&packet, iface)
        } else {
            None
        };
        if let Some((packet, target_iface)) = reverse_packet {
            log::debug!(
                "[tp-diag] resource_proof_reverse_forward node={} link={} iface={}",
                handler.config.name,
                packet.destination,
                target_iface
            );
            handler.send(TxMessage { tx_type: TxMessageType::Direct(target_iface), packet }).await;
            return;
        }

        // A Link can carry a Resource in either direction. When the Link
        // initiator is receiving a reverse-direction Resource, its proof
        // arrives on the requester side and must continue toward the original
        // Link destination. The reverse-table branch above intentionally only
        // handles packets arriving from the responder side; mirror normal Link
        // data forwarding for the opposite direction.
        let lookup = handler.link_table.original_destination(&packet.destination);
        if lookup.is_some() {
            let sent = send_to_next_hop(&packet, &handler, lookup).await;
            log::debug!(
                "[tp-diag] resource_proof_forward node={} link={} sent={}",
                handler.config.name,
                packet.destination,
                sent
            );
        }
    }
}

pub(super) async fn handle_link_resource_packet<'a>(
    packet: &Packet,
    iface: AddressHash,
    handler: &mut MutexGuard<'a, TransportHandler>,
) -> bool {
    let link = link_for_resource_packet(handler, packet).await;
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
            if advertisement.is_response() {
                if let Some(request_id) = advertisement.request_id.as_ref() {
                    let response_size =
                        usize::try_from(advertisement.transfer_size).unwrap_or(usize::MAX);
                    if !link.take_response_limit_if_allowed(request_id, response_size) {
                        log::warn!(
                            "[resource-diag] reject_response_advertisement link={} hash={} size={}",
                            link.id(),
                            advertisement.hash,
                            advertisement.transfer_size
                        );
                        if let Ok(reject) = build_link_packet(
                            &link,
                            PacketType::Data,
                            PacketContext::ResourceReceiverCancel,
                            advertisement.hash.as_slice(),
                        ) {
                            let response_iface = link.ingress_iface().unwrap_or(iface);
                            handler
                                .send(TxMessage {
                                    tx_type: TxMessageType::Direct(response_iface),
                                    packet: reject,
                                })
                                .await;
                        }
                        return true;
                    }
                }
            }
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
                            if let Ok(reject) = build_link_packet(
                                &link,
                                PacketType::Data,
                                PacketContext::ResourceReceiverCancel,
                                advertisement.hash.as_slice(),
                            ) {
                                let response_iface = link.ingress_iface().unwrap_or(iface);
                                handler
                                    .send(TxMessage {
                                        tx_type: TxMessageType::Direct(response_iface),
                                        packet: reject,
                                    })
                                    .await;
                            }
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
    let events = handler.resource_manager.drain_events();
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
    for response in responses.drain(..) {
        handler
            .send(TxMessage { tx_type: TxMessageType::Direct(response_iface), packet: response })
            .await;
    }
    handler.resource_response_packets = responses;
    publish_resource_events(handler, events);
    true
}

async fn link_for_resource_packet(
    handler: &TransportHandler,
    packet: &Packet,
) -> Option<Arc<Mutex<Link>>> {
    let mut link = handler
        .in_links
        .get(&packet.destination)
        .cloned()
        .or_else(|| handler.out_links.get(&packet.destination).cloned());
    if link.is_none() {
        for candidate in handler.out_links.values() {
            if *candidate.lock().await.id() == packet.destination {
                link = Some(candidate.clone());
                break;
            }
        }
    }
    link
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

pub(super) fn publish_resource_events(handler: &TransportHandler, events: Vec<ResourceEvent>) {
    for event in events {
        let hash = event.hash;
        let link_id = event.link_id;
        if handler.resource_events_tx.send(event).is_err() {
            log::trace!(
                "[resource-diag] event has no active subscribers resource={} link={}",
                hash,
                link_id
            );
        }
    }
}
