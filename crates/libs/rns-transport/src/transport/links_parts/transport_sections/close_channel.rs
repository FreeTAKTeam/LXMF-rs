impl Transport {

    pub async fn close_channel(
        &self,
        link_id: &AddressHash,
    ) -> Result<(), crate::channel::ChannelError> {
        let link =
            self.find_any_link(link_id).await.ok_or(crate::channel::ChannelError::LinkNotReady)?;
        link.lock().await.close_channel();
        Ok(())
    }

    pub async fn channel_message_state(
        &self,
        link_id: &AddressHash,
        sequence: u16,
    ) -> Result<crate::channel::MessageState, crate::channel::ChannelError> {
        let link =
            self.find_any_link(link_id).await.ok_or(crate::channel::ChannelError::LinkNotReady)?;
        let state = link.lock().await.channel_state(sequence);
        Ok(state)
    }

    pub async fn send_resource_direct(
        &self,
        link_id: &AddressHash,
        iface: AddressHash,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
    ) -> Result<Hash, RnsError> {
        let link = self.find_in_link(link_id).await.ok_or(RnsError::InvalidArgument)?;
        let interface_mtu = self.resource_mtu_for_iface(Some(iface)).await;
        // See `reset_out_link.rs::send_resource_observed`: the build is done
        // before the handler lock is taken, so a large payload does not hold
        // every other link and announce on this node behind it.
        let prepared = {
            let link_guard = link.lock().await;
            let interface_mtu = interface_mtu.min(link_guard.link_mtu());
            ResourceManager::prepare_send(&link_guard, data, metadata, None, false, interface_mtu)?
        };
        let (resource_hash, packet) =
            self.handler.lock().await.resource_manager.track_prepared(prepared);
        let dispatch = self
            .handler
            .lock()
            .await
            .send(TxMessage { tx_type: TxMessageType::Direct(iface), packet })
            .await;
        let sent = dispatch.sent_ifaces > 0;
        let mut handler = self.handler.lock().await;
        handler.resource_manager.confirm_outbound_dispatch(resource_hash, sent);
        let events = handler.resource_manager.drain_events();
        super::resource_wire::publish_resource_events(&handler, events);
        if sent {
            Ok(resource_hash)
        } else {
            Err(RnsError::ConnectionError)
        }
    }

    pub async fn find_out_link(&self, link_id: &AddressHash) -> Option<Arc<Mutex<Link>>> {
        let links = {
            let handler = self.handler.lock().await;
            handler.out_links.values().cloned().collect::<Vec<_>>()
        };
        for link in links {
            if *link.lock().await.id() == *link_id {
                return Some(link);
            }
        }
        None
    }

    pub async fn find_in_link(&self, link_id: &AddressHash) -> Option<Arc<Mutex<Link>>> {
        self.handler.lock().await.in_links.get(link_id).cloned()
    }

    pub async fn link(&self, destination: DestinationDesc) -> Arc<Mutex<Link>> {
        let link = self.handler.lock().await.out_links.get(&destination.address_hash).cloned();

        if let Some(link) = link {
            let status = link.lock().await.status();
            log::debug!(
                "[tp-diag] reuse_out_link destination={} status={:?}",
                destination.address_hash,
                status
            );
            if status != LinkStatus::Closed {
                return link;
            } else {
                log::warn!("tp({}): link was closed", self.name);
            }
        }

        let mut link = Link::new(destination, self.link_out_event_tx.clone());
        let (next_hop_iface, hops) = {
            let handler = self.handler.lock().await;
            (
                handler.path_table.next_hop_iface(&destination.address_hash),
                handler.path_table.hops_to(&destination.address_hash),
            )
        };
        let next_hop_mtu = match next_hop_iface {
            Some(iface) => self.iface_manager.lock().await.mtu(&iface),
            None => None,
        };
        link.set_establishment_timeout(
            self.establishment_timeout_for(&destination.address_hash, next_hop_mtu, hops).await,
        );
        let packet = match next_hop_mtu {
            Some(mtu) => link.request_with_mtu(mtu),
            None => link.request(),
        };

        log::debug!(
            "tp({}): create new link {} for destination {}",
            self.name,
            link.id(),
            destination
        );
        log::debug!(
            "[tp-diag] create_out_link destination={} link_id={}",
            destination.address_hash,
            link.id()
        );

        let link = Arc::new(Mutex::new(link));

        self.handler.lock().await.out_links.insert(destination.address_hash, link.clone());

        self.send_packet(packet).await;

        link
    }

    pub async fn request_path(
        &self,
        destination: &AddressHash,
        on_iface: Option<AddressHash>,
        tag: Option<TagBytes>,
    ) -> TxDispatchTrace {
        let packet = {
            let mut handler = self.handler.lock().await;
            handler.path_requests.generate(destination, tag)
        };
        log::debug!(
            "[tp-diag] path_request_broadcast dst={} on_iface={}",
            destination,
            on_iface.map(|iface| iface.to_string()).unwrap_or_else(|| "-".to_string())
        );
        let dispatch = if let Some(iface) = on_iface {
            self.iface_manager
                .lock()
                .await
                .send_path_request_on_iface(iface, packet)
                .await
        } else {
            self.iface_manager
                .lock()
                .await
                .send_path_request(TxMessage {
                    tx_type: TxMessageType::Broadcast(None),
                    packet,
                })
                .await
        };
        log::debug!(
            "[tp-diag] path_request_broadcast_done dst={} matched={} sent={} failed={}",
            destination,
            dispatch.matched_ifaces,
            dispatch.sent_ifaces,
            dispatch.failed_ifaces
        );
        if dispatch.sent_ifaces > 0 || dispatch.queued_ifaces > 0 {
            self.handler.lock().await.path_requests.record_outgoing_request(destination);
        }
        dispatch
    }

    pub fn out_link_events(&self) -> broadcast::Receiver<LinkEventData> {
        self.link_out_event_tx.subscribe()
    }

    pub fn in_link_events(&self) -> broadcast::Receiver<LinkEventData> {
        self.link_in_event_tx.subscribe()
    }

    pub fn received_data_events(&self) -> broadcast::Receiver<ReceivedData> {
        self.received_data_tx.subscribe()
    }

    pub async fn add_destination(
        &self,
        identity: PrivateIdentity,
        name: DestinationName,
    ) -> Arc<Mutex<SingleInputDestination>> {
        let destination = SingleInputDestination::new(identity, name);
        self.register_destination(destination).await
    }

    pub async fn register_destination(
        &self,
        destination: SingleInputDestination,
    ) -> Arc<Mutex<SingleInputDestination>> {
        let address_hash = destination.desc.address_hash;

        log::debug!("tp({}): add destination {}", self.name, address_hash);

        let destination = Arc::new(Mutex::new(destination));

        self.handler.lock().await.single_in_destinations.insert(address_hash, destination.clone());

        destination
    }

    pub async fn deregister_destination(&self, address: &AddressHash) -> bool {
        let mut handler = self.handler.lock().await;
        let removed_input = handler.single_in_destinations.remove(address).is_some();
        handler.single_in_destination_app_data.remove(address);
        let removed_output = handler.single_out_destinations.remove(address).is_some();
        removed_input || removed_output
    }

    pub async fn find_interface_from_hash(&self, full_hash: &Hash) -> Option<AddressHash> {
        self.iface_manager.lock().await.address_for_full_hash(full_hash)
    }

    pub async fn detach_interfaces(&self) -> usize {
        self.iface_manager.lock().await.detach_interfaces()
    }

    pub async fn has_destination(&self, address: &AddressHash) -> bool {
        self.handler.lock().await.has_destination(address)
    }

    pub async fn knows_destination(&self, address: &AddressHash) -> bool {
        self.handler.lock().await.knows_destination(address)
    }

    pub async fn has_path(&self, address: &AddressHash) -> bool {
        self.handler.lock().await.path_table.get(address).is_some()
    }

    pub async fn await_path(
        &self,
        destination: &AddressHash,
        timeout: Duration,
        on_iface: Option<AddressHash>,
    ) -> bool {
        if self.has_path(destination).await {
            return true;
        }
        self.request_path(destination, on_iface, None).await;
        let deadline = tokio::time::Instant::now() + timeout;
        while tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(50)).await;
            if self.has_path(destination).await {
                return true;
            }
        }
        self.has_path(destination).await
    }

    pub async fn hops_to(&self, destination: &AddressHash) -> u8 {
        self.handler.lock().await.path_table.hops_to(destination)
    }

    pub async fn path_is_unresponsive(&self, destination: &AddressHash) -> bool {
        self.handler.lock().await.path_table.path_is_unresponsive(destination)
    }

    pub async fn mark_path_responsive(&self, destination: &AddressHash) -> bool {
        self.handler.lock().await.path_table.mark_path_responsive(destination)
    }

    pub async fn mark_path_unknown(&self, destination: &AddressHash) -> bool {
        self.handler.lock().await.path_table.mark_path_unknown(destination)
    }

    pub async fn next_hop_metrics(
        &self,
        destination: &AddressHash,
    ) -> Option<NextHopMetrics> {
        let interface = self.handler.lock().await.path_table.next_hop_iface(destination)?;
        let manager = self.iface_manager.lock().await;
        let bitrate = manager.announce_pacing(&interface)?.0;
        let hardware_mtu = manager.mtu(&interface);
        Some(NextHopMetrics {
            interface,
            bitrate,
            hardware_mtu,
            per_bit_latency: (bitrate > 0).then(|| 1.0 / bitrate as f64),
        })
    }

    pub async fn link_count(&self) -> usize {
        self.handler.lock().await.link_table.len()
    }

    pub async fn active_link_count(&self) -> usize {
        self.handler.lock().await.link_table.active_len()
    }

    pub async fn inbound_queue_snapshot(&self) -> InboundQueueSnapshot {
        self.handler.lock().await.inbound_queues.snapshot()
    }

    pub async fn interface_traffic_snapshots(
        &self,
    ) -> Vec<crate::iface::InterfaceTrafficSnapshot> {
        let announce_burst_ifaces = self.handler.lock().await.announce_limits.active_interfaces();
        let mut snapshots = self.iface_manager.lock().await.traffic_snapshots();
        for snapshot in &mut snapshots {
            snapshot.announce_burst_active = announce_burst_ifaces.contains(&snapshot.address);
        }
        let parents = snapshots
            .iter()
            .filter_map(|snapshot| snapshot.parent)
            .collect::<alloc::collections::BTreeSet<_>>();
        for parent in parents {
            let children = snapshots
                .iter()
                .filter(|snapshot| snapshot.parent == Some(parent))
                .cloned()
                .collect::<Vec<_>>();
            let announce_count =
                children.iter().filter(|snapshot| snapshot.announce_burst_active).count() as u64;
            let path_request_count = children
                .iter()
                .filter(|snapshot| snapshot.path_request_burst_active)
                .count() as u64;
            if let Some(parent_snapshot) =
                snapshots.iter_mut().find(|snapshot| snapshot.address == parent)
            {
                for child in &children {
                    parent_snapshot.aggregate_child(child);
                }
                parent_snapshot.ic_burst_count = Some(announce_count);
                parent_snapshot.ic_pr_burst_count = Some(path_request_count);
            }
        }
        snapshots
    }

    pub const fn default_data_queue_length() -> usize {
        DEFAULT_DATA_QUEUE_LENGTH
    }

    pub const fn default_announce_queue_length() -> usize {
        DEFAULT_ANNOUNCE_QUEUE_LENGTH
    }

    pub const fn default_path_request_queue_length() -> usize {
        DEFAULT_PATH_REQUEST_QUEUE_LENGTH
    }

    pub const fn default_ingress_limited_queue_length() -> usize {
        DEFAULT_INGRESS_LIMITED_QUEUE_LENGTH
    }

    pub async fn lowest_interface_bitrate(&self) -> Option<u64> {
        self.iface_manager.lock().await.lowest_interface_bitrate()
    }

    /// RNS 1.5 medium timeout: one MTU round trip at the slowest bitrate,
    /// clamped to the five-bit/s protocol minimum, plus six seconds grace.
    pub async fn medium_path_timeout(&self) -> Duration {
        medium_path_timeout_for_bitrate(self.lowest_interface_bitrate().await)
    }

    pub async fn path_status(&self, address: &AddressHash) -> crate::transport::TransportPathStatus {
        let handler = self.handler.lock().await;
        if let Some(entry) = handler.path_table.get(address) {
            crate::transport::TransportPathStatus {
                destination: *address,
                path_found: true,
                next_hop: Some(entry.received_from),
                interface: Some(entry.iface),
                hops: Some(entry.hops),
            }
        } else {
            crate::transport::TransportPathStatus {
                destination: *address,
                path_found: false,
                next_hop: None,
                interface: None,
                hops: None,
            }
        }
    }

    pub async fn expire_path(&self, destination: &AddressHash) -> bool {
        self.handler.lock().await.path_table.expire_path(destination)
    }

    pub async fn expire_paths_via(&self, transport: &AddressHash) -> usize {
        self.handler.lock().await.path_table.expire_paths_via(transport)
    }

    pub async fn drop_announce_queues(&self) -> usize {
        self.iface_manager.lock().await.drop_announce_queues()
    }

    pub async fn announce_rate_table(&self) -> Vec<crate::transport::AnnounceRateTableEntry> {
        self.handler.lock().await.announce_limits.rate_table()
    }

    /// Records per-packet radio metadata for local-client management queries.
    /// Entries use Python Reticulum's 512-item FIFO retention limit.
    pub async fn record_packet_signal(&self, packet_hash: Hash, signal: PacketSignal) {
        let mut handler = self.handler.lock().await;
        handler.packet_signal_cache.push_back((packet_hash, signal));
        while handler.packet_signal_cache.len() > 512 {
            handler.packet_signal_cache.pop_front();
        }
    }

    pub async fn packet_signal(&self, packet_hash: &Hash) -> Option<PacketSignal> {
        self.handler
            .lock()
            .await
            .packet_signal_cache
            .iter()
            .rev()
            .find(|(hash, _)| hash == packet_hash)
            .map(|(_, signal)| *signal)
    }

    pub async fn expire_paths_for_identity(&self, identity: &AddressHash) -> usize {
        let destinations = {
            let handler = self.handler.lock().await;
            handler
                .single_out_destinations
                .iter()
                .map(|(destination, record)| (*destination, record.clone()))
                .collect::<Vec<_>>()
        };
        let mut matching_destinations = Vec::new();
        for (destination, record) in destinations {
            if record.lock().await.identity.address_hash == *identity {
                matching_destinations.push(destination);
            }
        }
        let mut handler = self.handler.lock().await;
        matching_destinations
            .into_iter()
            .filter(|destination| handler.path_table.expire_path(destination))
            .count()
    }

    /// Updates the transport-level announce filter for an identity.
    ///
    /// Returns the number of currently learned paths removed when the identity
    /// becomes blackholed. This mirrors RNS 1.5's signalled blackhole result at
    /// the Rust transport boundary without coupling core signature validation
    /// to daemon-owned policy state.
    pub async fn set_identity_blackholed(&self, identity: AddressHash, blackholed: bool) -> usize {
        self.set_identity_blackholed_until(identity, blackholed, None).await
    }

    pub async fn set_identity_blackholed_until(
        &self,
        identity: AddressHash,
        blackholed: bool,
        until: Option<f64>,
    ) -> usize {
        {
            let mut handler = self.handler.lock().await;
            if blackholed {
                handler.blackholed_identities.insert(identity, until);
            } else {
                handler.blackholed_identities.remove(&identity);
                return 0;
            }
        }
        self.expire_paths_for_identity(&identity).await
    }

    pub async fn is_identity_blackholed(&self, identity: &AddressHash) -> bool {
        self.handler.lock().await.is_identity_blackholed(identity)
    }

    pub async fn destination_identity(&self, address: &AddressHash) -> Option<Identity> {
        let destination =
            { self.handler.lock().await.single_out_destinations.get(address).cloned() }?;
        let destination = destination.lock().await;
        Some(destination.identity)
    }

    #[cfg(test)]
    pub(crate) fn get_handler(&self) -> Arc<Mutex<TransportHandler>> {
        // direct access to handler for testing purposes
        self.handler.clone()
    }
}
