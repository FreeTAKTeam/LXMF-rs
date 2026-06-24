#[derive(Debug)]
pub struct ResourceManager {
    pending_outgoing: HashMap<Hash, ResourceSender>,
    outgoing: HashMap<Hash, ResourceSender>,
    incoming: HashMap<Hash, ResourceReceiver>,
    events: Vec<ResourceEvent>,
    retry_interval: Duration,
    retry_limit: u8,
    link_stats: HashMap<AddressHash, LinkStats>,
}

impl ResourceManager {
    pub fn confirm_outbound_dispatch(&mut self, resource_hash: Hash, sent: bool) {
        let Some(mut sender) = self.pending_outgoing.remove(&resource_hash) else {
            return;
        };

        if sent {
            sender.mark_advertised(self.retry_limit);
            self.outgoing.insert(resource_hash, sender);
        } else {
            self.events.push(ResourceEvent {
                hash: resource_hash,
                link_id: sender.link_id,
                kind: ResourceEventKind::OutboundFailed,
            });
        }
    }

    pub fn cancel_outgoing(
        &mut self,
        resource_hash: Hash,
        link: &Link,
    ) -> Result<Option<Packet>, RnsError> {
        if self.outgoing.contains_key(&resource_hash) {
            let packet = build_link_packet(
                link,
                PacketType::Data,
                PacketContext::ResourceInitiatorCancel,
                resource_hash.as_slice(),
            )?;
            let sender = self
                .outgoing
                .remove(&resource_hash)
                .expect("outgoing sender existed before cancel packet");
            self.events.push(ResourceEvent {
                hash: resource_hash,
                link_id: sender.link_id,
                kind: ResourceEventKind::OutboundCancelled,
            });
            return Ok(Some(packet));
        }

        if let Some(sender) = self.pending_outgoing.remove(&resource_hash) {
            self.events.push(ResourceEvent {
                hash: resource_hash,
                link_id: sender.link_id,
                kind: ResourceEventKind::OutboundCancelled,
            });
        }
        Ok(None)
    }

    pub fn remove_link_state(&mut self, link_id: AddressHash) {
        self.pending_outgoing.retain(|_, sender| sender.link_id != link_id);
        self.outgoing.retain(|_, sender| sender.link_id != link_id);
        self.incoming.retain(|_, receiver| receiver.link_id != link_id);
        self.link_stats.remove(&link_id);
    }

    pub fn drain_events(&mut self) -> Vec<ResourceEvent> {
        std::mem::take(&mut self.events)
    }

    #[cfg(test)]
    pub(crate) fn has_no_outbound_state(&self) -> bool {
        self.pending_outgoing.is_empty() && self.outgoing.is_empty()
    }

    pub fn retry_requests(&mut self, now: Instant) -> Vec<(AddressHash, ResourceRequest)> {
        let mut requests = Vec::new();
        let mut failed = Vec::new();
        for (hash, receiver) in self.incoming.iter_mut() {
            if receiver.retry_due(now, self.retry_interval, self.retry_limit) {
                let rtt = self.link_stats.get(&receiver.link_id)
                    .map(|s| s.rtt)
                    .unwrap_or(LinkStats::new().rtt);
                let request = receiver.build_request(now, rtt);
                if !request.requested_hashes.is_empty() || request.hashmap_exhausted {
                    receiver.mark_request();
                    requests.push((receiver.link_id, request));
                }
            }
            if receiver.retry_count >= self.retry_limit {
                failed.push((*hash, receiver.link_id, receiver.progress()));
            }
        }
        for (hash, link_id, progress) in failed {
            log::warn!("resource transfer failed link={link_id} hash={hash} reason=retry_limit_exhausted");
            self.incoming.remove(&hash);
            self.events.push(ResourceEvent {
                hash,
                link_id,
                kind: ResourceEventKind::InboundFailed(ResourceFailure {
                    reason: "retry_limit_exhausted".to_string(),
                    progress,
                }),
            });
        }
        requests
    }

    pub fn poll_outgoing(&mut self, now: Instant) -> Vec<(AddressHash, Packet)> {
        let mut packets = Vec::new();
        let mut failed = Vec::new();

        for (hash, sender) in self.outgoing.iter_mut() {
            match sender.poll(now, self.retry_interval) {
                OutboundResourcePoll::Send(packet) => {
                    packets.push((sender.link_id, (*packet).clone()));
                }
                OutboundResourcePoll::Failed => {
                    self.events.push(ResourceEvent {
                        hash: *hash,
                        link_id: sender.link_id,
                        kind: ResourceEventKind::OutboundFailed,
                    });
                    failed.push(*hash);
                }
                OutboundResourcePoll::None => {}
            }
        }

        for hash in failed {
            self.outgoing.remove(&hash);
        }

        packets
    }

    pub fn handle_packet(&mut self, packet: &Packet, link: &mut Link) -> Vec<Packet> {
        let mut responses = Vec::new();
        self.handle_packet_into(packet, link, &mut responses);
        responses
    }

    pub fn handle_packet_into(
        &mut self,
        packet: &Packet,
        link: &mut Link,
        responses: &mut Vec<Packet>,
    ) {
        self.handle_packet_into_with_mtu(packet, link, responses, DEFAULT_RESOURCE_INTERFACE_MTU);
    }

    pub fn handle_packet_into_with_mtu(
        &mut self,
        packet: &Packet,
        link: &mut Link,
        responses: &mut Vec<Packet>,
        interface_mtu: usize,
    ) {
        responses.clear();
        match packet.context {
            PacketContext::ResourceAdvrtisement => {
                self.handle_advertisement_into(packet, link, responses, interface_mtu)
            }
            PacketContext::ResourceRequest => self.handle_request_into(packet, link, responses),
            PacketContext::ResourceHashUpdate => {
                self.handle_hash_update_into(packet, link, responses)
            }
            PacketContext::Resource => self.handle_resource_part_into(packet, link, responses),
            PacketContext::ResourceProof => self.handle_proof_into(packet, responses),
            PacketContext::ResourceInitiatorCancel | PacketContext::ResourceReceiverCancel => {
                self.cancel_into(packet, responses)
            }
            _ => {}
        }
    }

    fn handle_advertisement_into(
        &mut self,
        packet: &Packet,
        link: &mut Link,
        responses: &mut Vec<Packet>,
        interface_mtu: usize,
    ) {
        let Ok(advertisement) = ResourceAdvertisement::unpack(packet.data.as_slice()) else {
            log::debug!("[resource-diag] reject_advertisement unpack_failed");
            return;
        };
        log::debug!(
            "[resource-diag] advertisement link={} hash={} transfer_size={} data_size={} parts={} flags=0x{:02x} request={} response={} metadata={} compressed={} encrypted={}",
            link.id(),
            advertisement.hash,
            advertisement.transfer_size,
            advertisement.data_size,
            advertisement.parts,
            advertisement.flags,
            advertisement.is_request(),
            advertisement.is_response(),
            (advertisement.flags & FLAG_METADATA) == FLAG_METADATA,
            advertisement.compressed(),
            advertisement.encrypted()
        );
        if (advertisement.flags & FLAG_SPLIT) == FLAG_SPLIT {
            log::warn!(
                "rejecting unsupported advertisement flags (split={})",
                (advertisement.flags & FLAG_SPLIT) == FLAG_SPLIT
            );
            log::debug!("[resource-diag] reject_advertisement split");
            return;
        }
        let resource_hash = advertisement.hash;
        if self.incoming.get(&resource_hash).is_some_and(|receiver| receiver.is_active()) {
            log::debug!("[resource-diag] advertisement_duplicate hash={resource_hash}");
            log::debug!("resource inbound: duplicate advertisement for active receiver hash={}", resource_hash);
            return;
        }
        let receiver = if interface_mtu == DEFAULT_RESOURCE_INTERFACE_MTU {
            ResourceReceiver::new(&advertisement, *link.id())
        } else {
            ResourceReceiver::new_with_mtu(&advertisement, *link.id(), interface_mtu)
        };
        let Ok(mut receiver) = receiver else {
            log::warn!("rejecting unreasonable advertisement");
            log::debug!("[resource-diag] reject_advertisement unreasonable");
            return;
        };
        let adv_now = Instant::now();
        let rtt = self.link_stats.entry(*link.id()).or_insert_with(LinkStats::new).rtt;
        let request = receiver.build_request(adv_now, rtt);
        log::debug!(
            "[resource-diag] request_parts hash={} requested={} exhausted={}",
            resource_hash,
            request.requested_hashes.len(),
            request.hashmap_exhausted
        );
        receiver.mark_request();
        self.incoming.insert(resource_hash, receiver);
        match build_link_packet(
            link,
            PacketType::Data,
            PacketContext::ResourceRequest,
            &request.encode(),
        ) {
            Ok(packet) => responses.push(packet),
            Err(_) => {
                log::warn!("failed to build request packet");
            }
        };
    }

    fn handle_request_into(
        &mut self,
        packet: &Packet,
        link: &mut Link,
        responses: &mut Vec<Packet>,
    ) {
        let Ok(request) = ResourceRequest::decode(packet.data.as_slice()) else {
            log::debug!("[resource-diag] request_decode_failed link={}", link.id());
            return;
        };
        log::debug!(
            "[resource-diag] request_received link={} hash={} requested={} exhausted={} sender_present={}",
            link.id(),
            request.resource_hash,
            request.requested_hashes.len(),
            request.hashmap_exhausted,
            self.outgoing.contains_key(&request.resource_hash)
        );
        if let Some(sender) = self.outgoing.get_mut(&request.resource_hash) {
            sender.handle_request_into(&request, link, responses);
            log::debug!(
                "[resource-diag] request_responses link={} hash={} responses={}",
                link.id(),
                request.resource_hash,
                responses.len()
            );
        }
    }

    fn handle_hash_update_into(
        &mut self,
        packet: &Packet,
        link: &mut Link,
        responses: &mut Vec<Packet>,
    ) {
        let Ok(update) = ResourceHashUpdate::decode(packet.data.as_slice()) else {
            return;
        };
        if let Some(receiver) = self.incoming.get_mut(&update.resource_hash) {
            receiver.handle_hash_update(&update);
            let update_now = Instant::now();
            let rtt = self.link_stats.get(&receiver.link_id)
                .map(|s| s.rtt)
                .unwrap_or(LinkStats::new().rtt);
            let request = receiver.build_request(update_now, rtt);
            match build_link_packet(
                link,
                PacketType::Data,
                PacketContext::ResourceRequest,
                &request.encode(),
            ) {
                Ok(packet) => responses.push(packet),
                Err(_) => {
                    log::warn!("failed to build request packet");
                }
            };
        }
    }

    fn handle_resource_part_into(
        &mut self,
        packet: &Packet,
        link: &mut Link,
        responses: &mut Vec<Packet>,
    ) {
        let mut completed: Option<Hash> = None;
        let mut proof_packet: Option<Packet> = None;
        let mut request_packet: Option<Packet> = None;
        let mut payload: Option<ResourcePayload> = None;
        let mut failed: Option<(Hash, AddressHash, ResourceProgress, &'static str)> = None;
        for (hash, receiver) in self.incoming.iter_mut() {
            let before_received = receiver.received;
            match receiver.handle_part(packet.data.as_slice(), link) {
                PartOutcome::NoMatch => continue,
                PartOutcome::Failed(reason) => {
                    failed = Some((*hash, receiver.link_id, receiver.progress(), reason));
                    break;
                }
                PartOutcome::Complete(packet, data_payload) => {
                    log::debug!(
                        "[resource-diag] complete hash={} len={} metadata={}",
                        hash,
                        data_payload.data.len(),
                        data_payload.metadata.as_ref().map(|data| data.len()).unwrap_or(0)
                    );
                    completed = Some(*hash);
                    proof_packet = Some(packet);
                    payload = Some(data_payload);
                    break;
                }
                PartOutcome::Incomplete => {
                    let now = Instant::now();
                    let stats = self.link_stats
                        .entry(receiver.link_id)
                        .or_insert_with(LinkStats::new);

                    // Collect RTT sample measured during handle_part (if any).
                    if let Some(rtt) = receiver.last_rtt_sample.take() {
                        stats.update_rtt(rtt);
                    }

                    if receiver.received > before_received {
                        stats.record_arrival(now);
                        log::debug!(
                            "[resource-diag] progress hash={} received={}/{} bytes={}/{}",
                            hash,
                            receiver.received,
                            receiver.parts.len(),
                            receiver.received_bytes,
                            receiver.total_bytes
                        );
                        self.events.push(ResourceEvent {
                            hash: *hash,
                            link_id: receiver.link_id,
                            kind: ResourceEventKind::Progress(receiver.progress()),
                        });
                    }

                    let rtt = stats.rtt;
                    let request = receiver.build_request(now, rtt);
                    if !request.requested_hashes.is_empty() || request.hashmap_exhausted {
                        receiver.mark_active_request();
                        request_packet = match build_link_packet(
                            link,
                            PacketType::Data,
                            PacketContext::ResourceRequest,
                            &request.encode(),
                        ) {
                            Ok(packet) => Some(packet),
                            Err(_) => {
                                log::warn!("failed to build request packet");
                                None
                            }
                        };
                    }
                    break;
                }
            }
        }
        if let Some((hash, link_id, progress, reason)) = failed {
            log::warn!("resource transfer failed link={link_id} hash={hash} reason={reason}");
            self.incoming.remove(&hash);
            self.events.push(ResourceEvent {
                hash,
                link_id,
                kind: ResourceEventKind::InboundFailed(ResourceFailure {
                    reason: reason.to_string(),
                    progress,
                }),
            });
            // Reset so the inter-resource gap doesn't skew the arrival EWMA.
            // TODO: a better approach is to schedule a delayed reset — wait
            // arrival_interval * 2, and only reset if no new part has arrived by
            // then. This preserves the estimate when the next resource starts
            // immediately after this one finishes.
            if let Some(stats) = self.link_stats.get_mut(link.id()) {
                stats.last_arrival = None;
            }
            return;
        }
        if let Some(hash) = completed {
            self.incoming.remove(&hash);
            // Same TODO as the failed path above.
            if let Some(stats) = self.link_stats.get_mut(link.id()) {
                stats.last_arrival = None;
            }
            if let Some(payload) = payload {
                self.events.push(ResourceEvent {
                    hash,
                    link_id: *link.id(),
                    kind: ResourceEventKind::Complete(ResourceComplete {
                        data: payload.data,
                        metadata: payload.metadata,
                        request_id: payload.request_id,
                        is_request: payload.is_request,
                        is_response: payload.is_response,
                    }),
                });
            }
        }
        if let Some(packet) = proof_packet {
            responses.push(packet);
        } else if let Some(packet) = request_packet {
            responses.push(packet);
        }
    }

    fn handle_proof_into(&mut self, packet: &Packet, _responses: &mut Vec<Packet>) {
        let Ok(proof) = ResourceProof::decode(packet.data.as_slice()) else {
            return;
        };
        if let Some(sender) = self.outgoing.get_mut(&proof.resource_hash) {
            if sender.handle_proof(&proof) {
                self.outgoing.remove(&proof.resource_hash);
                self.events.push(ResourceEvent {
                    hash: proof.resource_hash,
                    link_id: packet.destination,
                    kind: ResourceEventKind::OutboundComplete,
                });
            }
        }
    }

    fn cancel_into(&mut self, packet: &Packet, _responses: &mut Vec<Packet>) {
        if let Ok(hash_bytes) = copy_hash(packet.data.as_slice()) {
            let hash = Hash::new(hash_bytes);
            self.incoming.remove(&hash);
            self.pending_outgoing.remove(&hash);
            self.outgoing.remove(&hash);
        }
    }
}

impl Default for ResourceManager {
    fn default() -> Self {
        Self::new()
    }
}

