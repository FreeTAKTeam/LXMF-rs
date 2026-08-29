impl InterfaceManager {
    pub fn new(rx_cap: usize) -> Self {
        let (rx_send, rx_recv) = InterfaceChannel::make_rx_channel(rx_cap);
        let rx_recv = Arc::new(tokio::sync::Mutex::new(rx_recv));

        Self { counter: 0, rx_recv, rx_send, ifaces: Vec::new() }
    }

    pub fn new_channel(&mut self, tx_cap: usize) -> InterfaceChannel {
        self.new_channel_with_role(tx_cap, IfaceRole::default())
    }

    pub fn new_channel_with_role(&mut self, tx_cap: usize, role: IfaceRole) -> InterfaceChannel {
        self.new_channel_with_role_and_mode(tx_cap, role, InterfaceMode::default())
    }

    pub fn new_channel_with_role_and_mode(
        &mut self,
        tx_cap: usize,
        role: IfaceRole,
        mode: InterfaceMode,
    ) -> InterfaceChannel {
        self.new_channel_with_role_mode_mtu(tx_cap, role, mode, DEFAULT_IFACE_MTU)
    }

    pub fn new_channel_with_role_mode_mtu(
        &mut self,
        tx_cap: usize,
        role: IfaceRole,
        mode: InterfaceMode,
        mtu: usize,
    ) -> InterfaceChannel {
        self.counter += 1;

        let counter_bytes = self.counter.to_le_bytes();
        let full_hash = Hash::new_from_slice(&counter_bytes[..]);
        let address = AddressHash::new_from_hash(&full_hash);

        let (tx_send, tx_recv) = InterfaceChannel::make_tx_channel(tx_cap);

        log::debug!("create channel {} role={:?} mode={:?}", address, role, mode);

        let stop = CancellationToken::new();
        let online = Arc::new(AtomicBool::new(true));

        self.ifaces.push(LocalInterface {
            address,
            parent: None,
            full_hash,
            tx_send,
            stop: stop.clone(),
            online: online.clone(),
            mtu,
            role,
            mode,
            gravity: 0,
            outgoing: true,
            announce_queue: VecDeque::new(),
            announce_allowed_at: Instant::now(),
            announce_bitrate_bps: DEFAULT_IFACE_BITRATE_BPS,
            announce_cap_percent: DEFAULT_ANNOUNCE_CAP_PERCENT,
            shared_config: InterfaceSharedConfig::default(),
            is_shared_instance: false,
            outgoing_pr_history: VecDeque::new(),
            traffic: InterfaceTraffic::default(),
        });

        InterfaceChannel {
            rx_channel: self.rx_send.clone(),
            tx_channel: tx_recv,
            address,
            stop,
            online,
        }
    }

    pub fn new_context<T: Interface>(&mut self, inner: T) -> InterfaceContext<T> {
        self.new_context_with_role(inner, IfaceRole::default())
    }

    pub fn new_context_with_role<T: Interface>(
        &mut self,
        inner: T,
        role: IfaceRole,
    ) -> InterfaceContext<T> {
        self.new_context_with_role_and_mode(inner, role, InterfaceMode::default())
    }

    pub fn new_context_with_role_and_mode<T: Interface>(
        &mut self,
        inner: T,
        role: IfaceRole,
        mode: InterfaceMode,
    ) -> InterfaceContext<T> {
        let mtu = inner.configured_mtu();
        let channel =
            self.new_channel_with_role_mode_mtu(DEFAULT_IFACE_TX_QUEUE_CAPACITY, role, mode, mtu);
        let cancel = channel.stop.clone();
        let inner = Arc::new(Mutex::new(inner));
        InterfaceContext::<T> { inner: inner.clone(), channel, cancel }
    }

    pub fn spawn<T: Interface, F, R>(&mut self, inner: T, worker: F) -> AddressHash
    where
        F: FnOnce(InterfaceContext<T>) -> R,
        R: std::future::Future<Output = ()> + Send + 'static,
        R::Output: Send + 'static,
    {
        self.spawn_as(inner, worker, IfaceRole::default())
    }

    pub fn spawn_as<T: Interface, F, R>(
        &mut self,
        inner: T,
        worker: F,
        role: IfaceRole,
    ) -> AddressHash
    where
        F: FnOnce(InterfaceContext<T>) -> R,
        R: std::future::Future<Output = ()> + Send + 'static,
        R::Output: Send + 'static,
    {
        self.spawn_as_with_mode(inner, worker, role, InterfaceMode::default())
    }

    pub fn spawn_as_with_mode<T: Interface, F, R>(
        &mut self,
        inner: T,
        worker: F,
        role: IfaceRole,
        mode: InterfaceMode,
    ) -> AddressHash
    where
        F: FnOnce(InterfaceContext<T>) -> R,
        R: std::future::Future<Output = ()> + Send + 'static,
        R::Output: Send + 'static,
    {
        let context = self.new_context_with_role_and_mode(inner, role, mode);
        let address = *context.channel.address();

        task::spawn(worker(context));

        address
    }

    pub fn spawn_as_with_mode_and_handle<T: Interface, F, R>(
        &mut self,
        inner: T,
        worker: F,
        role: IfaceRole,
        mode: InterfaceMode,
    ) -> (AddressHash, Arc<Mutex<T>>)
    where
        F: FnOnce(InterfaceContext<T>) -> R,
        R: std::future::Future<Output = ()> + Send + 'static,
        R::Output: Send + 'static,
    {
        let context = self.new_context_with_role_and_mode(inner, role, mode);
        let address = *context.channel.address();
        let handle = context.inner.clone();

        task::spawn(worker(context));

        (address, handle)
    }

    pub fn role(&self, address: &AddressHash) -> Option<IfaceRole> {
        self.ifaces.iter().find(|i| i.address == *address).map(|i| i.role)
    }

    pub fn mode(&self, address: &AddressHash) -> Option<InterfaceMode> {
        self.ifaces.iter().find(|i| i.address == *address).map(|i| i.mode)
    }

    pub fn outgoing(&self, address: &AddressHash) -> Option<bool> {
        self.ifaces.iter().find(|i| i.address == *address).map(|i| i.outgoing)
    }

    pub fn mtu(&self, address: &AddressHash) -> Option<usize> {
        self.ifaces.iter().find(|i| i.address == *address).map(|i| i.mtu)
    }

    pub fn announce_pacing(&self, address: &AddressHash) -> Option<(u64, u64)> {
        self.ifaces
            .iter()
            .find(|i| i.address == *address)
            .map(|i| (i.announce_bitrate_bps, i.announce_cap_percent))
    }

    pub fn shared_config(&self, address: &AddressHash) -> Option<&InterfaceSharedConfig> {
        self.ifaces.iter().find(|i| i.address == *address).map(|i| &i.shared_config)
    }

    pub fn full_hash(&self, address: &AddressHash) -> Option<Hash> {
        self.ifaces.iter().find(|i| i.address == *address).map(|i| i.full_hash)
    }

    pub fn address_for_full_hash(&self, full_hash: &Hash) -> Option<AddressHash> {
        self.ifaces.iter().find(|i| i.full_hash == *full_hash).map(|i| i.address)
    }

    pub fn set_mode(&mut self, address: AddressHash, mode: InterfaceMode) -> bool {
        if let Some(iface) = self.ifaces.iter_mut().find(|i| i.address == address) {
            iface.mode = mode;
            true
        } else {
            false
        }
    }

    pub fn set_outgoing(&mut self, address: AddressHash, outgoing: bool) -> bool {
        if let Some(iface) = self.ifaces.iter_mut().find(|i| i.address == address) {
            iface.outgoing = outgoing;
            true
        } else {
            false
        }
    }

    pub fn set_announce_pacing(
        &mut self,
        address: AddressHash,
        bitrate_bps: u64,
        cap_percent: u64,
    ) -> bool {
        if let Some(iface) = self.ifaces.iter_mut().find(|i| i.address == address) {
            iface.announce_bitrate_bps = bitrate_bps;
            iface.announce_cap_percent = cap_percent;
            true
        } else {
            false
        }
    }

    pub fn inherit_runtime_config(
        &mut self,
        source: AddressHash,
        target: AddressHash,
    ) -> bool {
        let Some(source_iface) = self.ifaces.iter().find(|i| i.address == source) else {
            return false;
        };
        let mode = source_iface.mode;
        let gravity = source_iface.gravity;
        let outgoing = source_iface.outgoing;
        let announce_bitrate_bps = source_iface.announce_bitrate_bps;
        let announce_cap_percent = source_iface.announce_cap_percent;
        let shared_config = source_iface.shared_config.clone();
        let is_shared_instance = source_iface.is_shared_instance;

        let Some(target_iface) = self.ifaces.iter_mut().find(|i| i.address == target) else {
            return false;
        };
        target_iface.mode = mode;
        target_iface.parent = Some(source);
        target_iface.gravity = gravity;
        target_iface.outgoing = outgoing;
        target_iface.announce_bitrate_bps = announce_bitrate_bps;
        target_iface.announce_cap_percent = announce_cap_percent;
        target_iface.shared_config = shared_config;
        target_iface.is_shared_instance = is_shared_instance;
        true
    }

    /// Register a virtual iface that shares its tx channel with an
    /// existing host iface. Used by the UDP multicast iface to pin
    /// per-peer point-to-point routes without spawning additional
    /// sockets. Returns `None` if `host` is not registered.
    ///
    /// The returned `AddressHash` is used by the transport's routing
    /// tables (path_table, link ingress_iface) so `Direct` tx
    /// targeting this address is delivered into the host iface's tx
    /// task, which then sends the packet unicast to the pinned peer
    /// via its own socket.
    pub fn register_virtual_iface(
        &mut self,
        host: AddressHash,
        role: IfaceRole,
    ) -> Option<AddressHash> {
        let host_iface = self.ifaces.iter().find(|i| i.address == host)?;
        let host_tx = host_iface.tx_send.clone();
        let host_online = host_iface.online.clone();
        let mtu = host_iface.mtu;
        let mode = host_iface.mode;
        let gravity = host_iface.gravity;

        // Virtual iface gets its own CancellationToken so it can be
        // stopped (and GC'd by `cleanup()`) independently of the host.
        // A cloned token would share cancel state with the host,
        // meaning stopping one would tear down the other.
        let stop = CancellationToken::new();

        self.counter += 1;
        let counter_bytes = self.counter.to_le_bytes();
        let full_hash = Hash::new_from_slice(&counter_bytes[..]);
        let address = AddressHash::new_from_hash(&full_hash);

        log::debug!(
            "register virtual iface {} on host {} role={:?} mode={:?}",
            address,
            host,
            role,
            mode
        );

        self.ifaces.push(LocalInterface {
            address,
            parent: Some(host),
            full_hash,
            tx_send: host_tx,
            stop,
            online: host_online,
            mtu,
            role,
            mode,
            gravity,
            outgoing: host_iface.outgoing,
            announce_queue: VecDeque::new(),
            announce_allowed_at: Instant::now(),
            announce_bitrate_bps: host_iface.announce_bitrate_bps,
            announce_cap_percent: host_iface.announce_cap_percent,
            shared_config: host_iface.shared_config.clone(),
            is_shared_instance: host_iface.is_shared_instance,
            outgoing_pr_history: VecDeque::new(),
            traffic: InterfaceTraffic::default(),
        });

        Some(address)
    }

    /// Test-only: returns the number of tracked ifaces (live or stopped).
    #[cfg(test)]
    pub fn iface_count(&self) -> usize {
        self.ifaces.len()
    }

    /// Test-only: returns true if any tracked iface has the given role.
    #[cfg(test)]
    pub fn has_role(&self, role: IfaceRole) -> bool {
        self.ifaces.iter().any(|i| i.role == role)
    }

    fn queue_announce(iface: &mut LocalInterface, message: TxMessage, now: Instant) -> bool {
        iface
            .announce_queue
            .retain(|entry| now.duration_since(entry.queued_at) <= QUEUED_ANNOUNCE_LIFE);

        let emitted = announce_emitted(&message.packet);
        if let Some(existing) = iface
            .announce_queue
            .iter_mut()
            .find(|entry| entry.message.packet.destination == message.packet.destination)
        {
            if emitted > existing.emitted {
                existing.message = message;
                existing.queued_at = now;
                existing.emitted = emitted;
            }
            return true;
        }

        if iface.announce_queue.len() >= MAX_QUEUED_ANNOUNCES_PER_IFACE {
            log::warn!(
                "dropping announce for {} on {} because announce queue is full",
                message.packet.destination,
                iface.address
            );
            return false;
        }

        iface.announce_queue.push_back(QueuedAnnounce { message, queued_at: now, emitted });
        true
    }

    fn pop_next_announce(iface: &mut LocalInterface, now: Instant) -> Option<TxMessage> {
        iface
            .announce_queue
            .retain(|entry| now.duration_since(entry.queued_at) <= QUEUED_ANNOUNCE_LIFE);
        let index = iface
            .announce_queue
            .iter()
            .enumerate()
            .min_by_key(|(_, entry)| (entry.message.packet.header.hops, entry.queued_at))
            .map(|(index, _)| index)?;
        iface.announce_queue.remove(index).map(|entry| entry.message)
    }

    fn outgoing_pr_frequency(iface: &mut LocalInterface, now: Instant) -> f64 {
        while let Some(oldest) = iface.outgoing_pr_history.front().copied() {
            if now.duration_since(oldest) > OUTGOING_PR_FREQ_DECAY {
                iface.outgoing_pr_history.pop_front();
            } else {
                break;
            }
        }

        let Some(oldest) = iface.outgoing_pr_history.front().copied() else {
            return 0.0;
        };
        let span = now.duration_since(oldest);
        if span.is_zero() {
            0.0
        } else {
            iface.outgoing_pr_history.len() as f64 / span.as_secs_f64()
        }
    }

    fn should_egress_limit_pr(iface: &mut LocalInterface, now: Instant) -> bool {
        // RNS 1.5.2 deliberately leaves the local shared-instance carrier
        // outside Backbone ingress/egress dataplane control. The shared
        // instance already owns admission and pacing for that socket; adding
        // a second limiter here can unnecessarily throttle local clients.
        if iface.is_shared_instance || iface.shared_config.egress_control != Some(true) {
            return false;
        }
        let threshold = iface
            .shared_config
            .ec_pr_freq
            .filter(|value| value.is_finite() && *value >= 0.0)
            .unwrap_or(DEFAULT_EGRESS_PR_FREQ_HZ);
        Self::outgoing_pr_frequency(iface, now) > threshold
            && iface.outgoing_pr_history.len() >= OUTGOING_PR_MIN_LIMIT_SAMPLES
    }

    fn record_outgoing_pr(iface: &mut LocalInterface, now: Instant) {
        iface.outgoing_pr_history.push_back(now);
        while iface.outgoing_pr_history.len() > OUTGOING_PR_FREQ_SAMPLES {
            iface.outgoing_pr_history.pop_front();
        }
    }

    pub async fn release_queued_announces(&mut self) -> TxDispatchTrace {
        self.cleanup();
        let mut trace = TxDispatchTrace::default();
        let now = Instant::now();
        let mut saw_closed_queue = false;

        for iface in &mut self.ifaces {
            if iface.stop.is_cancelled()
                || !iface.outgoing
                || iface.is_shared_instance
                || iface.announce_queue.is_empty()
                || now < iface.announce_allowed_at
            {
                continue;
            }

            let Some(message) = Self::pop_next_announce(iface, now) else {
                continue;
            };
            let wire_len = packet_wire_len_for_dispatch(&message);

            trace.matched_ifaces += 1;
            iface.announce_allowed_at = now
                + announce_wait(
                    &message.packet,
                    iface.announce_bitrate_bps,
                    iface.announce_cap_percent,
                );
            match Self::send_to_iface(iface, message).await {
                TxIfaceSendResult::Sent => {
                    trace.sent_ifaces += 1;
                    if let Some(wire_len) = wire_len {
                        Self::record_outbound_traffic(
                            iface,
                            PacketType::Announce,
                            false,
                            wire_len,
                            now,
                        );
                    }
                }
                TxIfaceSendResult::Failed => trace.failed_ifaces += 1,
                TxIfaceSendResult::Closed => {
                    trace.failed_ifaces += 1;
                    saw_closed_queue = true;
                }
            }
        }

        if saw_closed_queue {
            self.cleanup_closed_tx_queues();
        }
        self.cleanup();
        trace
    }
}
