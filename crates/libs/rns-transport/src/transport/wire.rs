use super::path::send_to_next_hop;
use super::resource_wire;
use super::wire_encryption::should_encrypt_packet;
use super::wire_receipt::validated_receipt_hash;
use super::*;
use crate::packet::Header;
use ed25519_dalek::SIGNATURE_LENGTH;

async fn should_forward_link_table_proof(
    packet: &Packet,
    handler: &TransportHandler,
    iface: AddressHash,
) -> bool {
    if !handler.config.transport_enabled {
        log::debug!(
            "[tp-diag] link_proof_forward_skip node={} reason=transport_disabled link={} iface={}",
            handler.config.name,
            packet.destination,
            iface
        );
        return false;
    }

    if packet.context != PacketContext::LinkRequestProof {
        return true;
    }

    let Some((original_destination, expected_iface)) =
        handler.link_table.proof_validation_context(&packet.destination)
    else {
        log::debug!(
            "[tp-diag] lrproof_forward_skip node={} reason=no_link_table_entry link={} iface={}",
            handler.config.name,
            packet.destination,
            iface
        );
        return false;
    };
    if expected_iface != iface {
        log::debug!(
            "[tp-diag] lrproof_forward_skip node={} reason=wrong_iface link={} expected={} got={}",
            handler.config.name,
            packet.destination,
            expected_iface,
            iface
        );
        return false;
    }

    let Some(destination) = handler.single_out_destinations.get(&original_destination).cloned()
    else {
        log::debug!(
            "[tp-diag] lrproof_forward_skip node={} reason=missing_destination_identity link={} dst={}",
            handler.config.name,
            packet.destination,
            original_destination
        );
        return false;
    };
    let destination = destination.lock().await;

    let valid = crate::destination::link::validate_link_request_proof_packet(
        &destination.desc,
        &packet.destination,
        packet,
    )
    .is_ok();
    log::debug!(
        "[tp-diag] lrproof_forward_validate node={} link={} dst={} iface={} valid={}",
        handler.config.name,
        packet.destination,
        original_destination,
        iface,
        valid
    );
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
    let receipt_hash = receipt_hash.unwrap_or_else(|err| {
        log::warn!("[tp] proof crypto validation failed dst={}: {:?}", packet.destination, err);
        None
    });
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
        if let (true, Some(source_iface)) = (handler.config.transport_enabled, source_iface) {
            if source_iface != iface {
                log::debug!(
                    "[tp-diag] destination_proof_reverse_forward node={} proof_dst={} source_iface={} ingress_iface={}",
                    handler.config.name,
                    packet.destination,
                    source_iface,
                    iface
                );
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

    let maybe_packet = if should_forward_link_table_proof(&packet, &handler, iface).await {
        handler.link_table.handle_proof(&packet)
    } else {
        None
    };

    if let Some((packet, iface)) = maybe_packet {
        log::debug!(
            "[tp-diag] lrproof_forward node={} link={} iface={}",
            handler.config.name,
            packet.destination,
            iface
        );
        handler.send(TxMessage { tx_type: TxMessageType::Direct(iface), packet }).await;
    } else if packet.context == PacketContext::LinkRequestProof {
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
        let lookup = if handler.config.transport_enabled {
            handler.link_table.handle_keepalive(packet)
        } else {
            None
        };

        if let Some((propagated, iface)) = lookup {
            handler
                .send(TxMessage { tx_type: TxMessageType::Direct(iface), packet: propagated })
                .await;
        }

        return true;
    }

    false
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

        let reverse_packet = if handler.config.transport_enabled {
            handler.link_table.handle_reverse_link_packet(packet, iface)
        } else {
            None
        };
        if let Some((packet, iface)) = reverse_packet {
            log::debug!(
                "[resource-diag] wire_resource_reverse_forward node={} link={} iface={}",
                handler.config.name,
                packet.destination,
                iface
            );
            handler.send(TxMessage { tx_type: TxMessageType::Direct(iface), packet }).await;
            return;
        }

        if handler.config.transport_enabled {
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
            if handler
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
                .is_err()
            {
                // A broadcast sender has no durable queue. No subscribers is
                // valid when the transport is used without an application
                // event consumer, but it must remain observable while
                // diagnosing an apparently missing inbound delivery.
                log::debug!(
                    "tp({}): inbound data event had no subscribers dst={} context={:?}",
                    handler.config.name,
                    packet.destination,
                    packet.context
                );
            }

            // Generates the automatic delivery proof this branch was
            // missing: it decrypts and forwards a plain `Single`/`Data`
            // packet (e.g. a direct LXMF message) above, but never told
            // the sender it arrived. The receive-side validation this
            // proof round-trips through
            // (`validated_receipt_hash`/`validate_destination_receipt_proof`,
            // same file) already handles it correctly, so this is the one
            // missing half. Wire format matches what that validation
            // function already expects: `packet_hash(32B) ++
            // Ed25519_signature(64B)`, signed by this destination's own
            // private identity over the hash of the packet just received.
            // Replied on the same interface the original packet arrived
            // on — this is correct, not a shortcut: Reticulum's own proof
            // relay is hop-by-hop reversal (each node along a path replies
            // on the interface it received on), not a fresh path-table
            // lookup by the proof's own destination (which here is *this*
            // destination's own hash, not a remote one any path table
            // would have an entry for). See #479.
            //
            // Whether a proof actually gets sent is gated by this
            // destination's own `proof_strategy` (mirrors Python
            // Reticulum's `PROVE_NONE`/`PROVE_APP`/`PROVE_ALL` —
            // see `ProofStrategy`'s doc comment) — a receiver-owned policy
            // decision, not something the crate imposes unconditionally.
            // `context: None` is still checked first regardless of
            // strategy: `Request`/`Response` already have the response
            // itself as their own delivery acknowledgement;
            // `Resource`/`KeepAlive`/`CacheRequest` (the same set
            // `should_encrypt_packet` above already excludes, for the same
            // underlying reason — they're each a sub-protocol with its own
            // semantics, not a plain opportunistic app message) have their
            // own dedicated completion/ack mechanisms elsewhere in this
            // crate. Proving those too would be redundant at best and
            // could plausibly confuse a peer expecting exactly one
            // specific ack shape per context.
            if packet.context == PacketContext::None {
                let packet_hash = packet.hash();
                let signature = {
                    let destination_guard = destination.lock().await;
                    let should_prove = match destination_guard.proof_strategy {
                        ProofStrategy::None => false,
                        ProofStrategy::All => true,
                        ProofStrategy::App => destination_guard
                            .proof_requested_callback
                            .as_ref()
                            .is_some_and(|cb| cb.proof_requested(packet)),
                    };
                    should_prove.then(|| destination_guard.identity.sign(&packet_hash.to_bytes()))
                };
                if let Some(signature) = signature {
                    let mut proof_data = Vec::with_capacity(HASH_SIZE + SIGNATURE_LENGTH);
                    proof_data.extend_from_slice(&packet_hash.to_bytes());
                    proof_data.extend_from_slice(&signature.to_bytes());
                    let proof_packet = Packet {
                        header: Header { packet_type: PacketType::Proof, ..Default::default() },
                        ifac: None,
                        // Real Reticulum always addresses a proof to
                        // `Packet.generate_proof_destination()` — the
                        // truncated hash of the *proved* packet, not the
                        // proving destination's own real address hash —
                        // for both explicit and implicit proof shapes
                        // alike (`RNS/Identity.py::prove`,
                        // `RNS/Packet.py::ProofDestination`). This crate's
                        // own reverse-routing table for proofs
                        // (`PacketCache::note_source`/
                        // `by_proof_destination`) is keyed the same way.
                        // Addressing this to our own real destination hash
                        // instead "worked" for direct connections (Python's
                        // local receipt validation in `Transport.py`
                        // matches by scanning `Transport.receipts`, not by
                        // this field), but silently broke reachability the
                        // moment the proof needed to traverse any
                        // intermediate Transport/relay hop back to the
                        // original sender — that hop's own reverse-routing
                        // table would never have an entry under our real
                        // address hash. Confirmed against
                        // `RNS/Identity.py::prove` and
                        // `RNS/Transport.py`'s inbound-proof handling
                        // directly.
                        destination: AddressHash::new_from_hash(&packet_hash),
                        transport: None,
                        context: PacketContext::None,
                        data: PacketDataBuffer::new_from_slice(&proof_data),
                    };
                    let dispatch = handler
                        .send(TxMessage {
                            tx_type: TxMessageType::Direct(iface),
                            packet: proof_packet,
                        })
                        .await;
                    if dispatch.sent_ifaces == 0 && dispatch.queued_ifaces == 0 {
                        log::warn!(
                            "tp({}): delivery proof dispatch failed dst={} packet_hash={} \
                             iface={} matched={} failed={}",
                            handler.config.name,
                            packet.destination,
                            packet_hash,
                            iface,
                            dispatch.matched_ifaces,
                            dispatch.failed_ifaces
                        );
                    }
                }
            }
        } else if handler.config.transport_enabled {
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
