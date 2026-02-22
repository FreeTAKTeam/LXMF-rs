#[derive(Debug)]
pub struct ResourceManager {
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
        let advertisement = sender.advertisement(0);
        let payload = advertisement.pack()?;
        let packet = build_link_packet(
            link,
            PacketType::Data,
            PacketContext::ResourceAdvrtisement,
            &payload,
        )?;
        self.outgoing.insert(resource_hash, sender);
        Ok((resource_hash, packet))
    }

    pub fn drain_events(&mut self) -> Vec<ResourceEvent> {
        std::mem::take(&mut self.events)
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

    pub fn handle_packet(&mut self, packet: &Packet, link: &mut Link) -> Vec<Packet> {
        match packet.context {
            PacketContext::ResourceAdvrtisement => self.handle_advertisement(packet, link),
            PacketContext::ResourceRequest => self.handle_request(packet, link),
            PacketContext::ResourceHashUpdate => self.handle_hash_update(packet, link),
            PacketContext::Resource => self.handle_resource_part(packet, link),
            PacketContext::ResourceProof => self.handle_proof(packet),
            PacketContext::ResourceInitiatorCancel | PacketContext::ResourceReceiverCancel => {
                self.cancel(packet)
            }
            _ => Vec::new(),
        }
    }

    fn handle_advertisement(&mut self, packet: &Packet, link: &mut Link) -> Vec<Packet> {
        let Ok(advertisement) = ResourceAdvertisement::unpack(packet.data.as_slice()) else {
            return Vec::new();
        };
        if (advertisement.flags & FLAG_SPLIT) == FLAG_SPLIT {
            log::warn!(
                "resource: rejecting unsupported advertisement flags (split={})",
                (advertisement.flags & FLAG_SPLIT) == FLAG_SPLIT
            );
            return Vec::new();
        }
        let resource_hash = advertisement.hash;
        let mut receiver = ResourceReceiver::new(&advertisement, *link.id());
        let request = receiver.build_request();
        receiver.mark_request();
        self.incoming.insert(resource_hash, receiver);
        match build_link_packet(
            link,
            PacketType::Data,
            PacketContext::ResourceRequest,
            &request.encode(),
        ) {
            Ok(packet) => vec![packet],
            Err(_) => {
                log::warn!("resource: failed to build request packet");
                Vec::new()
            }
        }
    }

    fn handle_request(&mut self, packet: &Packet, link: &mut Link) -> Vec<Packet> {
        let Ok(request) = ResourceRequest::decode(packet.data.as_slice()) else {
            return Vec::new();
        };
        if let Some(sender) = self.outgoing.get_mut(&request.resource_hash) {
            sender.handle_request(&request, link)
        } else {
            Vec::new()
        }
    }

    fn handle_hash_update(&mut self, packet: &Packet, link: &mut Link) -> Vec<Packet> {
        let Ok(update) = ResourceHashUpdate::decode(packet.data.as_slice()) else {
            return Vec::new();
        };
        if let Some(receiver) = self.incoming.get_mut(&update.resource_hash) {
            receiver.handle_hash_update(&update);
            let request = receiver.build_request();
            return match build_link_packet(
                link,
                PacketType::Data,
                PacketContext::ResourceRequest,
                &request.encode(),
            ) {
                Ok(packet) => vec![packet],
                Err(_) => {
                    log::warn!("resource: failed to build request packet");
                    Vec::new()
                }
            };
        }
        Vec::new()
    }

    fn handle_resource_part(&mut self, packet: &Packet, link: &mut Link) -> Vec<Packet> {
        let mut completed: Option<Hash> = None;
        let mut proof_packet: Option<Packet> = None;
        let mut request_packet: Option<Packet> = None;
        let mut payload: Option<ResourcePayload> = None;
        for (hash, receiver) in self.incoming.iter_mut() {
            let before_received = receiver.received;
            match receiver.handle_part(packet.data.as_slice(), link) {
                PartOutcome::NoMatch => continue,
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
            return vec![packet];
        }
        if let Some(packet) = request_packet {
            return vec![packet];
        }
        Vec::new()
    }

    fn handle_proof(&mut self, packet: &Packet) -> Vec<Packet> {
        let Ok(proof) = ResourceProof::decode(packet.data.as_slice()) else {
            return Vec::new();
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
        Vec::new()
    }

    fn cancel(&mut self, packet: &Packet) -> Vec<Packet> {
        if let Ok(hash_bytes) = copy_hash(packet.data.as_slice()) {
            let hash = Hash::new(hash_bytes);
            self.incoming.remove(&hash);
            self.outgoing.remove(&hash);
        }
        Vec::new()
    }
}

impl Default for ResourceManager {
    fn default() -> Self {
        Self::new()
    }
}
