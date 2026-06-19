impl Link {

    pub fn next_watchdog_deadline(&self, initiator: bool) -> Option<Instant> {
        match self.status {
            LinkStatus::Active => {
                let inbound_anchor = self.inbound_anchor();
                let keepalive_due_at = inbound_anchor + self.keepalive;
                if keepalive_due_at > Instant::now() {
                    return Some(keepalive_due_at);
                }

                let stale_due_at = inbound_anchor + self.stale_time;
                if !initiator {
                    return Some(stale_due_at);
                }

                let keepalive_anchor = self.last_keepalive.unwrap_or(inbound_anchor);
                Some((keepalive_anchor + self.keepalive).min(stale_due_at))
            }
            LinkStatus::Stale => self.stale_since.map(|stale_since| {
                stale_since
                    + Duration::from_secs_f32(
                        (self.rtt.as_secs_f32() * KEEPALIVE_TIMEOUT_FACTOR) + STALE_GRACE_SECS,
                    )
            }),
            _ => None,
        }
    }

    pub fn check_watchdog(&mut self, initiator: bool) -> LinkWatchdogAction {
        let now = Instant::now();
        match self.status {
            LinkStatus::Active => {
                let inbound_anchor = self.inbound_anchor();
                let keepalive_due = now.duration_since(inbound_anchor) >= self.keepalive;
                if keepalive_due {
                    if now.duration_since(inbound_anchor) >= self.stale_time {
                        self.status = LinkStatus::Stale;
                        self.stale_since = Some(now);
                    }

                    if initiator {
                        let keepalive_anchor = self.last_keepalive.unwrap_or(inbound_anchor);
                        if now.duration_since(keepalive_anchor) >= self.keepalive {
                            return LinkWatchdogAction::SendKeepAlive;
                        }
                    }
                }
                LinkWatchdogAction::None
            }
            LinkStatus::Stale => {
                let stale_timeout = Duration::from_secs_f32(
                    (self.rtt.as_secs_f32() * KEEPALIVE_TIMEOUT_FACTOR) + STALE_GRACE_SECS,
                );
                if let Some(stale_since) = self.stale_since {
                    if now.duration_since(stale_since) >= stale_timeout {
                        if let Some(packet) = self.teardown() {
                            return LinkWatchdogAction::SendTeardown(packet);
                        }
                    }
                }
                LinkWatchdogAction::None
            }
            _ => LinkWatchdogAction::None,
        }
    }

    fn encrypt_packet_data_into(
        &self,
        data: &[u8],
        packet_data: &mut PacketDataBuffer,
    ) -> Result<(), RnsError> {
        packet_data.reset();
        let cipher_text_len = {
            let cipher_text = self.encrypt(data, packet_data.accuire_buf_max())?;
            cipher_text.len()
        };
        if cipher_text_len > crate::packet::PACKET_MDU {
            return Err(RnsError::OutOfMemory);
        }
        packet_data.resize(cipher_text_len);
        Ok(())
    }

    fn post_event(&self, event: LinkEvent) {
        let _ = self.event_tx.send(LinkEventData {
            id: self.id,
            address_hash: self.destination.address_hash,
            event,
        });
    }

    fn finalize_local_close(&mut self) {
        for pending in self.channel_pending.drain().map(|(_, pending)| pending) {
            self.channel_states.insert(pending.sequence, ChannelMessageState::Failed);
        }
        self.channel_rx_ring.clear();
        self.channel_open = false;
        self.status = LinkStatus::Closed;
        self.peer_identity = Identity::default();
        self.derived_key = DerivedKey::new_empty();
        self.session_cipher = None;
        self.last_keepalive = None;
        self.last_proof = None;
        self.stale_since = None;
        self.next_channel_sequence = 0;
        self.next_channel_rx_sequence = 0;
        self.reset_channel_flow_control();

        self.post_event(LinkEvent::Closed);

        println!("{}", link_close_line(&self.id));
        log::warn!("close {}", self.id);
    }

    fn teardown_packet(&self) -> Result<Packet, RnsError> {
        self.packet_with_context(self.id.as_slice(), PacketContext::LinkClose)
    }

    pub fn teardown(&mut self) -> Option<Packet> {
        let packet =
            if self.status.can_send_teardown() { self.teardown_packet().ok() } else { None };
        if packet.is_some() {
            self.note_outbound(PacketContext::LinkClose);
        }
        self.finalize_local_close();
        packet
    }

    pub fn close(&mut self) {
        self.finalize_local_close();
    }

    pub fn restart(&mut self) {
        log::warn!("link({}): restart after {}s", self.id, self.request_time.elapsed().as_secs());

        for pending in self.channel_pending.drain().map(|(_, pending)| pending) {
            self.channel_states.insert(pending.sequence, ChannelMessageState::Failed);
        }
        self.channel_rx_ring.clear();
        self.status = LinkStatus::Pending;
        self.peer_identity = Identity::default();
        self.derived_key = DerivedKey::new_empty();
        self.session_cipher = None;
        self.activated_at = None;
        self.ingress_iface = None;
        self.last_inbound = None;
        self.last_outbound = None;
        self.last_data = None;
        self.last_keepalive = None;
        self.last_proof = None;
        self.stale_since = None;
        self.next_channel_rx_sequence = 0;
        self.keepalive = Duration::from_secs_f32(KEEPALIVE_MAX_SECS);
        self.stale_time = Duration::from_secs_f32(KEEPALIVE_MAX_SECS * STALE_FACTOR);
        self.refresh_local_identity();
        self.reset_channel_flow_control();
    }

    pub fn elapsed(&self) -> Duration {
        self.request_time.elapsed()
    }

    pub fn status(&self) -> LinkStatus {
        self.status
    }

    pub fn id(&self) -> &LinkId {
        &self.id
    }

    pub(crate) fn validate_packet_proof(&self, packet: &Packet) -> Result<Hash, RnsError> {
        validate_link_packet_proof(&self.peer_identity, &self.id, packet)
    }

    fn channel_is_open(&self) -> bool {
        self.channel_open || !self.channel_handlers.is_empty()
    }

    fn handle_channel_frame(&mut self, plain_text: &[u8]) -> bool {
        if !self.channel_is_open() {
            return false;
        }

        let Ok(envelope) = ChannelEnvelope::unpack(plain_text) else {
            log::warn!("link({}): invalid channel frame", self.id);
            return false;
        };

        let distance = envelope.sequence.wrapping_sub(self.next_channel_rx_sequence);
        if distance >= 0x8000 {
            log::debug!("link({}): duplicate/old channel frame seq={}", self.id, envelope.sequence);
            return false;
        }
        if distance >= CHANNEL_RX_WINDOW_MAX {
            log::debug!(
                "link({}): channel frame outside receive window seq={} next={}",
                self.id,
                envelope.sequence,
                self.next_channel_rx_sequence
            );
            return false;
        }
        if self.channel_rx_ring.insert(envelope.sequence, envelope).is_some() {
            log::debug!(
                "link({}): duplicate buffered channel frame seq={}",
                self.id,
                self.next_channel_rx_sequence
            );
            return false;
        }

        let mut ready = VecDeque::new();
        while let Some(envelope) = self.channel_rx_ring.remove(&self.next_channel_rx_sequence) {
            ready.push_back(envelope);
            self.next_channel_rx_sequence = self.next_channel_rx_sequence.wrapping_add(1);
        }

        for envelope in ready {
            let Some(handlers) = self.channel_handlers.get_mut(&envelope.msg_type) else {
                log::debug!(
                    "link({}): channel frame without handler type={}",
                    self.id,
                    envelope.msg_type
                );
                continue;
            };
            for registered in handlers {
                match catch_unwind(AssertUnwindSafe(|| (registered.handler)(envelope.clone()))) {
                    Ok(true) => break,
                    Ok(false) => {}
                    Err(_) => log::error!("link({}): channel handler panicked", self.id),
                }
            }
        }

        true
    }
}
