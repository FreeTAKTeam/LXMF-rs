impl Link {
    pub fn new(
        destination: DestinationDesc,
        event_tx: tokio::sync::broadcast::Sender<LinkEventData>,
    ) -> Self {
        Self {
            id: AddressHash::new_empty(),
            destination,
            ingress_iface: None,
            priv_identity: PrivateIdentity::new_from_rand(OsRng),
            peer_identity: Identity::default(),
            identified_peer_identity: None,
            derived_key: DerivedKey::new_empty(),
            session_cipher: None,
            signalling: None,
            status: LinkStatus::Pending,
            request_time: Instant::now(),
            rtt: Duration::from_secs(0),
            activated_at: None,
            last_inbound: None,
            last_outbound: None,
            last_data: None,
            last_keepalive: None,
            last_proof: None,
            stale_since: None,
            keepalive: Duration::from_secs_f32(KEEPALIVE_MAX_SECS),
            stale_time: Duration::from_secs_f32(KEEPALIVE_MAX_SECS * STALE_FACTOR),
            next_channel_sequence: 0,
            next_channel_rx_sequence: 0,
            channel_open: false,
            next_channel_handler_id: 0,
            channel_handlers: HashMap::new(),
            channel_pending: HashMap::new(),
            channel_states: HashMap::new(),
            channel_rx_ring: HashMap::new(),
            pending_response_limits: HashMap::new(),
            channel_window: CHANNEL_WINDOW_INIT,
            channel_window_max: CHANNEL_WINDOW_MAX_SLOW,
            channel_window_min: CHANNEL_WINDOW_MIN,
            channel_window_flexibility: CHANNEL_WINDOW_FLEXIBILITY,
            channel_fast_rate_rounds: 0,
            channel_medium_rate_rounds: 0,
            event_tx,
        }
    }

    pub fn request(&mut self) -> Packet {
        if self.status != LinkStatus::Pending {
            self.refresh_local_identity();
        }

        let mut packet_data = PacketDataBuffer::new();

        packet_data.safe_write(self.priv_identity.as_identity().public_key.as_bytes());
        packet_data.safe_write(self.priv_identity.as_identity().verifying_key.as_bytes());
        // Pack the 3-byte MTU/mode signalling suffix that `new_from_request`
        // above already knows how to parse on receipt
        // (`LINK_MTU_SIZE`/`clamp_link_signalling`) but this method never
        // wrote at all — every outbound request signalled cipher mode `0`
        // by accident of the field being entirely absent, not by choice.
        // `LinkMode::DEFAULT` is always the one mode this build can
        // actually use (see its own doc comment for why there's no
        // fallback to try instead), and `RETICULUM_COMPAT_MTU` is the same
        // conservative ceiling this crate already clamps an *incoming*
        // signalled value to — advertising a larger value here was
        // confirmed live to make single-packet Request/Response traffic
        // fail against at least one real destination, even after the Link
        // itself activates successfully.
        let mtu_value =
            (RETICULUM_COMPAT_MTU & LINK_MTU_MASK) | ((LinkMode::DEFAULT.mode_bits() << 21) & LINK_MODE_MASK);
        packet_data.safe_write(&[
            ((mtu_value >> 16) & 0xFF) as u8,
            ((mtu_value >> 8) & 0xFF) as u8,
            (mtu_value & 0xFF) as u8,
        ]);

        let packet = Packet {
            header: Header { packet_type: PacketType::LinkRequest, ..Default::default() },
            ifac: None,
            destination: self.destination.address_hash,
            transport: None,
            context: PacketContext::None,
            data: packet_data,
        };

        self.status = LinkStatus::Pending;
        self.id = LinkId::from(&packet);
        self.derived_key = DerivedKey::new_empty();
        self.session_cipher = None;
        self.request_time = Instant::now();
        self.activated_at = None;
        self.ingress_iface = None;
        self.last_inbound = None;
        self.last_outbound = Some(self.request_time);
        self.last_data = Some(self.request_time);
        self.last_keepalive = None;
        self.last_proof = None;
        self.stale_since = None;
        self.keepalive = Duration::from_secs_f32(KEEPALIVE_MAX_SECS);
        self.stale_time = Duration::from_secs_f32(KEEPALIVE_MAX_SECS * STALE_FACTOR);
        self.next_channel_sequence = 0;
        self.next_channel_rx_sequence = 0;
        self.channel_open = false;
        self.channel_pending.clear();
        self.channel_states.clear();
        self.channel_rx_ring.clear();
        self.pending_response_limits.clear();
        self.reset_channel_flow_control();

        packet
    }

    pub fn prove(&mut self) -> Packet {
        log::debug!("link({}): prove", self.id);

        if self.status != LinkStatus::Active {
            self.status = LinkStatus::Active;
            let activated_at = Instant::now();
            self.activated_at = Some(activated_at);
            self.last_proof = Some(activated_at);
            self.stale_since = None;
            self.post_event(LinkEvent::Activated);
        }

        let mut packet_data = PacketDataBuffer::new();

        packet_data.safe_write(self.id.as_slice());
        packet_data.safe_write(self.priv_identity.as_identity().public_key.as_bytes());
        packet_data.safe_write(self.priv_identity.as_identity().verifying_key.as_bytes());
        if let Some(signalling) = self.signalling {
            packet_data.safe_write(&signalling);
        }

        let signature = self.priv_identity.sign(packet_data.as_slice());

        packet_data.reset();
        packet_data.safe_write(&signature.to_bytes()[..]);
        packet_data.safe_write(self.priv_identity.as_identity().public_key.as_bytes());
        if let Some(signalling) = self.signalling {
            packet_data.safe_write(&signalling);
        }

        Packet {
            header: Header {
                packet_type: PacketType::Proof,
                destination_type: DestinationType::Link,
                hops: 0,
                ..Default::default()
            },
            ifac: None,
            destination: self.id,
            transport: None,
            context: PacketContext::LinkRequestProof,
            data: packet_data,
        }
    }

    pub fn prove_packet(&self, packet: &Packet) -> Packet {
        let hash = packet.hash().to_bytes();
        let signature = self.priv_identity.sign(&hash).to_bytes();
        let mut packet_data = PacketDataBuffer::new();

        packet_data.safe_write(&hash);
        packet_data.safe_write(&signature);

        Packet {
            header: Header {
                packet_type: PacketType::Proof,
                destination_type: DestinationType::Link,
                ..Default::default()
            },
            ifac: None,
            destination: self.id,
            transport: None,
            context: PacketContext::LinkProof,
            data: packet_data,
        }
    }

    fn handle_data_packet(&mut self, packet: &Packet) -> LinkHandleResult {
        if self.status != LinkStatus::Active {
            log::warn!("link({}): handling data packet in inactive state", self.id);
        }

        match packet.context {
            PacketContext::Channel => {
                if !self.channel_is_open() {
                    log::debug!("link({}): channel data received without open channel", self.id);
                    return LinkHandleResult::None;
                }

                // Sized from the ciphertext, not from a fixed 464-byte
                // array: decrypted output is never larger than what came
                // in, and once a link negotiates a bigger MTU a single
                // data packet legitimately exceeds `PACKET_MDU`. The
                // fixed buffer made those decrypts fail silently.
                let mut buffer = vec![0u8; packet.data.as_slice().len().max(PACKET_MDU)];
                if let Ok(plain_text) = self.decrypt(packet.data.as_slice(), &mut buffer[..]) {
                    self.note_inbound(packet.context);
                    log::trace!("link({}): data {}B", self.id, plain_text.len());
                    self.handle_channel_frame(plain_text);
                    return LinkHandleResult::Proof(self.prove_packet(packet));
                }
                log::error!("link({}): can't decrypt packet", self.id);
                return LinkHandleResult::None;
            }
            PacketContext::LinkIdentify => {
                // Sized from the ciphertext, not from a fixed 464-byte
                // array: decrypted output is never larger than what came
                // in, and once a link negotiates a bigger MTU a single
                // data packet legitimately exceeds `PACKET_MDU`. The
                // fixed buffer made those decrypts fail silently.
                let mut buffer = vec![0u8; packet.data.as_slice().len().max(PACKET_MDU)];
                if let Ok(plain_text) = self.decrypt(packet.data.as_slice(), &mut buffer[..]) {
                    if let Some(identity) = parse_link_identify_payload(plain_text, &self.id) {
                        self.note_inbound(packet.context);
                        self.post_event(LinkEvent::PeerIdentified(Box::new(identity)));
                    } else {
                        log::warn!("link({}): invalid identify payload, dropping", self.id);
                    }
                } else {
                    log::error!("link({}): can't decrypt identify packet", self.id);
                }
            }
            PacketContext::None | PacketContext::Request | PacketContext::Response => {
                // Sized from the ciphertext, not from a fixed 464-byte
                // array: decrypted output is never larger than what came
                // in, and once a link negotiates a bigger MTU a single
                // data packet legitimately exceeds `PACKET_MDU`. The
                // fixed buffer made those decrypts fail silently.
                let mut buffer = vec![0u8; packet.data.as_slice().len().max(PACKET_MDU)];
                if let Ok(plain_text) = self.decrypt(packet.data.as_slice(), &mut buffer[..]) {
                    self.note_inbound(packet.context);
                    log::trace!("link({}): data {}B", self.id, plain_text.len());
                    let (request_id, response_accepted) = if packet.context == PacketContext::Request {
                        let hash = packet.hash().to_bytes();
                        let mut id = [0u8; ADDRESS_HASH_SIZE];
                        id.copy_from_slice(&hash[..ADDRESS_HASH_SIZE]);
                        (Some(id), true)
                    } else if packet.context == PacketContext::Response {
                        match response_envelope_metadata(plain_text) {
                            Some((id, response_size)) => {
                                let accepted = id.as_ref().is_none_or(|id| {
                                    self.take_response_limit_if_allowed(id, response_size)
                                });
                                (id, accepted)
                            }
                            None => {
                                log::warn!(
                                    "link({}): response is not a valid request envelope",
                                    self.id
                                );
                                (None, true)
                            }
                        }
                    } else {
                        (None, true)
                    };
                    if !response_accepted {
                        log::warn!(
                            "link({}): response exceeded the pending request size limit",
                            self.id
                        );
                        return LinkHandleResult::None;
                    };
                    self.post_event(LinkEvent::Data(Box::new(
                        LinkPayload::new_from_slice_with_context_and_request_id(
                            plain_text,
                            packet.context,
                            request_id,
                        ),
                    )));
                    if packet.context == PacketContext::None {
                        return LinkHandleResult::Proof(self.prove_packet(packet));
                    }
                    return LinkHandleResult::None;
                } else {
                    log::error!("link({}): can't decrypt packet", self.id);
                }
            }
            PacketContext::KeepAlive => {
                if packet.data.as_slice() == [0xFF] {
                    self.note_inbound(packet.context);
                    self.request_time = Instant::now();
                    log::trace!("link({}): keep-alive request", self.id);
                    return LinkHandleResult::KeepAlive;
                }
                if packet.data.as_slice() == [0xFE] {
                    self.note_inbound(packet.context);
                    log::trace!("link({}): keep-alive response", self.id);
                    return LinkHandleResult::None;
                }
            }
            PacketContext::LinkClose => {
                // Sized from the ciphertext, not from a fixed 464-byte
                // array: decrypted output is never larger than what came
                // in, and once a link negotiates a bigger MTU a single
                // data packet legitimately exceeds `PACKET_MDU`. The
                // fixed buffer made those decrypts fail silently.
                let mut buffer = vec![0u8; packet.data.as_slice().len().max(PACKET_MDU)];
                match self.decrypt(packet.data.as_slice(), &mut buffer[..]) {
                    Ok(plain_text) if plain_text == self.id.as_slice() => {
                        self.note_inbound(packet.context);
                        self.finalize_local_close();
                    }
                    Ok(plain_text) => {
                        log::warn!(
                            "link({}): ignored link close with mismatched payload len={}",
                            self.id,
                            plain_text.len()
                        );
                    }
                    Err(err) => {
                        log::warn!("link({}): failed to decrypt link close: {:?}", self.id, err);
                    }
                }
                return LinkHandleResult::None;
            }
            PacketContext::LinkRTT => {
                // Sized from the ciphertext, not from a fixed 464-byte
                // array: decrypted output is never larger than what came
                // in, and once a link negotiates a bigger MTU a single
                // data packet legitimately exceeds `PACKET_MDU`. The
                // fixed buffer made those decrypts fail silently.
                let mut buffer = vec![0u8; packet.data.as_slice().len().max(PACKET_MDU)];
                if let Ok(plain_text) = self.decrypt(packet.data.as_slice(), &mut buffer[..]) {
                    let mut cursor = std::io::Cursor::new(plain_text);
                    if let Ok(peer_rtt) = rmp::decode::read_f32(&mut cursor) {
                        let consumed_all = cursor.position() == plain_text.len() as u64;
                        if consumed_all
                            && peer_rtt.is_finite()
                            && (0.0..=KEEPALIVE_MAX_SECS).contains(&peer_rtt)
                        {
                            let measured_rtt = self.request_time.elapsed().as_secs_f32();
                            self.rtt = Duration::from_secs_f32(measured_rtt.max(peer_rtt));
                            self.update_keepalive_timing();
                            self.refresh_channel_flow_control();
                            if self.activated_at.is_none() {
                                self.activated_at = Some(Instant::now());
                            }
                            self.note_inbound(packet.context);
                        } else {
                            log::warn!("link({}): invalid RTT payload", self.id);
                        }
                    }
                }
            }
            _ => {}
        }

        LinkHandleResult::None
    }

    fn iface_matches(&self, iface: AddressHash) -> bool {
        if let Some(expected_iface) = self.ingress_iface {
            if expected_iface != iface {
                log::warn!(
                    "link({}): dropping packet from iface {} expected {}",
                    self.id,
                    iface,
                    expected_iface
                );
                return false;
            }
        }

        true
    }

    pub fn handle_packet(&mut self, packet: &Packet, iface: AddressHash) -> LinkHandleResult {
        if packet.destination != self.id {
            return LinkHandleResult::None;
        }
        if !self.iface_matches(iface) {
            return LinkHandleResult::None;
        }

        match packet.header.packet_type {
            PacketType::Data => return self.handle_data_packet(packet),
            PacketType::Proof => return self.handle_proof_packet(packet, iface),
            _ => {}
        }

        LinkHandleResult::None
    }

    pub fn data_packet(&self, data: &[u8]) -> Result<Packet, RnsError> {
        self.packet_with_context(data, PacketContext::None)
    }

    pub fn channel_packet(&self, data: &[u8]) -> Result<Packet, RnsError> {
        self.packet_with_context(data, PacketContext::Channel)
    }

    /// Build a link peer identification packet (context = 0xFB LinkIdentify).
    /// Payload: `public_key ++ verifying_key ++
    /// sign(link_id ++ public_key ++ verifying_key)`.
    pub fn identify_packet(&self, payload: &[u8]) -> Result<Packet, RnsError> {
        self.packet_with_context(payload, PacketContext::LinkIdentify)
    }

    pub fn register_channel_handler<F>(&mut self, msg_type: u16, handler: F) -> HandlerId
    where
        F: FnMut(ChannelEnvelope) -> bool + Send + 'static,
    {
        self.channel_open = true;
        let id = HandlerId::new(self.next_channel_handler_id);
        self.next_channel_handler_id = self.next_channel_handler_id.wrapping_add(1);
        self.channel_handlers
            .entry(msg_type)
            .or_default()
            .push(RegisteredChannelHandler { id, handler: Box::new(handler) });
        id
    }
}

include!("new_from_request.rs");

const LINK_IDENTIFY_PAYLOAD_LENGTH: usize = PUBLIC_KEY_LENGTH * 2 + SIGNATURE_LENGTH;

fn parse_link_identify_payload(payload: &[u8], link_id: &AddressHash) -> Option<Identity> {
    if payload.len() != LINK_IDENTIFY_PAYLOAD_LENGTH {
        return None;
    }
    let identity = Identity::try_new_from_slices(
        &payload[..PUBLIC_KEY_LENGTH],
        &payload[PUBLIC_KEY_LENGTH..PUBLIC_KEY_LENGTH * 2],
    )
    .ok()?;
    let signature = Signature::from_slice(
        &payload[PUBLIC_KEY_LENGTH * 2..PUBLIC_KEY_LENGTH * 2 + SIGNATURE_LENGTH],
    )
    .ok()?;
    let mut signed = Vec::with_capacity(ADDRESS_HASH_SIZE + PUBLIC_KEY_LENGTH * 2);
    signed.extend_from_slice(link_id.as_slice());
    signed.extend_from_slice(&payload[..PUBLIC_KEY_LENGTH * 2]);
    identity.verify(&signed, &signature).ok()?;
    Some(identity)
}
