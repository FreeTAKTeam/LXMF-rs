impl Link {
    pub fn new_from_request(
        packet: &Packet,
        signing_key: SigningKey,
        destination: DestinationDesc,
        event_tx: tokio::sync::broadcast::Sender<LinkEventData>,
    ) -> Result<Self, RnsError> {
        if packet.data.len() < PUBLIC_KEY_LENGTH * 2 {
            return Err(RnsError::InvalidArgument);
        }

        let data = packet.data.as_slice();
        let peer_identity = Identity::try_new_from_slices(
            &data[..PUBLIC_KEY_LENGTH],
            &data[PUBLIC_KEY_LENGTH..PUBLIC_KEY_LENGTH * 2],
        )?;
        let signalling = if data.len() >= PUBLIC_KEY_LENGTH * 2 + LINK_MTU_SIZE {
            let mut bytes = [0u8; LINK_MTU_SIZE];
            bytes.copy_from_slice(
                &data[PUBLIC_KEY_LENGTH * 2..PUBLIC_KEY_LENGTH * 2 + LINK_MTU_SIZE],
            );
            Some(clamp_link_signalling(bytes))
        } else {
            None
        };

        let link_id = LinkId::from(packet);
        log::debug!("create from request {}", link_id);

        let mut link = Self {
            id: link_id,
            destination,
            ingress_iface: None,
            priv_identity: PrivateIdentity::new(StaticSecret::random_from_rng(OsRng), signing_key),
            peer_identity,
            identified_peer_identity: None,
            derived_key: DerivedKey::new_empty(),
            session_cipher: None,
            signalling,
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
        };

        link.handshake(peer_identity);

        Ok(link)
    }
}
