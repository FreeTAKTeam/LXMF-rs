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
) -> Result<Option<[u8; HASH_SIZE]>, RnsError> {
    if packet.header.packet_type != PacketType::Proof {
        return Ok(None);
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
            return match link.validate_packet_proof(packet) {
                Ok(hash) => Ok(Some(hash.to_bytes())),
                Err(_) => Err(RnsError::CryptoError),
            };
        }
        return Ok(None);
    }

    if packet.data.len() == SIGNATURE_LENGTH {
        let proof_context = {
            let packet_cache = handler.packet_cache.lock().await;
            packet_cache.proof_context_for_destination(&packet.destination)
        };
        if let Some((receipt_hash, proved_destination, _)) = proof_context {
            let mut destination_checked = false;
            if let Some(destination) =
                handler.single_out_destinations.get(&proved_destination).cloned()
            {
                destination_checked = true;
                let destination = destination.lock().await;
                if let Ok(hash) = validate_destination_receipt_signature(
                    &destination.identity,
                    &receipt_hash,
                    packet.data.as_slice(),
                ) {
                    return Ok(Some(hash.to_bytes()));
                }
            }
            if let Some(destination) =
                handler.single_in_destinations.get(&proved_destination).cloned()
            {
                destination_checked = true;
                let destination = destination.lock().await;
                if let Ok(hash) = validate_destination_receipt_signature(
                    destination.identity.as_identity(),
                    &receipt_hash,
                    packet.data.as_slice(),
                ) {
                    return Ok(Some(hash.to_bytes()));
                }
            }
            if destination_checked {
                return Err(RnsError::CryptoError);
            }
        }
    }

    // Explicit proofs are addressed to the truncated hash of the proved
    // packet. Resolve that reverse mapping through the packet cache, require
    // the embedded hash to match, and verify only against the destination we
    // tracked for that packet. Trying arbitrary known identities here would
    // allow a peer to forge a receipt for a packet addressed to someone else.
    if packet.data.len() == HASH_SIZE + SIGNATURE_LENGTH {
        let proof_context = {
            let packet_cache = handler.packet_cache.lock().await;
            packet_cache.proof_context_for_destination(&packet.destination)
        };
        if let Some((receipt_hash, proved_destination, _)) = proof_context {
            let mut embedded_hash = [0u8; HASH_SIZE];
            embedded_hash.copy_from_slice(&packet.data.as_slice()[..HASH_SIZE]);
            if embedded_hash == receipt_hash.to_bytes() {
                let mut destination_checked = false;
                if let Some(destination) =
                    handler.single_out_destinations.get(&proved_destination).cloned()
                {
                    destination_checked = true;
                    let destination = destination.lock().await;
                    if let Ok(hash) =
                        validate_destination_receipt_proof(&destination.identity, packet)
                    {
                        return Ok(Some(hash.to_bytes()));
                    }
                }
                if let Some(destination) =
                    handler.single_in_destinations.get(&proved_destination).cloned()
                {
                    destination_checked = true;
                    let destination = destination.lock().await;
                    if let Ok(hash) = validate_destination_receipt_proof(
                        destination.identity.as_identity(),
                        packet,
                    ) {
                        return Ok(Some(hash.to_bytes()));
                    }
                }
                if destination_checked {
                    return Err(RnsError::CryptoError);
                }
            }
        }
    }

    Ok(None)
}
