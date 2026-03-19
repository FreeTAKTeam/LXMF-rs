#[derive(Debug)]
pub struct ResourceManager {
    pending_outgoing: HashMap<Hash, ResourceSender>,
    outgoing: HashMap<Hash, ResourceSender>,
    incoming: HashMap<Hash, ResourceReceiver>,
    events: Vec<ResourceEvent>,
    retry_interval: Duration,
    retry_limit: u8,
}

impl ResourceManager {
    pub fn new() -> Self {
        Self::new_with_config(Duration::from_secs(2), 5)
    }

    pub fn new_with_config(retry_interval: Duration, retry_limit: u8) -> Self {
        Self {
            pending_outgoing: HashMap::new(),
            outgoing: HashMap::new(),
            incoming: HashMap::new(),
            events: Vec::new(),
            retry_interval,
            retry_limit,
        }
    }

    pub fn start_send(
        &mut self,
        link: &Link,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
    ) -> Result<(Hash, Packet), RnsError> {
        let sender = ResourceSender::new(link, data, metadata)?;
        let resource_hash = sender.resource_hash;
        let packet = sender.advertisement_packet();
        self.pending_outgoing.insert(resource_hash, sender);
        Ok((resource_hash, packet))
    }

    pub fn confirm_outbound_dispatch(&mut self, resource_hash: Hash, sent: bool) {
        let Some(mut sender) = self.pending_outgoing.remove(&resource_hash) else {
            return;
        };

        if sent {
            sender.mark_advertised(self.retry_limit);
            self.outgoing.insert(resource_hash, sender);
        }
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
                let request = receiver.build_request();
                receiver.mark_request();
                requests.push((receiver.link_id, request));
            }
            if receiver.retry_count >= self.retry_limit {
                failed.push(*hash);
            }
        }
        for hash in failed {
            self.incoming.remove(&hash);
        }
        requests
    }

    pub fn poll_outgoing(&mut self, now: Instant) -> Vec<(AddressHash, Packet)> {
        let mut packets = Vec::new();
        let mut failed = Vec::new();

        for (hash, sender) in self.outgoing.iter_mut() {
            match sender.poll(now, self.retry_interval) {
                OutboundResourcePoll::Send(packet) => {
                    packets.push((sender.link_id, *packet));
                }
                OutboundResourcePoll::Failed => {
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
        responses.clear();
        match packet.context {
            PacketContext::ResourceAdvrtisement => {
                self.handle_advertisement_into(packet, link, responses)
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
    ) {
        let Ok(advertisement) = ResourceAdvertisement::unpack(packet.data.as_slice()) else {
            return;
        };
        if (advertisement.flags & FLAG_SPLIT) == FLAG_SPLIT {
            log::warn!(
                "resource: rejecting unsupported advertisement flags (split={})",
                (advertisement.flags & FLAG_SPLIT) == FLAG_SPLIT
            );
            return;
        }
        let resource_hash = advertisement.hash;
        if self.incoming.get(&resource_hash).is_some_and(|receiver| receiver.is_active()) {
            return;
        }
        let Ok(mut receiver) = ResourceReceiver::new(&advertisement, *link.id()) else {
            log::warn!("resource: rejecting unreasonable advertisement");
            return;
        };
        let request = receiver.build_request();
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
                log::warn!("resource: failed to build request packet");
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
            return;
        };
        if let Some(sender) = self.outgoing.get_mut(&request.resource_hash) {
            sender.handle_request_into(&request, link, responses);
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
            let request = receiver.build_request();
            match build_link_packet(
                link,
                PacketType::Data,
                PacketContext::ResourceRequest,
                &request.encode(),
            ) {
                Ok(packet) => responses.push(packet),
                Err(_) => {
                    log::warn!("resource: failed to build request packet");
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
        let mut failed: Option<Hash> = None;
        for (hash, receiver) in self.incoming.iter_mut() {
            let before_received = receiver.received;
            match receiver.handle_part(packet.data.as_slice(), link) {
                PartOutcome::NoMatch => continue,
                PartOutcome::Failed => {
                    failed = Some(*hash);
                    break;
                }
                PartOutcome::Complete(packet, data_payload) => {
                    completed = Some(*hash);
                    proof_packet = Some(packet);
                    payload = Some(data_payload);
                    break;
                }
                PartOutcome::Incomplete => {
                    let request = receiver.build_request();
                    receiver.mark_request();
                    request_packet = match build_link_packet(
                        link,
                        PacketType::Data,
                        PacketContext::ResourceRequest,
                        &request.encode(),
                    ) {
                        Ok(packet) => Some(packet),
                        Err(_) => {
                            log::warn!("resource: failed to build request packet");
                            None
                        }
                    };
                    if receiver.received > before_received {
                        self.events.push(ResourceEvent {
                            hash: *hash,
                            link_id: receiver.link_id,
                            kind: ResourceEventKind::Progress(receiver.progress()),
                        });
                    }
                    break;
                }
            }
        }
        if let Some(hash) = failed {
            self.incoming.remove(&hash);
            return;
        }
        if let Some(hash) = completed {
            self.incoming.remove(&hash);
            if let Some(payload) = payload {
                self.events.push(ResourceEvent {
                    hash,
                    link_id: *link.id(),
                    kind: ResourceEventKind::Complete(ResourceComplete {
                        data: payload.data,
                        metadata: payload.metadata,
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
