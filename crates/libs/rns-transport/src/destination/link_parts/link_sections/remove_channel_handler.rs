impl Link {

    pub fn remove_channel_handler(&mut self, handler_id: HandlerId) -> bool {
        let mut empty_msg_types = Vec::new();
        let mut removed = false;

        for (msg_type, handlers) in &mut self.channel_handlers {
            let before = handlers.len();
            handlers.retain(|registered| registered.id != handler_id);
            if handlers.is_empty() {
                empty_msg_types.push(*msg_type);
            }
            if handlers.len() != before {
                removed = true;
            }
        }

        for msg_type in empty_msg_types {
            self.channel_handlers.remove(&msg_type);
        }

        removed
    }

    pub fn send_channel_message(
        &mut self,
        msg_type: u16,
        payload: Vec<u8>,
    ) -> Result<(u16, Packet), ChannelError> {
        if self.status != LinkStatus::Active {
            return Err(ChannelError::LinkNotReady);
        }
        self.channel_open = true;
        if self.channel_pending.len() >= self.channel_send_window() {
            return Err(ChannelError::LinkNotReady);
        }

        let sequence = self.next_channel_sequence;
        self.next_channel_sequence = self.next_channel_sequence.wrapping_add(1);
        let envelope = ChannelEnvelope { msg_type, sequence, payload };
        let raw = envelope.pack();
        let packet = self.channel_packet(&raw).map_err(|_| ChannelError::PayloadTooLarge)?;
        self.channel_pending.insert(
            packet.hash(),
            PendingChannelPacket {
                sequence,
                packet: packet.clone(),
                tries: 1,
                next_retry_at: Instant::now()
                    + Self::channel_retry_timeout_for(self.rtt, 1, self.channel_pending.len() + 1),
            },
        );
        self.channel_states.insert(sequence, ChannelMessageState::Sent);
        Ok((sequence, packet.clone()))
    }

    pub fn channel_state(&self, sequence: u16) -> ChannelMessageState {
        self.channel_states.get(&sequence).copied().unwrap_or(ChannelMessageState::New)
    }

    pub fn open_channel(&mut self) {
        self.channel_open = true;
    }

    pub fn close_channel(&mut self) {
        self.channel_open = false;
    }

    pub(crate) fn mark_channel_failed(&mut self, sequence: u16) {
        if let Some(hash) = self
            .channel_pending
            .iter()
            .find_map(|(hash, pending)| (pending.sequence == sequence).then_some(*hash))
        {
            self.channel_pending.remove(&hash);
        }
        self.channel_states.insert(sequence, ChannelMessageState::Failed);
    }

    #[allow(dead_code)]
    pub(crate) fn poll_channel_timeouts(&mut self, now: Instant) -> Vec<Packet> {
        if !self.status.can_retry_channel_messages() {
            return Vec::new();
        }

        let timed_out = self
            .channel_pending
            .iter()
            .filter_map(|(hash, pending)| (pending.next_retry_at <= now).then_some(*hash))
            .collect::<Vec<_>>();
        if timed_out.is_empty() {
            return Vec::new();
        }

        let outstanding = self.channel_pending.len().max(1);
        let rtt = self.rtt;
        let mut resend_packets = Vec::new();
        let mut exhausted = false;

        for hash in timed_out {
            self.note_channel_timeout();
            if let Some(pending) = self.channel_pending.get_mut(&hash) {
                if pending.tries >= CHANNEL_MAX_TRIES {
                    exhausted = true;
                    break;
                }

                pending.tries += 1;
                let tries = pending.tries;
                let retry_timeout = Self::channel_retry_timeout_for(rtt, tries, outstanding);
                pending.next_retry_at = now + retry_timeout;
                resend_packets.push(pending.packet.clone());
            }
        }

        if exhausted {
            for pending in self.channel_pending.drain().map(|(_, pending)| pending) {
                self.channel_states.insert(pending.sequence, ChannelMessageState::Failed);
            }
            self.close();
            return Vec::new();
        }

        resend_packets
    }

    #[allow(dead_code)]
    pub(crate) fn next_channel_retry_at(&self) -> Option<Instant> {
        if !self.status.can_retry_channel_messages() {
            return None;
        }

        self.channel_pending.values().map(|pending| pending.next_retry_at).min()
    }

    fn channel_send_window(&self) -> usize {
        usize::from(self.channel_window)
    }

    pub fn channel_ready_to_send(&self) -> bool {
        self.status.can_exchange_data()
            && self.ingress_iface.is_some()
            && self.channel_pending.len() < self.channel_send_window()
    }

    pub fn channel_close_wait_hint(&self) -> Duration {
        Duration::from_secs_f32(self.rtt.as_secs_f32() * self.channel_pending.len() as f32)
    }

    fn channel_retry_timeout_for(rtt: Duration, tries: u8, outstanding: usize) -> Duration {
        let base = (rtt.as_secs_f32() * 2.5).max(0.025);
        let multiplier = 1.5_f32.powi(i32::from(tries.saturating_sub(1)));
        Duration::from_secs_f32(multiplier * base * (outstanding as f32 + 1.5))
    }

    fn channel_window_profile(rtt: Duration) -> (u8, u8, u8, u8) {
        if rtt.as_secs_f32() > CHANNEL_RTT_SLOW_SECS {
            (1, 1, 1, 1)
        } else {
            (
                CHANNEL_WINDOW_INIT,
                CHANNEL_WINDOW_MAX_SLOW,
                CHANNEL_WINDOW_MIN,
                CHANNEL_WINDOW_FLEXIBILITY,
            )
        }
    }

    fn reset_channel_flow_control(&mut self) {
        let (window, window_max, window_min, flexibility) = Self::channel_window_profile(self.rtt);
        self.channel_window = window;
        self.channel_window_max = window_max;
        self.channel_window_min = window_min;
        self.channel_window_flexibility = flexibility;
        self.channel_fast_rate_rounds = 0;
        self.channel_medium_rate_rounds = 0;
    }

    fn refresh_channel_flow_control(&mut self) {
        let (window, window_max, window_min, flexibility) = Self::channel_window_profile(self.rtt);
        self.channel_window_max = window_max;
        self.channel_window_min = window_min;
        self.channel_window_flexibility = flexibility;
        if self.channel_window < self.channel_window_min || self.channel_window == 0 {
            self.channel_window = self.channel_window_min.max(window);
        }
        if self.channel_window > self.channel_window_max {
            self.channel_window = self.channel_window_max;
        }
    }

    fn note_channel_delivery(&mut self) {
        if self.channel_window < self.channel_window_max {
            self.channel_window += 1;
        }

        if self.rtt.is_zero() {
            return;
        }

        if self.rtt.as_secs_f32() > CHANNEL_RTT_FAST_SECS {
            self.channel_fast_rate_rounds = 0;

            if self.rtt.as_secs_f32() > CHANNEL_RTT_MEDIUM_SECS {
                self.channel_medium_rate_rounds = 0;
            } else {
                self.channel_medium_rate_rounds = self.channel_medium_rate_rounds.saturating_add(1);
                if self.channel_window_max < CHANNEL_WINDOW_MAX_MEDIUM
                    && self.channel_medium_rate_rounds == CHANNEL_FAST_RATE_THRESHOLD
                {
                    self.channel_window_max = CHANNEL_WINDOW_MAX_MEDIUM;
                    self.channel_window_min = CHANNEL_WINDOW_MIN_LIMIT_MEDIUM;
                    if self.channel_window < self.channel_window_min {
                        self.channel_window = self.channel_window_min;
                    }
                }
            }
        } else {
            self.channel_fast_rate_rounds = self.channel_fast_rate_rounds.saturating_add(1);
            if self.channel_window_max < CHANNEL_WINDOW_MAX_FAST
                && self.channel_fast_rate_rounds == CHANNEL_FAST_RATE_THRESHOLD
            {
                self.channel_window_max = CHANNEL_WINDOW_MAX_FAST;
                self.channel_window_min = CHANNEL_WINDOW_MIN_LIMIT_FAST;
                if self.channel_window < self.channel_window_min {
                    self.channel_window = self.channel_window_min;
                }
            }
        }
    }

    fn note_channel_timeout(&mut self) {
        self.channel_fast_rate_rounds = 0;
        self.channel_medium_rate_rounds = 0;

        if self.channel_window > self.channel_window_min {
            self.channel_window -= 1;
        }
        if self.channel_window_max > self.channel_window_min + self.channel_window_flexibility {
            self.channel_window_max -= 1;
        }
        if self.channel_window > self.channel_window_max {
            self.channel_window = self.channel_window_max;
        }
    }

    fn packet_with_context(&self, data: &[u8], context: PacketContext) -> Result<Packet, RnsError> {
        if !self.status.can_exchange_data() {
            log::warn!("can't create data packet for closed link");
        }

        let mut packet_data = PacketDataBuffer::new();
        self.encrypt_packet_data_into(data, &mut packet_data)?;

        Ok(Packet {
            header: Header {
                destination_type: DestinationType::Link,
                packet_type: PacketType::Data,
                ..Default::default()
            },
            ifac: None,
            destination: self.id,
            transport: None,
            context,
            data: packet_data,
        })
    }

    pub fn data_packet_into(&self, data: &[u8], packet: &mut Packet) -> Result<(), RnsError> {
        if !self.status.can_exchange_data() {
            log::warn!("can't create data packet for closed link");
        }

        packet.header = Header {
            destination_type: DestinationType::Link,
            packet_type: PacketType::Data,
            ..Default::default()
        };
        packet.ifac = None;
        packet.destination = self.id;
        packet.transport = None;
        packet.context = PacketContext::None;
        self.encrypt_packet_data_into(data, &mut packet.data)
    }

    pub fn keep_alive_packet(&self, data: u8) -> Packet {
        log::trace!("link({}): create keep alive {}", self.id, data);

        let mut packet_data = PacketDataBuffer::new();
        packet_data.safe_write(&[data]);

        Packet {
            header: Header {
                destination_type: DestinationType::Link,
                packet_type: PacketType::Data,
                ..Default::default()
            },
            ifac: None,
            destination: self.id,
            transport: None,
            context: PacketContext::KeepAlive,
            data: packet_data,
        }
    }

    pub fn encrypt<'a>(&self, text: &[u8], out_buf: &'a mut [u8]) -> Result<&'a [u8], RnsError> {
        if let Some(session_cipher) = &self.session_cipher {
            let token = session_cipher.encrypt(OsRng, PlainText::from(text), out_buf)?;
            Ok(token.as_bytes())
        } else {
            self.priv_identity.encrypt(OsRng, text, &self.derived_key, out_buf)
        }
    }

    pub fn decrypt<'a>(&self, text: &[u8], out_buf: &'a mut [u8]) -> Result<&'a [u8], RnsError> {
        if let Some(session_cipher) = &self.session_cipher {
            let verified = session_cipher.verify(Token::from(text))?;
            let plain_text = session_cipher.decrypt(verified, out_buf)?;
            Ok(plain_text.as_bytes())
        } else {
            self.priv_identity.decrypt(OsRng, text, &self.derived_key, out_buf)
        }
    }

    pub fn destination(&self) -> &DestinationDesc {
        &self.destination
    }

    pub fn ingress_iface(&self) -> Option<AddressHash> {
        self.ingress_iface
    }

    pub fn set_ingress_iface(&mut self, iface: AddressHash) {
        self.ingress_iface = Some(iface);
    }

    pub fn peer_identity(&self) -> &Identity {
        &self.peer_identity
    }

    pub fn create_rtt(&self) -> Packet {
        let rtt = self.rtt.as_secs_f32();
        let mut buf = Vec::new();
        {
            buf.reserve(4);
            rmp::encode::write_f32(&mut buf, rtt).unwrap();
        }

        let mut packet_data = PacketDataBuffer::new();

        let token_len = {
            let token = self
                .encrypt(buf.as_slice(), packet_data.accuire_buf_max())
                .expect("encrypted data");
            token.len()
        };

        packet_data.resize(token_len);

        log::trace!("{} create rtt packet = {} sec", self.id, rtt);

        Packet {
            header: Header { destination_type: DestinationType::Link, ..Default::default() },
            ifac: None,
            destination: self.id,
            transport: None,
            context: PacketContext::LinkRTT,
            data: packet_data,
        }
    }

    fn refresh_local_identity(&mut self) {
        self.priv_identity = PrivateIdentity::new(
            StaticSecret::random_from_rng(OsRng),
            self.priv_identity.sign_key().clone(),
        );
    }

    fn handshake(&mut self, peer_identity: Identity) {
        log::debug!("link({}): handshake", self.id);

        self.status = LinkStatus::Handshake;
        self.peer_identity = peer_identity;

        self.derived_key =
            self.priv_identity.derive_key(&self.peer_identity.public_key, Some(self.id.as_slice()));
        let key_bytes = self.derived_key.as_bytes();
        let split = key_bytes.len() / 2;
        self.session_cipher =
            Some(CachedFernet::new_from_slices(&key_bytes[..split], &key_bytes[split..]));
    }

    fn note_inbound(&mut self, context: PacketContext) {
        let now = Instant::now();
        self.last_inbound = Some(now);
        if self.status == LinkStatus::Stale {
            self.status = LinkStatus::Active;
            self.stale_since = None;
        }
        if context != PacketContext::KeepAlive {
            self.last_data = Some(now);
            self.request_time = now;
        }
    }

    pub(crate) fn note_outbound(&mut self, context: PacketContext) {
        let now = Instant::now();
        self.last_outbound = Some(now);
        if context == PacketContext::KeepAlive {
            self.last_keepalive = Some(now);
        } else {
            self.last_data = Some(now);
        }
    }

    fn update_keepalive_timing(&mut self) {
        let keepalive_secs = (self.rtt.as_secs_f32() * (KEEPALIVE_MAX_SECS / KEEPALIVE_MAX_RTT))
            .clamp(KEEPALIVE_MIN_SECS, KEEPALIVE_MAX_SECS);
        self.keepalive = Duration::from_secs_f32(keepalive_secs);
        self.stale_time = Duration::from_secs_f32(keepalive_secs * STALE_FACTOR);
    }

    fn inbound_anchor(&self) -> Instant {
        [self.activated_at, self.last_proof, self.last_inbound]
            .into_iter()
            .flatten()
            .max()
            .unwrap_or(self.request_time)
    }

    fn activity_anchor(&self, last_activity: Option<Instant>) -> Instant {
        [self.activated_at, last_activity].into_iter().flatten().max().unwrap_or(self.request_time)
    }

    pub fn no_inbound_for(&self) -> Duration {
        Instant::now().duration_since(self.activity_anchor(self.last_inbound))
    }

    pub fn no_outbound_for(&self) -> Duration {
        Instant::now().duration_since(self.activity_anchor(self.last_outbound))
    }

    pub fn no_data_for(&self) -> Duration {
        Instant::now().duration_since(self.activity_anchor(self.last_data))
    }

    pub fn inactive_for(&self) -> Duration {
        self.no_inbound_for().min(self.no_outbound_for())
    }
}
