use super::path::send_to_next_hop;
use super::*;
use ed25519_dalek::{Signature, SIGNATURE_LENGTH};

fn validate_destination_receipt_proof(
    identity: &Identity,
    packet: &Packet,
) -> Result<Hash, RnsError> {
    if packet.header.packet_type != PacketType::Proof
        || packet.context == PacketContext::LinkRequestProof
        || packet.data.len() < HASH_SIZE + SIGNATURE_LENGTH
    {
        return Err(RnsError::PacketError);
    }

    let mut hash = [0u8; HASH_SIZE];
    hash.copy_from_slice(&packet.data.as_slice()[..HASH_SIZE]);
    let signature =
        Signature::from_slice(&packet.data.as_slice()[HASH_SIZE..HASH_SIZE + SIGNATURE_LENGTH])
            .map_err(|_| RnsError::CryptoError)?;
    identity.verify(&hash, &signature)?;

    Ok(Hash::new(hash))
}

pub(super) async fn validated_receipt_hash(
    packet: &Packet,
    handler: &TransportHandler,
) -> Option<[u8; HASH_SIZE]> {
    if packet.header.packet_type != PacketType::Proof {
        return None;
    }

    if packet.header.destination_type == DestinationType::Link
        && packet.context == PacketContext::LinkProof
    {
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
        if let Some(link) = link {
            let link = link.lock().await;
            if let Ok(hash) = link.validate_packet_proof(packet) {
                return Some(hash.to_bytes());
            }
        }
        return None;
    }

    if let Some(destination) = handler.single_out_destinations.get(&packet.destination).cloned() {
        let destination = destination.lock().await;
        if let Ok(hash) = validate_destination_receipt_proof(&destination.identity, packet) {
            return Some(hash.to_bytes());
        }
    }
    if let Some(destination) = handler.single_in_destinations.get(&packet.destination).cloned() {
        let destination = destination.lock().await;
        if let Ok(hash) =
            validate_destination_receipt_proof(destination.identity.as_identity(), packet)
        {
            return Some(hash.to_bytes());
        }
    }

    None
}

async fn should_forward_link_request_proof(
    packet: &Packet,
    handler: &TransportHandler,
    iface: AddressHash,
) -> bool {
    if packet.context != PacketContext::LinkRequestProof {
        return true;
    }

    let Some((original_destination, expected_iface)) =
        handler.link_table.proof_validation_context(&packet.destination)
    else {
        return false;
    };
    if expected_iface != iface {
        return false;
    }

    let Some(destination) = handler.single_out_destinations.get(&original_destination).cloned()
    else {
        return false;
    };
    let destination = destination.lock().await;

    crate::destination::link::validate_link_request_proof_packet(
        &destination.desc,
        &packet.destination,
        packet,
    )
    .is_ok()
}

pub(super) async fn handle_proof(
    packet: Packet,
    handler: Arc<Mutex<TransportHandler>>,
    iface: AddressHash,
) {
    if packet.context == PacketContext::ResourceProof
        && packet.header.destination_type == DestinationType::Link
    {
        let mut handler = handler.lock().await;
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
            for event in events {
                let _ = handler.resource_events_tx.send(event);
            }
        }
        return;
    }
    eprintln!("[tp] proof dst={} ctx={:02x}", packet.destination, packet.context as u8);
    let receipt_hash = {
        let handler = handler.lock().await;
        validated_receipt_hash(&packet, &handler).await
    };
    if let Some(receipt_hash) = receipt_hash {
        let receipt = DeliveryReceipt::new(receipt_hash);
        let receipt_handler = {
            let handler = handler.lock().await;
            log::trace!("tp({}): handle proof for {}", handler.config.name, packet.destination);
            handler.receipt_handler.clone()
        };

        if let Some(receipt_handler) = receipt_handler {
            receipt_handler.on_receipt(&receipt);
        }
    }

    let mut handler = handler.lock().await;

    let mut rtt_packets = Vec::new();
    for link in handler.out_links.values() {
        let mut link = link.lock().await;
        if let LinkHandleResult::Activated = link.handle_packet(&packet, iface) {
            rtt_packets.push(link.create_rtt());
        }
    }
    for packet in rtt_packets {
        handler.send_packet(packet).await;
    }

    let maybe_packet = if should_forward_link_request_proof(&packet, &handler, iface).await {
        handler.link_table.handle_proof(&packet)
    } else {
        None
    };

    if let Some((packet, iface)) = maybe_packet {
        handler.send(TxMessage { tx_type: TxMessageType::Direct(iface), packet }).await;
    }
}

pub(super) async fn handle_keepalive_response<'a>(
    packet: &Packet,
    handler: &mut MutexGuard<'a, TransportHandler>,
) -> bool {
    if packet.context == PacketContext::KeepAlive
        && packet.data.as_slice()[0] == KEEP_ALIVE_RESPONSE
    {
        let lookup = handler.link_table.handle_keepalive(packet);

        if let Some((propagated, iface)) = lookup {
            handler
                .send(TxMessage { tx_type: TxMessageType::Direct(iface), packet: propagated })
                .await;
        }

        return true;
    }

    false
}

pub(super) fn should_encrypt_packet(packet: &Packet) -> bool {
    if packet.header.packet_type != PacketType::Data {
        return false;
    }
    if packet.header.destination_type != DestinationType::Single {
        return false;
    }
    !matches!(
        packet.context,
        PacketContext::Resource
            | PacketContext::ResourceAdvrtisement
            | PacketContext::ResourceRequest
            | PacketContext::ResourceHashUpdate
            | PacketContext::ResourceProof
            | PacketContext::ResourceInitiatorCancel
            | PacketContext::ResourceReceiverCancel
            | PacketContext::KeepAlive
            | PacketContext::CacheRequest
    )
}

pub(super) async fn handle_data<'a>(
    packet: &Packet,
    iface: AddressHash,
    mut handler: MutexGuard<'a, TransportHandler>,
) {
    let mut data_handled = false;

    if packet.header.destination_type == DestinationType::Link {
        if matches!(
            packet.context,
            PacketContext::Resource
                | PacketContext::ResourceAdvrtisement
                | PacketContext::ResourceRequest
                | PacketContext::ResourceHashUpdate
                | PacketContext::ResourceProof
                | PacketContext::ResourceInitiatorCancel
                | PacketContext::ResourceReceiverCancel
        ) {
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

            if let Some(link) = link {
                let mut link = link.lock().await;
                let needs_decrypt = matches!(
                    packet.context,
                    PacketContext::ResourceAdvrtisement
                        | PacketContext::ResourceRequest
                        | PacketContext::ResourceHashUpdate
                        | PacketContext::ResourceInitiatorCancel
                        | PacketContext::ResourceReceiverCancel
                );
                let packet_for_manager = if needs_decrypt {
                    let mut buffer = PacketDataBuffer::new();
                    let plain_len =
                        match link.decrypt(packet.data.as_slice(), buffer.accuire_buf_max()) {
                            Ok(plain) => plain.len(),
                            Err(err) => {
                                log::warn!("resource: failed to decrypt packet: {:?}", err);
                                return;
                            }
                        };
                    buffer.resize(plain_len);
                    let mut plain_packet = *packet;
                    plain_packet.data = buffer;
                    plain_packet
                } else {
                    *packet
                };
                let mut responses = std::mem::take(&mut handler.resource_response_packets);
                handler.resource_manager.handle_packet_into(
                    &packet_for_manager,
                    &mut link,
                    &mut responses,
                );
                let events = handler.resource_manager.drain_events();
                drop(link);
                for response in responses.drain(..) {
                    handler.send_packet(response).await;
                }
                handler.resource_response_packets = responses;
                for event in events {
                    let _ = handler.resource_events_tx.send(event);
                }
                return;
            }
        }

        eprintln!(
            "[tp] link_data dst={} ctx={:02x} len={}",
            packet.destination,
            packet.context as u8,
            packet.data.len()
        );
        let mut link_packets = Vec::new();
        if let Some(link) = handler.in_links.get(&packet.destination).cloned() {
            let mut link = link.lock().await;
            let result = link.handle_packet(packet, iface);
            if let LinkHandleResult::KeepAlive = result {
                link_packets.push(link.keep_alive_packet(KEEP_ALIVE_RESPONSE));
            } else if let LinkHandleResult::Proof(proof_packet) = result {
                link_packets.push(proof_packet);
            }
        }

        let mut proof_packets = Vec::new();
        for link in handler.out_links.values() {
            let mut link = link.lock().await;
            let result = link.handle_packet(packet, iface);
            if let LinkHandleResult::Proof(proof_packet) = result {
                proof_packets.push(proof_packet);
            }
            data_handled = true;
        }

        for packet in link_packets {
            handler.send(TxMessage { tx_type: TxMessageType::Direct(iface), packet }).await;
        }
        for packet in proof_packets {
            handler.send(TxMessage { tx_type: TxMessageType::Direct(iface), packet }).await;
        }

        if handle_keepalive_response(packet, &mut handler).await {
            return;
        }

        let lookup = handler.link_table.original_destination(&packet.destination);
        if lookup.is_some() {
            let sent = send_to_next_hop(packet, &handler, lookup).await;

            log::trace!(
                "tp({}): {} packet to remote link {}",
                handler.config.name,
                if sent { "forwarded" } else { "could not forward" },
                packet.destination
            );
        }
    }

    if packet.header.destination_type == DestinationType::Single {
        if let Some(destination) = handler.single_in_destinations.get(&packet.destination).cloned()
        {
            data_handled = true;
            let mut ratchet_used = false;
            let payload = if should_encrypt_packet(packet) {
                let mut destination = destination.lock().await;
                match destination.decrypt_with_ratchets(packet.data.as_slice()) {
                    Ok((plaintext, used)) => {
                        ratchet_used = used;
                        plaintext
                    }
                    Err(err) => {
                        log::warn!(
                            "tp({}): decrypt failed for {}: {:?}",
                            handler.config.name,
                            packet.destination,
                            err
                        );
                        return;
                    }
                }
            } else {
                packet.data.as_slice().to_vec()
            };
            let mut buffer = PacketDataBuffer::new();
            if buffer.write(&payload).is_err() {
                log::warn!(
                    "tp({}): decrypted payload too large for {}",
                    handler.config.name,
                    packet.destination
                );
                return;
            }
            handler
                .received_data_tx
                .send(ReceivedData {
                    destination: packet.destination,
                    data: buffer,
                    payload_mode: ReceivedPayloadMode::DestinationStripped,
                    ratchet_used,
                    context: Some(packet.context),
                    request_id: if matches!(
                        packet.context,
                        PacketContext::Request | PacketContext::Response
                    ) {
                        let hash = packet.hash().to_bytes();
                        let mut request_id = [0u8; 16];
                        request_id.copy_from_slice(&hash[..16]);
                        Some(request_id)
                    } else {
                        None
                    },
                    hops: Some(packet.header.hops),
                    interface: packet.transport.map(|value| value.as_slice().to_vec()),
                })
                .ok();
        } else {
            data_handled = send_to_next_hop(packet, &handler, None).await;
        }
    }

    if data_handled {
        log::trace!(
            "tp({}): handle data request for {} dst={:2x} ctx={:2x}",
            handler.config.name,
            packet.destination,
            packet.header.destination_type as u8,
            packet.context as u8,
        );
    }
}
