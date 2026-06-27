use super::diag;
use super::path::send_to_next_hop;
use super::resource_wire;
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

fn validate_destination_receipt_signature(
    identity: &Identity,
    receipt_hash: &Hash,
    signature_bytes: &[u8],
) -> Result<Hash, RnsError> {
    if signature_bytes.len() < SIGNATURE_LENGTH {
        return Err(RnsError::PacketError);
    }
    let signature = Signature::from_slice(&signature_bytes[..SIGNATURE_LENGTH])
        .map_err(|_| RnsError::CryptoError)?;
    identity.verify(receipt_hash.as_slice(), &signature)?;

    Ok(*receipt_hash)
}

pub(super) async fn validated_receipt_hash(
    packet: &Packet,
    handler: &TransportHandler,
) -> Option<[u8; HASH_SIZE]> {
    if packet.header.packet_type != PacketType::Proof {
        return None;
    }

    if packet.header.destination_type == DestinationType::Link
        && matches!(packet.context, PacketContext::LinkProof | PacketContext::None)
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

    if packet.data.len() == SIGNATURE_LENGTH {
        let proof_context = {
            let packet_cache = handler.packet_cache.lock().await;
            packet_cache.proof_context_for_destination(&packet.destination)
        };
        if let Some((receipt_hash, proved_destination, _)) = proof_context {
            if let Some(destination) =
                handler.single_out_destinations.get(&proved_destination).cloned()
            {
                let destination = destination.lock().await;
                if let Ok(hash) = validate_destination_receipt_signature(
                    &destination.identity,
                    &receipt_hash,
                    packet.data.as_slice(),
                ) {
                    return Some(hash.to_bytes());
                }
            }
            if let Some(destination) =
                handler.single_in_destinations.get(&proved_destination).cloned()
            {
                let destination = destination.lock().await;
                if let Ok(hash) = validate_destination_receipt_signature(
                    destination.identity.as_identity(),
                    &receipt_hash,
                    packet.data.as_slice(),
                ) {
                    return Some(hash.to_bytes());
                }
            }
        }
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
        if diag::enabled() {
            log::debug!(
                "[tp-diag] lrproof_forward_skip node={} reason=no_link_table_entry link={} iface={}",
                handler.config.name,
                packet.destination,
                iface
            );
        }
        return false;
    };
    if expected_iface != iface {
        if diag::enabled() {
            log::debug!(
                "[tp-diag] lrproof_forward_skip node={} reason=wrong_iface link={} expected={} got={}",
                handler.config.name,
                packet.destination,
                expected_iface,
                iface
            );
        }
        return false;
    }

    let Some(destination) = handler.single_out_destinations.get(&original_destination).cloned()
    else {
        if diag::enabled() {
            log::debug!(
                "[tp-diag] lrproof_forward_skip node={} reason=missing_destination_identity link={} dst={}",
                handler.config.name,
                packet.destination,
                original_destination
            );
        }
        return false;
    };
    let destination = destination.lock().await;

    let valid = crate::destination::link::validate_link_request_proof_packet(
        &destination.desc,
        &packet.destination,
        packet,
    )
    .is_ok();
    if diag::enabled() {
        log::debug!(
            "[tp-diag] lrproof_forward_validate node={} link={} dst={} iface={} valid={}",
            handler.config.name,
            packet.destination,
            original_destination,
            iface,
            valid
        );
    }
    valid
}

pub(super) async fn handle_proof(
    packet: Packet,
    handler: Arc<Mutex<TransportHandler>>,
    iface: AddressHash,
) {
    if resource_wire::is_link_resource_proof(&packet) {
        resource_wire::handle_resource_proof(packet, handler, iface).await;
        return;
    }
    log::trace!("[tp] proof dst={} ctx={:02x}", packet.destination, packet.context as u8);
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

    if packet.header.destination_type != DestinationType::Link {
        let source_iface = {
            let packet_cache = handler.packet_cache.lock().await;
            if packet.data.len() == SIGNATURE_LENGTH {
                packet_cache
                    .source_iface_for_proof_destination(&packet.destination)
                    .map(|(_, source_iface)| source_iface)
            } else if packet.data.len() >= HASH_SIZE {
                let mut proof_hash = [0u8; HASH_SIZE];
                proof_hash.copy_from_slice(&packet.data.as_slice()[..HASH_SIZE]);
                packet_cache.source_iface_for_hash(&Hash::new(proof_hash))
            } else {
                None
            }
        };
        if let Some(source_iface) = source_iface {
            if source_iface != iface {
                if diag::enabled() {
                    log::debug!(
                        "[tp-diag] destination_proof_reverse_forward node={} proof_dst={} source_iface={} ingress_iface={}",
                        handler.config.name,
                        packet.destination,
                        source_iface,
                        iface
                    );
                }
                handler
                    .send(TxMessage { tx_type: TxMessageType::Direct(source_iface), packet })
                    .await;
                return;
            }
        }
    }

    let mut rtt_messages = Vec::new();
    for link in handler.out_links.values() {
        let mut link = link.lock().await;
        if let LinkHandleResult::Activated = link.handle_packet(&packet, iface) {
            rtt_messages.push(TxMessage {
                tx_type: TxMessageType::Direct(iface),
                packet: link.create_rtt(),
            });
        }
    }
    for message in rtt_messages {
        let dispatch = handler.send(message).await;
        if dispatch.sent_ifaces == 0 {
            log::warn!(
                "tp({}): failed to dispatch link RTT packet matched={} failed={}",
                handler.config.name,
                dispatch.matched_ifaces,
                dispatch.failed_ifaces
            );
        }
    }

    let maybe_packet = if should_forward_link_request_proof(&packet, &handler, iface).await {
        handler.link_table.handle_proof(&packet)
    } else {
        None
    };

    if let Some((packet, iface)) = maybe_packet {
        if diag::enabled() {
            log::debug!(
                "[tp-diag] lrproof_forward node={} link={} iface={}",
                handler.config.name,
                packet.destination,
                iface
            );
        }
        handler.send(TxMessage { tx_type: TxMessageType::Direct(iface), packet }).await;
    } else if packet.context == PacketContext::LinkRequestProof && diag::enabled() {
        log::debug!(
            "[tp-diag] lrproof_not_forwarded node={} link={} ingress_iface={}",
            handler.config.name,
            packet.destination,
            iface
        );
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
    handler.packet_cache.lock().await.note_source(packet, iface);
    let mut data_handled = false;

    if packet.header.destination_type == DestinationType::Link {
        if resource_wire::is_link_resource_packet(packet)
            && resource_wire::handle_link_resource_packet(packet, iface, &mut handler).await
        {
            return;
        }

        log::trace!(
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

        if let Some((packet, iface)) = handler.link_table.handle_reverse_link_packet(packet, iface)
        {
            if diag::enabled() {
                log::debug!(
                    "[resource-diag] wire_resource_reverse_forward node={} link={} iface={}",
                    handler.config.name,
                    packet.destination,
                    iface
                );
            }
            handler.send(TxMessage { tx_type: TxMessageType::Direct(iface), packet }).await;
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
        let has_local_destination =
            handler.single_in_destinations.contains_key(&packet.destination);
        log::info!(
            "[tp-diag] inbound_single_data node={} dst={} iface={} local_destination={} ctx={:02x} len={}",
            handler.config.name,
            packet.destination,
            iface,
            has_local_destination,
            packet.context as u8,
            packet.data.len(),
        );
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
