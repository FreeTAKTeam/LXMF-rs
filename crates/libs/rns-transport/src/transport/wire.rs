use super::diag;
use super::path::message_to_next_hop;
use super::worker_boundary::{
    SingleDestinationDecryptBatchItem, WorkerBackend, WorkerJob, WorkerJobKind,
};
use super::*;
use crate::destination::link::LinkPacketContext;
use crate::resource::{
    ResourceCompletionSnapshot, ResourcePacketLink, ResourcePayload, ResourceProof,
};
use ed25519_dalek::{Signature, SIGNATURE_LENGTH};

pub(super) const MAX_SINGLE_DESTINATION_DECRYPT_WORKERS: usize = 4;
pub(super) const MAX_RESOURCE_DECRYPT_WORKERS: usize = 4;

static SINGLE_DESTINATION_DECRYPT_PERMITS: std::sync::OnceLock<Arc<tokio::sync::Semaphore>> =
    std::sync::OnceLock::new();
static RESOURCE_DECRYPT_PERMITS: std::sync::OnceLock<Arc<tokio::sync::Semaphore>> =
    std::sync::OnceLock::new();

pub(super) fn single_destination_decrypt_permits() -> Arc<tokio::sync::Semaphore> {
    SINGLE_DESTINATION_DECRYPT_PERMITS
        .get_or_init(|| {
            Arc::new(tokio::sync::Semaphore::new(MAX_SINGLE_DESTINATION_DECRYPT_WORKERS))
        })
        .clone()
}

pub(super) fn resource_decrypt_permits() -> Arc<tokio::sync::Semaphore> {
    RESOURCE_DECRYPT_PERMITS
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(MAX_RESOURCE_DECRYPT_WORKERS)))
        .clone()
}

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

async fn link_for_destination_unlocked(
    handler: Arc<Mutex<TransportHandler>>,
    destination: &AddressHash,
) -> Option<Arc<Mutex<Link>>> {
    let (direct_link, out_candidates) = {
        let handler = handler.lock().await;
        let direct_link = handler
            .in_links
            .get(destination)
            .cloned()
            .or_else(|| handler.out_links.get(destination).cloned());
        let out_candidates = if direct_link.is_none() {
            handler.out_links.values().cloned().collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        (direct_link, out_candidates)
    };

    if direct_link.is_some() {
        return direct_link;
    }

    find_ready_outbound_link_candidate(out_candidates, destination)
}

pub(super) fn find_ready_outbound_link_candidate(
    out_candidates: Vec<Arc<Mutex<Link>>>,
    destination: &AddressHash,
) -> Option<Arc<Mutex<Link>>> {
    for candidate in out_candidates {
        let Ok(link) = candidate.try_lock() else {
            log::debug!("tp: skipping busy outbound link while resolving link destination");
            continue;
        };
        if *link.id() == *destination {
            drop(link);
            return Some(candidate);
        }
    }

    None
}

pub(super) async fn validated_receipt_hash_unlocked(
    handler: Arc<Mutex<TransportHandler>>,
    packet: &Packet,
) -> Option<[u8; HASH_SIZE]> {
    if packet.header.packet_type != PacketType::Proof {
        return None;
    }

    if packet.header.destination_type == DestinationType::Link
        && packet.context == PacketContext::LinkProof
    {
        if let Some(link) = link_for_destination_unlocked(handler, &packet.destination).await {
            match link.try_lock() {
                Ok(link) => {
                    if let Ok(hash) = link.validate_packet_proof(packet) {
                        return Some(hash.to_bytes());
                    }
                }
                Err(_) => {
                    log::debug!("tp: skipping receipt proof validation for busy link");
                }
            }
        }
        return None;
    }

    let (single_out, single_in) = {
        let handler = handler.lock().await;
        (
            handler.single_out_destinations.get(&packet.destination).cloned(),
            handler.single_in_destinations.get(&packet.destination).cloned(),
        )
    };

    if let Some(destination) = single_out {
        match destination.try_lock() {
            Ok(destination) => {
                if let Ok(hash) = validate_destination_receipt_proof(&destination.identity, packet)
                {
                    return Some(hash.to_bytes());
                }
            }
            Err(_) => {
                log::debug!("tp: skipping receipt proof validation for busy output destination");
            }
        }
    }
    if let Some(destination) = single_in {
        match destination.try_lock() {
            Ok(destination) => {
                if let Ok(hash) =
                    validate_destination_receipt_proof(destination.identity.as_identity(), packet)
                {
                    return Some(hash.to_bytes());
                }
            }
            Err(_) => {
                log::debug!("tp: skipping receipt proof validation for busy input destination");
            }
        }
    }

    None
}

async fn should_forward_link_request_proof_unlocked(
    packet: &Packet,
    handler: Arc<Mutex<TransportHandler>>,
    iface: AddressHash,
) -> bool {
    if packet.context != PacketContext::LinkRequestProof {
        return true;
    }

    let (config_name, original_destination, destination) = {
        let handler = handler.lock().await;
        let config_name = handler.config.name.clone();
        let Some((original_destination, expected_iface)) =
            handler.link_table.proof_validation_context(&packet.destination)
        else {
            if diag::enabled() {
                log::info!(
                    "[tp-diag] lrproof_forward_skip node={} reason=no_link_table_entry link={} iface={}",
                    config_name,
                    packet.destination,
                    iface
                );
            }
            return false;
        };
        if expected_iface != iface {
            if diag::enabled() {
                log::info!(
                    "[tp-diag] lrproof_forward_skip node={} reason=wrong_iface link={} expected={} got={}",
                    config_name,
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
                log::info!(
                    "[tp-diag] lrproof_forward_skip node={} reason=missing_destination_identity link={} dst={}",
                    config_name,
                    packet.destination,
                    original_destination
                );
            }
            return false;
        };

        (config_name, original_destination, destination)
    };

    let destination_desc = match destination.try_lock() {
        Ok(destination) => destination.desc,
        Err(_) => {
            log::debug!("tp: skipping link-request-proof forwarding while destination is busy");
            return false;
        }
    };

    let valid = crate::destination::link::validate_link_request_proof_packet(
        &destination_desc,
        &packet.destination,
        packet,
    )
    .is_ok();
    if diag::enabled() {
        log::debug!(
            "[tp-diag] lrproof_forward_validate node={} link={} dst={} iface={} valid={}",
            config_name,
            packet.destination,
            original_destination,
            iface,
            valid
        );
    }
    valid
}

pub(super) fn collect_ready_link_activation_rtts(
    packet: &Packet,
    iface: AddressHash,
    out_links: Vec<Arc<Mutex<Link>>>,
) -> Vec<TxMessage> {
    let mut rtt_messages = Vec::new();
    for link in out_links {
        let Ok(mut link) = link.try_lock() else {
            log::debug!("tp: skipping proof handling for busy outbound link");
            continue;
        };
        if let LinkHandleResult::Activated = link.handle_packet(packet, iface) {
            rtt_messages.push(TxMessage {
                tx_type: TxMessageType::Direct(iface),
                packet: link.create_rtt(),
            });
        }
    }
    rtt_messages
}

pub(super) fn collect_ready_outbound_link_proofs(
    packet: &Packet,
    iface: AddressHash,
    out_links: Vec<Arc<Mutex<Link>>>,
) -> Vec<Packet> {
    let mut proof_packets = Vec::new();
    for link in out_links {
        let Ok(mut link) = link.try_lock() else {
            log::debug!("tp: skipping data handling for busy outbound link");
            continue;
        };
        if let LinkHandleResult::Proof(proof_packet) = link.handle_packet(packet, iface) {
            proof_packets.push(proof_packet);
        }
    }
    proof_packets
}

pub(super) async fn handle_proof(
    packet: Packet,
    handler: Arc<Mutex<TransportHandler>>,
    iface: AddressHash,
) {
    if packet.context == PacketContext::ResourceProof
        && packet.header.destination_type == DestinationType::Link
    {
        let link = link_for_destination_unlocked(handler.clone(), &packet.destination).await;
        let (responses, events, events_tx) = if let Some(link) = link {
            let (resource_lane, events_tx) = {
                let handler = handler.lock().await;
                (handler.resource_lane.clone(), handler.resource_events_tx.clone())
            };
            let result = resource_lane.handle_link_packet(packet, link).await;
            (result.responses, result.events, Some(events_tx))
        } else {
            (Vec::new(), Vec::new(), None)
        };
        for response in responses {
            let _ =
                TransportHandler::send_packet_with_trace_unlocked(handler.clone(), response).await;
        }
        if let Some(events_tx) = events_tx {
            for event in events {
                let _ = events_tx.send(event);
            }
        }
        return;
    }
    log::trace!("[tp] proof dst={} ctx={:02x}", packet.destination, packet.context as u8);
    let receipt_hash = validated_receipt_hash_unlocked(handler.clone(), &packet).await;
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

    let (config_name, rtt_messages, maybe_packet) = {
        let (config_name, out_links) = {
            let handler = handler.lock().await;
            (handler.config.name.clone(), handler.out_links.values().cloned().collect::<Vec<_>>())
        };
        let rtt_messages = collect_ready_link_activation_rtts(&packet, iface, out_links);

        let maybe_packet =
            if should_forward_link_request_proof_unlocked(&packet, handler.clone(), iface).await {
                let mut handler = handler.lock().await;
                handler.link_table.handle_proof(&packet)
            } else {
                None
            };

        (config_name, rtt_messages, maybe_packet)
    };

    for message in rtt_messages {
        let dispatch = TransportHandler::send_message_unlocked(handler.clone(), message).await;
        if dispatch.sent_ifaces == 0 {
            log::warn!(
                "tp({}): failed to dispatch link RTT packet matched={} failed={}",
                config_name,
                dispatch.matched_ifaces,
                dispatch.failed_ifaces
            );
        }
    }

    if let Some((packet, iface)) = maybe_packet {
        if diag::enabled() {
            log::debug!(
                "[tp-diag] lrproof_forward node={} link={} iface={}",
                config_name,
                packet.destination,
                iface
            );
        }
        let _ = TransportHandler::send_message_unlocked(
            handler.clone(),
            TxMessage { tx_type: TxMessageType::Direct(iface), packet },
        )
        .await;
    } else if packet.context == PacketContext::LinkRequestProof && diag::enabled() {
        log::debug!(
            "[tp-diag] lrproof_not_forwarded node={} link={} ingress_iface={}",
            config_name,
            packet.destination,
            iface
        );
    }
}

pub(super) fn handle_keepalive_response<'a>(
    packet: &Packet,
    handler: &mut MutexGuard<'a, TransportHandler>,
) -> (bool, Option<TxMessage>) {
    if packet.context == PacketContext::KeepAlive
        && packet.data.as_slice()[0] == KEEP_ALIVE_RESPONSE
    {
        let lookup = handler.link_table.handle_keepalive(packet);
        let message = lookup.map(|(propagated, iface)| TxMessage {
            tx_type: TxMessageType::Direct(iface),
            packet: propagated,
        });
        return (true, message);
    }

    (false, None)
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

pub(super) fn is_resource_data_packet(packet: &Packet) -> bool {
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

fn resource_packet_needs_decrypt(packet: &Packet) -> bool {
    matches!(
        packet.context,
        PacketContext::ResourceAdvrtisement
            | PacketContext::ResourceRequest
            | PacketContext::ResourceHashUpdate
            | PacketContext::ResourceInitiatorCancel
            | PacketContext::ResourceReceiverCancel
    )
}

pub(super) const MAX_RESOURCE_COMPLETION_WORKERS: usize = 4;

static RESOURCE_COMPLETION_PERMITS: std::sync::OnceLock<Arc<tokio::sync::Semaphore>> =
    std::sync::OnceLock::new();

pub(super) fn resource_completion_permits() -> Arc<tokio::sync::Semaphore> {
    RESOURCE_COMPLETION_PERMITS
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(MAX_RESOURCE_COMPLETION_WORKERS)))
        .clone()
}

async fn decrypt_link_resource_packet(
    packet: Packet,
    link: Arc<Mutex<Link>>,
) -> Result<Packet, RnsError> {
    if !resource_packet_needs_decrypt(&packet) {
        return Ok(packet);
    }

    let permit = resource_decrypt_permits().try_acquire_owned().map_err(|_| {
        log::debug!("resource: skipping resource packet decrypt while worker lane is saturated");
        RnsError::ConnectionError
    })?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let Ok(link) = link.try_lock() else {
            log::debug!("resource: skipping resource packet decrypt for busy link");
            return Err(RnsError::ConnectionError);
        };
        let mut buffer = PacketDataBuffer::new();
        let plain_len = link.decrypt(packet.data.as_slice(), buffer.accuire_buf_max())?.len();
        buffer.resize(plain_len);
        let mut plain_packet = packet;
        plain_packet.data = buffer;
        Ok(plain_packet)
    })
    .await
    .map_err(|_| RnsError::ConnectionError)?
}

pub(super) async fn complete_link_resource_on_worker(
    job: ResourceCompletionJob,
    link: Arc<Mutex<Link>>,
    remote_backend: Option<Arc<dyn WorkerBackend>>,
) -> Result<ResourceCompletion, RnsError> {
    let link_context = match link.try_lock() {
        Ok(link) => link.packet_context(),
        Err(_) => {
            log::debug!("resource: skipping resource completion for busy link");
            return Err(RnsError::ConnectionError);
        }
    };

    if let Some(remote_backend) = remote_backend {
        match complete_link_resource_on_remote_worker(
            job.to_snapshot(),
            &link_context,
            remote_backend,
        )
        .await
        {
            Ok(completion) => return Ok(completion),
            Err(err) => {
                log::debug!(
                    "resource: remote resource completion unavailable, falling back locally: {:?}",
                    err
                );
            }
        }
    }

    let permit = resource_completion_permits().try_acquire_owned().map_err(|_| {
        log::debug!("resource: skipping resource completion while worker lane is saturated");
        RnsError::ConnectionError
    })?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let link_id = job.link_id;
        let hash = job.resource_hash;
        let (proof, payload) = complete_resource_job(job, |ciphertext| {
            let mut out = vec![0u8; ciphertext.len() + 64];
            link_context
                .resource_decrypt(ciphertext, &mut out)
                .map(|plaintext| plaintext.to_vec())
                .map_err(|_| ())
        })
        .map_err(|_| RnsError::CryptoError)?;
        let proof_packet = build_resource_proof_packet(&link_context, proof)?;
        Ok(ResourceCompletion { hash, link_id, proof_packet, payload })
    })
    .await
    .map_err(|_| RnsError::ConnectionError)?
}

async fn complete_link_resource_on_remote_worker(
    snapshot: ResourceCompletionSnapshot,
    link_context: &LinkPacketContext,
    backend: Arc<dyn WorkerBackend>,
) -> Result<ResourceCompletion, RnsError> {
    let hash = Hash::new(snapshot.resource_hash);
    let link_id = AddressHash::new(snapshot.link_id);
    let response = backend
        .submit(WorkerJob {
            id: u64::from_be_bytes(snapshot.resource_hash[..8].try_into().unwrap_or([0; 8])),
            kind: WorkerJobKind::resource_complete_from_snapshot_with_link_context(
                snapshot,
                link_context.to_snapshot(),
            ),
        })
        .await
        .map_err(|err| {
            log::debug!("resource: remote resource completion failed: {:?}", err);
            RnsError::ConnectionError
        })?;

    let outcome = response.kind.into_resource_completion_outcome().map_err(|err| {
        log::debug!("resource: remote resource completion returned unexpected result: {:?}", err);
        RnsError::ConnectionError
    })?;

    let proof = ResourceProof {
        resource_hash: Hash::new(outcome.resource_hash),
        proof: Hash::new(outcome.proof),
    };
    let proof_packet = build_resource_proof_packet(link_context, proof)?;
    Ok(ResourceCompletion {
        hash,
        link_id,
        proof_packet,
        payload: ResourcePayload {
            data: outcome.data,
            metadata: outcome.metadata,
            request_id: outcome.request_id,
            is_request: outcome.is_request,
            is_response: outcome.is_response,
        },
    })
}

async fn decrypt_single_destination_payload(
    destination: Arc<Mutex<SingleInputDestination>>,
    packet: Packet,
    remote_batch_lane: Option<crypto_batch_lane::InboundCryptoBatchLane>,
) -> Result<(PacketDataBuffer, bool), RnsError> {
    if !should_encrypt_packet(&packet) {
        return Ok((packet.data, false));
    }

    if let Some(remote_batch_lane) = remote_batch_lane {
        match decrypt_single_destination_payload_on_remote_worker(
            destination.clone(),
            &packet,
            remote_batch_lane,
        )
        .await
        {
            Ok(result) => return Ok(result),
            Err(err) => {
                log::debug!(
                    "tp: remote single-destination decrypt unavailable, falling back locally: {:?}",
                    err
                );
            }
        }
    }

    decrypt_single_destination_payload_on_local_worker(destination, packet).await
}

async fn decrypt_single_destination_payload_on_local_worker(
    destination: Arc<Mutex<SingleInputDestination>>,
    packet: Packet,
) -> Result<(PacketDataBuffer, bool), RnsError> {
    let permit = single_destination_decrypt_permits().try_acquire_owned().map_err(|_| {
        log::debug!("tp: skipping single-destination decrypt while worker lane is saturated");
        RnsError::ConnectionError
    })?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let Ok(mut destination) = destination.try_lock() else {
            log::debug!("tp: skipping single-destination decrypt for busy destination");
            return Err(RnsError::ConnectionError);
        };
        let mut buffer = PacketDataBuffer::new();
        let (plaintext, ratchet_used) = destination
            .decrypt_with_ratchets_into(packet.data.as_slice(), buffer.accuire_buf_max())?;
        let plain_len = plaintext.len();
        buffer.resize(plain_len);
        Ok((buffer, ratchet_used))
    })
    .await
    .map_err(|_| RnsError::ConnectionError)?
}

async fn decrypt_single_destination_payload_on_remote_worker(
    destination: Arc<Mutex<SingleInputDestination>>,
    packet: &Packet,
    batch_lane: crypto_batch_lane::InboundCryptoBatchLane,
) -> Result<(PacketDataBuffer, bool), RnsError> {
    let private_key = {
        let Ok(destination) = destination.try_lock() else {
            log::debug!("tp: skipping remote single-destination decrypt for busy destination");
            return Err(RnsError::ConnectionError);
        };
        destination.identity.to_private_key_bytes()
    };
    let packet_wire = packet.to_bytes()?;
    let mut destination_hash = [0u8; crate::hash::ADDRESS_HASH_SIZE];
    destination_hash.copy_from_slice(packet.destination.as_slice());
    let result = batch_lane
        .decrypt(SingleDestinationDecryptBatchItem {
            packet_wire,
            destination: destination_hash,
            private_key: serde_bytes::ByteBuf::from(private_key.to_vec()),
        })
        .await
        .map_err(|err| {
            log::debug!("tp: remote single-destination decrypt failed: {:?}", err);
            RnsError::ConnectionError
        })?;

    Ok((PacketDataBuffer::new_from_slice(result.payload.as_ref()), result.ratchet_used))
}

pub(super) async fn handle_local_single_destination_data(
    packet: &Packet,
    destination: Arc<Mutex<SingleInputDestination>>,
    received_data_tx: broadcast::Sender<ReceivedData>,
    config_name: &str,
    remote_batch_lane: Option<crypto_batch_lane::InboundCryptoBatchLane>,
) -> bool {
    let (buffer, ratchet_used) =
        match decrypt_single_destination_payload(destination, *packet, remote_batch_lane).await {
            Ok(result) => result,
            Err(err) => {
                log::warn!(
                    "tp({}): decrypt failed for {}: {:?}",
                    config_name,
                    packet.destination,
                    err
                );
                return true;
            }
        };

    received_data_tx
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
    true
}

pub(super) async fn handle_link_resource_data(
    packet: Packet,
    handler_arc: Arc<Mutex<TransportHandler>>,
) -> bool {
    if !is_resource_data_packet(&packet) {
        return false;
    }

    let link = link_for_destination_unlocked(handler_arc.clone(), &packet.destination).await;

    let Some(link) = link else {
        return false;
    };

    let packet_for_manager = match decrypt_link_resource_packet(packet, link.clone()).await {
        Ok(packet) => packet,
        Err(err) => {
            log::warn!("resource: failed to decrypt packet: {:?}", err);
            return true;
        }
    };

    let (completion_job, responses, events, events_tx) = {
        let (resource_lane, events_tx) = {
            let handler = handler_arc.lock().await;
            (handler.resource_lane.clone(), handler.resource_events_tx.clone())
        };
        let result = resource_lane.handle_link_packet(packet_for_manager, link.clone()).await;
        (result.completion_job, result.responses, result.events, events_tx)
    };
    for response in responses {
        let _ =
            TransportHandler::send_packet_with_trace_unlocked(handler_arc.clone(), response).await;
    }
    for event in events {
        let _ = events_tx.send(event);
    }

    if let Some(job) = completion_job {
        let resource_worker_backend = {
            let handler = handler_arc.lock().await;
            handler.resource_worker_backend.clone()
        };
        match complete_link_resource_on_worker(job, link.clone(), resource_worker_backend).await {
            Ok(completion) => {
                let (proof_packet, events, events_tx) = {
                    let (resource_lane, events_tx) = {
                        let handler = handler_arc.lock().await;
                        (handler.resource_lane.clone(), handler.resource_events_tx.clone())
                    };
                    let (proof_packet, events) = resource_lane.finish_completion(completion).await;
                    (proof_packet, events, events_tx)
                };
                let _ = TransportHandler::send_packet_with_trace_unlocked(
                    handler_arc.clone(),
                    proof_packet,
                )
                .await;
                for event in events {
                    let _ = events_tx.send(event);
                }
            }
            Err(err) => {
                log::warn!("resource: failed to complete resource on worker: {:?}", err);
            }
        }
    }
    true
}

pub(super) async fn handle_data<'a>(
    packet: &Packet,
    iface: AddressHash,
    handler_arc: Arc<Mutex<TransportHandler>>,
    mut handler: MutexGuard<'a, TransportHandler>,
) {
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
            drop(handler);
            if handle_link_resource_data(*packet, handler_arc.clone()).await {
                return;
            }
            handler = handler_arc.lock().await;
        }

        log::trace!(
            "[tp] link_data dst={} ctx={:02x} len={}",
            packet.destination,
            packet.context as u8,
            packet.data.len()
        );
        let (in_link, out_links, handled_keepalive, keepalive_message, next_hop_message) = {
            let in_link = handler.in_links.get(&packet.destination).cloned();
            let out_links = handler.out_links.values().cloned().collect::<Vec<_>>();
            let (handled_keepalive, keepalive_message) =
                handle_keepalive_response(packet, &mut handler);

            let lookup = handler.link_table.original_destination(&packet.destination);
            let mut next_hop_message = None;
            if lookup.is_some() {
                next_hop_message = message_to_next_hop(packet, &handler, lookup);
                let sent = next_hop_message.is_some();

                log::trace!(
                    "tp({}): {} packet to remote link {}",
                    handler.config.name,
                    if sent { "forwarded" } else { "could not forward" },
                    packet.destination
                );
            }

            (in_link, out_links, handled_keepalive, keepalive_message, next_hop_message)
        };
        drop(handler);

        let mut link_packets = Vec::new();
        if let Some(link) = in_link {
            match link.try_lock() {
                Ok(mut link) => {
                    let result = link.handle_packet(packet, iface);
                    if let LinkHandleResult::KeepAlive = result {
                        link_packets.push(link.keep_alive_packet(KEEP_ALIVE_RESPONSE));
                    } else if let LinkHandleResult::Proof(proof_packet) = result {
                        link_packets.push(proof_packet);
                    }
                }
                Err(_) => {
                    log::debug!("tp: skipping busy inbound link during link data handling");
                }
            }
        }

        let proof_packets = collect_ready_outbound_link_proofs(packet, iface, out_links);

        let direct_messages = link_packets
            .into_iter()
            .chain(proof_packets)
            .map(|packet| TxMessage { tx_type: TxMessageType::Direct(iface), packet })
            .collect::<Vec<_>>();

        if handled_keepalive {
            for message in direct_messages {
                let _ = TransportHandler::send_message_unlocked(handler_arc.clone(), message).await;
            }
            if let Some(message) = keepalive_message {
                let _ = TransportHandler::send_message_unlocked(handler_arc.clone(), message).await;
            }
            return;
        }

        for message in direct_messages {
            let _ = TransportHandler::send_message_unlocked(handler_arc.clone(), message).await;
        }
        if let Some(message) = next_hop_message {
            let _ = TransportHandler::send_message_unlocked(handler_arc.clone(), message).await;
        }
        return;
    }

    if packet.header.destination_type == DestinationType::Single {
        if let Some(destination) = handler.single_in_destinations.get(&packet.destination).cloned()
        {
            let received_data_tx = handler.received_data_tx.clone();
            let config_name = handler.config.name.clone();
            let inbound_crypto_batch_lane = handler.inbound_crypto_batch_lane.clone();
            drop(handler);
            handle_local_single_destination_data(
                packet,
                destination,
                received_data_tx,
                &config_name,
                inbound_crypto_batch_lane,
            )
            .await;
            log::trace!(
                "tp({}): handle data request for {} dst={:2x} ctx={:2x}",
                config_name,
                packet.destination,
                packet.header.destination_type as u8,
                packet.context as u8,
            );
        } else {
            let next_hop_message = message_to_next_hop(packet, &handler, None);
            let data_handled = next_hop_message.is_some();
            let config_name = handler.config.name.clone();
            let destination = packet.destination;
            let destination_type = packet.header.destination_type;
            let context = packet.context;
            drop(handler);
            if let Some(message) = next_hop_message {
                let _ = TransportHandler::send_message_unlocked(handler_arc.clone(), message).await;
            }
            if data_handled {
                log::trace!(
                    "tp({}): handle data request for {} dst={:2x} ctx={:2x}",
                    config_name,
                    destination,
                    destination_type as u8,
                    context as u8,
                );
            }
        }
    }
}
