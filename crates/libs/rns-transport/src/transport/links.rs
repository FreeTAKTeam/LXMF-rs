use super::*;

pub(super) const MAX_RESOURCE_PREPARE_WORKERS: usize = 4;

static RESOURCE_PREPARE_PERMITS: std::sync::OnceLock<Arc<tokio::sync::Semaphore>> =
    std::sync::OnceLock::new();

pub(super) fn resource_prepare_permits() -> Arc<tokio::sync::Semaphore> {
    RESOURCE_PREPARE_PERMITS
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(MAX_RESOURCE_PREPARE_WORKERS)))
        .clone()
}

fn collect_ready_link_packets<F>(
    links: Vec<Arc<Mutex<Link>>>,
    destination: Option<&AddressHash>,
    payload: &[u8],
    packet_builder: F,
) -> Vec<Packet>
where
    F: Fn(&Link, &[u8]) -> Result<Packet, RnsError>,
{
    let mut packets = Vec::new();
    for link in links {
        let Ok(link) = link.try_lock() else {
            log::debug!("tp: skipping busy link during public link fanout");
            continue;
        };
        if destination.is_some_and(|destination| link.destination().address_hash != *destination) {
            continue;
        }
        if link.status() == LinkStatus::Active {
            if let Ok(packet) = packet_builder(&link, payload) {
                packets.push(packet);
            }
        }
    }
    packets
}

fn find_ready_out_link_candidate(
    links: Vec<Arc<Mutex<Link>>>,
    link_id: &AddressHash,
) -> Option<Arc<Mutex<Link>>> {
    for link in links {
        let Ok(link_guard) = link.try_lock() else {
            log::debug!("tp: skipping busy output link during link-id lookup");
            continue;
        };
        if *link_guard.id() == *link_id {
            drop(link_guard);
            return Some(link);
        }
    }
    None
}

impl Transport {
    pub(crate) async fn send_link_packet_on_bound_iface(
        &self,
        link: &Arc<Mutex<Link>>,
        packet: Packet,
    ) -> SendPacketOutcome {
        let Ok(link_guard) = link.try_lock() else {
            log::debug!("tp: dropping link-bound packet dispatch for busy link");
            return SendPacketOutcome::DroppedNoRoute;
        };
        let Some(iface) = link_guard.ingress_iface() else {
            return SendPacketOutcome::DroppedNoRoute;
        };
        drop(link_guard);
        let dispatch = TransportHandler::send_message_unlocked(
            self.handler.clone(),
            TxMessage { tx_type: TxMessageType::Direct(iface), packet },
        )
        .await;
        if dispatch.sent_ifaces > 0 {
            SendPacketOutcome::SentDirect
        } else {
            SendPacketOutcome::DroppedNoRoute
        }
    }

    async fn resource_lane(&self) -> resource_lane::ResourceManagerLane {
        self.handler.lock().await.resource_lane.clone()
    }

    async fn commit_prepared_resource_send(
        &self,
        prepared: PreparedResourceSend,
    ) -> Result<(Hash, Packet), RnsError> {
        self.resource_lane().await.commit_prepared_send(prepared).await
    }

    async fn confirm_resource_dispatch(&self, resource_hash: Hash, sent: bool) {
        self.resource_lane().await.confirm_outbound_dispatch(resource_hash, sent).await;
    }

    async fn find_any_link(&self, link_id: &AddressHash) -> Option<Arc<Mutex<Link>>> {
        let (out_links, in_link) = {
            let handler = self.handler.lock().await;
            (
                handler.out_links.values().cloned().collect::<Vec<_>>(),
                handler.in_links.get(link_id).cloned(),
            )
        };

        if let Some(link) = in_link {
            return Some(link);
        }

        find_ready_out_link_candidate(out_links, link_id)
    }

    async fn prepare_resource_send_on_worker(
        link: Arc<Mutex<Link>>,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        request_id: Option<Vec<u8>>,
        is_response: bool,
    ) -> Result<PreparedResourceSend, RnsError> {
        let link_context = match link.try_lock() {
            Ok(link) => link.packet_context(),
            Err(_) => {
                log::debug!("resource: skipping send preparation for busy link");
                return Err(RnsError::ConnectionError);
            }
        };
        let permit = resource_prepare_permits().try_acquire_owned().map_err(|_| {
            log::debug!("resource: skipping send preparation while worker lane is saturated");
            RnsError::ConnectionError
        })?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            if request_id.is_none() && !is_response {
                ResourceManager::prepare_send_for(&link_context, data, metadata)
            } else {
                ResourceManager::prepare_send_for_with_options(
                    &link_context,
                    data,
                    metadata,
                    request_id,
                    is_response,
                )
            }
        })
        .await
        .map_err(|_| RnsError::ConnectionError)?
    }

    pub async fn send_to_all_out_links(&self, payload: &[u8]) {
        let links = { self.handler.lock().await.out_links.values().cloned().collect::<Vec<_>>() };
        let packets = collect_ready_link_packets(links, None, payload, Link::data_packet);
        if packets.is_empty() {
            return;
        }
        for packet in packets {
            let _ = TransportHandler::send_packet_with_trace_unlocked(self.handler.clone(), packet)
                .await;
        }
    }

    pub async fn send_channel_to_all_out_links(&self, payload: &[u8]) {
        let links = { self.handler.lock().await.out_links.values().cloned().collect::<Vec<_>>() };
        let packets = collect_ready_link_packets(links, None, payload, Link::channel_packet);
        if packets.is_empty() {
            return;
        }
        for packet in packets {
            let _ = TransportHandler::send_packet_with_trace_unlocked(self.handler.clone(), packet)
                .await;
        }
    }

    pub async fn send_to_out_links(&self, destination: &AddressHash, payload: &[u8]) {
        let mut count = 0usize;
        let links = { self.handler.lock().await.out_links.values().cloned().collect::<Vec<_>>() };
        let packets =
            collect_ready_link_packets(links, Some(destination), payload, Link::data_packet);
        if !packets.is_empty() {
            count = packets.len();
            for packet in packets {
                let _ =
                    TransportHandler::send_packet_with_trace_unlocked(self.handler.clone(), packet)
                        .await;
            }
        }

        if count == 0 {
            log::trace!("tp({}): no output links for {} destination", self.name, destination);
        }
    }

    pub async fn send_channel_to_out_links(&self, destination: &AddressHash, payload: &[u8]) {
        let mut count = 0usize;
        let links = { self.handler.lock().await.out_links.values().cloned().collect::<Vec<_>>() };
        let packets =
            collect_ready_link_packets(links, Some(destination), payload, Link::channel_packet);
        if !packets.is_empty() {
            count = packets.len();
            for packet in packets {
                let _ =
                    TransportHandler::send_packet_with_trace_unlocked(self.handler.clone(), packet)
                        .await;
            }
        }

        if count == 0 {
            log::trace!("tp({}): no output links for {} destination", self.name, destination);
        }
    }

    pub async fn send_to_in_links(&self, destination: &AddressHash, payload: &[u8]) {
        let mut count = 0usize;
        let links = { self.handler.lock().await.in_links.values().cloned().collect::<Vec<_>>() };
        let packets =
            collect_ready_link_packets(links, Some(destination), payload, Link::data_packet);
        if !packets.is_empty() {
            count = packets.len();
            for packet in packets {
                let _ =
                    TransportHandler::send_packet_with_trace_unlocked(self.handler.clone(), packet)
                        .await;
            }
        }

        if count == 0 {
            log::trace!("tp({}): no input links for {} destination", self.name, destination);
        }
    }

    pub async fn send_channel_to_in_links(&self, destination: &AddressHash, payload: &[u8]) {
        let mut count = 0usize;
        let links = { self.handler.lock().await.in_links.values().cloned().collect::<Vec<_>>() };
        let packets =
            collect_ready_link_packets(links, Some(destination), payload, Link::channel_packet);
        if !packets.is_empty() {
            count = packets.len();
            for packet in packets {
                let _ =
                    TransportHandler::send_packet_with_trace_unlocked(self.handler.clone(), packet)
                        .await;
            }
        }

        if count == 0 {
            log::trace!("tp({}): no input links for {} destination", self.name, destination);
        }
    }

    pub async fn send_resource(
        &self,
        link_id: &AddressHash,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
    ) -> Result<Hash, RnsError> {
        let link = self.find_any_link(link_id).await.ok_or(RnsError::InvalidArgument)?;
        self.send_resource_on_link(link, data, metadata).await
    }

    pub async fn send_resource_on_link(
        &self,
        link: Arc<Mutex<Link>>,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
    ) -> Result<Hash, RnsError> {
        let prepared =
            Self::prepare_resource_send_on_worker(Arc::clone(&link), data, metadata, None, false)
                .await?;
        let (resource_hash, packet) = self.commit_prepared_resource_send(prepared).await?;
        let outcome = self.send_link_packet_on_bound_iface(&link, packet).await;
        let sent =
            matches!(outcome, SendPacketOutcome::SentDirect | SendPacketOutcome::SentBroadcast);
        self.confirm_resource_dispatch(resource_hash, sent).await;
        if sent {
            Ok(resource_hash)
        } else {
            Err(RnsError::ConnectionError)
        }
    }

    pub async fn send_response_resource(
        &self,
        link_id: &AddressHash,
        request_id: Vec<u8>,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
    ) -> Result<Hash, RnsError> {
        let link = self.find_any_link(link_id).await.ok_or(RnsError::InvalidArgument)?;
        let prepared = Self::prepare_resource_send_on_worker(
            Arc::clone(&link),
            data,
            metadata,
            Some(request_id),
            true,
        )
        .await?;
        let (resource_hash, packet) = self.commit_prepared_resource_send(prepared).await?;
        let outcome = self.send_link_packet_on_bound_iface(&link, packet).await;
        let sent =
            matches!(outcome, SendPacketOutcome::SentDirect | SendPacketOutcome::SentBroadcast);
        self.confirm_resource_dispatch(resource_hash, sent).await;
        if sent {
            Ok(resource_hash)
        } else {
            Err(RnsError::ConnectionError)
        }
    }

    pub async fn send_request_resource(
        &self,
        link_id: &AddressHash,
        request_id: Vec<u8>,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
    ) -> Result<Hash, RnsError> {
        let link = self.find_any_link(link_id).await.ok_or(RnsError::InvalidArgument)?;
        let prepared = Self::prepare_resource_send_on_worker(
            Arc::clone(&link),
            data,
            metadata,
            Some(request_id),
            false,
        )
        .await?;
        let (resource_hash, packet) = self.commit_prepared_resource_send(prepared).await?;
        let outcome = self.send_link_packet_on_bound_iface(&link, packet).await;
        let sent =
            matches!(outcome, SendPacketOutcome::SentDirect | SendPacketOutcome::SentBroadcast);
        self.confirm_resource_dispatch(resource_hash, sent).await;
        if sent {
            Ok(resource_hash)
        } else {
            Err(RnsError::ConnectionError)
        }
    }

    pub async fn send_channel_message(
        &self,
        link_id: &AddressHash,
        msg_type: u16,
        payload: Vec<u8>,
    ) -> Result<u16, crate::channel::ChannelError> {
        let link =
            self.find_any_link(link_id).await.ok_or(crate::channel::ChannelError::LinkNotReady)?;

        let (sequence, iface, packet) = {
            let mut link = link.lock().await;
            let iface = link.ingress_iface().ok_or(crate::channel::ChannelError::LinkNotReady)?;
            let (sequence, packet) = link.send_channel_message(msg_type, payload)?;
            (sequence, iface, packet)
        };

        let dispatch = TransportHandler::send_message_unlocked(
            self.handler.clone(),
            TxMessage { tx_type: TxMessageType::Direct(iface), packet },
        )
        .await;
        if dispatch.sent_ifaces == 0 {
            link.lock().await.mark_channel_failed(sequence);
            return Err(crate::channel::ChannelError::LinkNotReady);
        }

        Ok(sequence)
    }

    pub async fn register_channel_handler<F>(
        &self,
        link_id: &AddressHash,
        msg_type: u16,
        handler: F,
    ) -> Result<crate::channel::HandlerId, crate::channel::ChannelError>
    where
        F: FnMut(crate::channel::Envelope) -> bool + Send + 'static,
    {
        let link =
            self.find_any_link(link_id).await.ok_or(crate::channel::ChannelError::LinkNotReady)?;
        let handler_id = link.lock().await.register_channel_handler(msg_type, handler);
        Ok(handler_id)
    }

    pub async fn remove_channel_handler(
        &self,
        link_id: &AddressHash,
        handler_id: crate::channel::HandlerId,
    ) -> Result<bool, crate::channel::ChannelError> {
        let link =
            self.find_any_link(link_id).await.ok_or(crate::channel::ChannelError::LinkNotReady)?;
        let removed = link.lock().await.remove_channel_handler(handler_id);
        Ok(removed)
    }

    pub async fn open_channel(
        &self,
        link_id: &AddressHash,
    ) -> Result<(), crate::channel::ChannelError> {
        let link =
            self.find_any_link(link_id).await.ok_or(crate::channel::ChannelError::LinkNotReady)?;
        link.lock().await.open_channel();
        Ok(())
    }

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
        let prepared =
            Self::prepare_resource_send_on_worker(Arc::clone(&link), data, metadata, None, false)
                .await?;
        let (resource_hash, packet) = self.commit_prepared_resource_send(prepared).await?;
        let dispatch = TransportHandler::send_message_unlocked(
            self.handler.clone(),
            TxMessage { tx_type: TxMessageType::Direct(iface), packet },
        )
        .await;
        let sent = dispatch.sent_ifaces > 0;
        self.confirm_resource_dispatch(resource_hash, sent).await;
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
        find_ready_out_link_candidate(links, link_id)
    }

    pub async fn find_in_link(&self, link_id: &AddressHash) -> Option<Arc<Mutex<Link>>> {
        self.handler.lock().await.in_links.get(link_id).cloned()
    }

    pub async fn link(&self, destination: DestinationDesc) -> Arc<Mutex<Link>> {
        let link = self.handler.lock().await.out_links.get(&destination.address_hash).cloned();

        if let Some(link) = link {
            let should_reuse = match link.try_lock() {
                Ok(link_guard) if link_guard.status() == LinkStatus::Closed => {
                    log::warn!("tp({}): link was closed", self.name);
                    false
                }
                Ok(_) => true,
                Err(_) => {
                    log::debug!("tp({}): reusing busy output link for {}", self.name, destination);
                    true
                }
            };
            if should_reuse {
                return link;
            }
        }

        let mut link = Link::new(destination, self.link_out_event_tx.clone());

        let packet = link.request();

        log::debug!(
            "tp({}): create new link {} for destination {}",
            self.name,
            link.id(),
            destination
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
    ) {
        let packet = {
            let mut handler = self.handler.lock().await;
            handler.path_requests.generate(destination, tag)
        };
        let _ = TransportHandler::send_message_unlocked(
            self.handler.clone(),
            TxMessage { tx_type: TxMessageType::Broadcast(on_iface), packet },
        )
        .await;
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
        &mut self,
        identity: PrivateIdentity,
        name: DestinationName,
    ) -> Arc<Mutex<SingleInputDestination>> {
        let destination = SingleInputDestination::new(identity, name);
        let address_hash = destination.desc.address_hash;

        log::debug!("tp({}): add destination {}", self.name, address_hash);

        let destination = Arc::new(Mutex::new(destination));

        self.handler.lock().await.single_in_destinations.insert(address_hash, destination.clone());

        destination
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

impl TransportChannel {
    async fn find_link(&self) -> Option<Arc<Mutex<Link>>> {
        let (out_links, in_link) = {
            let handler = self.handler.lock().await;
            (
                handler.out_links.values().cloned().collect::<Vec<_>>(),
                handler.in_links.get(&self.link_id).cloned(),
            )
        };

        if let Some(link) = in_link {
            return Some(link);
        }

        find_ready_out_link_candidate(out_links, &self.link_id)
    }

    pub fn link_id(&self) -> AddressHash {
        self.link_id
    }

    pub async fn send(
        &self,
        msg_type: u16,
        payload: Vec<u8>,
    ) -> Result<u16, crate::channel::ChannelError> {
        let link = self.find_link().await.ok_or(crate::channel::ChannelError::LinkNotReady)?;

        let (sequence, iface, packet) = {
            let mut link = link.lock().await;
            let iface = link.ingress_iface().ok_or(crate::channel::ChannelError::LinkNotReady)?;
            let (sequence, packet) = link.send_channel_message(msg_type, payload)?;
            (sequence, iface, packet)
        };

        let dispatch = TransportHandler::send_message_unlocked(
            self.handler.clone(),
            TxMessage { tx_type: TxMessageType::Direct(iface), packet },
        )
        .await;
        if dispatch.sent_ifaces == 0 {
            link.lock().await.mark_channel_failed(sequence);
            return Err(crate::channel::ChannelError::LinkNotReady);
        }

        Ok(sequence)
    }

    pub async fn open(&self) -> Result<(), crate::channel::ChannelError> {
        let link = self.find_link().await.ok_or(crate::channel::ChannelError::LinkNotReady)?;
        link.lock().await.open_channel();
        Ok(())
    }

    pub async fn close(&self) -> Result<(), crate::channel::ChannelError> {
        let link = self.find_link().await.ok_or(crate::channel::ChannelError::LinkNotReady)?;
        link.lock().await.close_channel();
        Ok(())
    }

    pub async fn is_ready_to_send(&self) -> Result<bool, crate::channel::ChannelError> {
        let link = self.find_link().await.ok_or(crate::channel::ChannelError::LinkNotReady)?;
        let ready = link.lock().await.channel_ready_to_send();
        Ok(ready)
    }

    pub async fn close_wait_hint(&self) -> Result<Duration, crate::channel::ChannelError> {
        let link = self.find_link().await.ok_or(crate::channel::ChannelError::LinkNotReady)?;
        let hint = link.lock().await.channel_close_wait_hint();
        Ok(hint)
    }
    pub async fn send_typed<M: crate::channel::TypedMessage>(
        &self,
        message: &M,
    ) -> Result<u16, crate::channel::ChannelError> {
        self.send(M::MSG_TYPE, message.encode()).await
    }

    pub async fn register_handler<F>(
        &self,
        msg_type: u16,
        handler: F,
    ) -> Result<crate::channel::HandlerId, crate::channel::ChannelError>
    where
        F: FnMut(crate::channel::Envelope) -> bool + Send + 'static,
    {
        let link = self.find_link().await.ok_or(crate::channel::ChannelError::LinkNotReady)?;
        let handler_id = link.lock().await.register_channel_handler(msg_type, handler);
        Ok(handler_id)
    }

    pub async fn register_typed_handler<M, F>(
        &self,
        mut handler: F,
    ) -> Result<crate::channel::HandlerId, crate::channel::ChannelError>
    where
        M: crate::channel::TypedMessage,
        F: FnMut(M) -> bool + Send + 'static,
    {
        crate::channel::validate_typed_message_type::<M>()?;
        self.register_handler(M::MSG_TYPE, move |envelope| match M::decode(&envelope.payload) {
            Ok(message) => handler(message),
            Err(_) => false,
        })
        .await
    }

    pub async fn remove_handler(
        &self,
        handler_id: crate::channel::HandlerId,
    ) -> Result<bool, crate::channel::ChannelError> {
        let link = self.find_link().await.ok_or(crate::channel::ChannelError::LinkNotReady)?;
        let removed = link.lock().await.remove_channel_handler(handler_id);
        Ok(removed)
    }

    pub async fn message_state(
        &self,
        sequence: u16,
    ) -> Result<crate::channel::MessageState, crate::channel::ChannelError> {
        let link = self.find_link().await.ok_or(crate::channel::ChannelError::LinkNotReady)?;
        let state = link.lock().await.channel_state(sequence);
        Ok(state)
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn out_link_lookup_skips_busy_nonmatching_candidates() {
        let identity = PrivateIdentity::new_from_rand(OsRng);
        let destination = DestinationDesc {
            identity: *identity.as_identity(),
            address_hash: identity.as_identity().address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (events, _) = tokio::sync::broadcast::channel(8);

        let busy = Arc::new(Mutex::new(Link::new(destination, events.clone())));
        let _busy_guard = busy.lock().await;

        let mut ready = Link::new(destination, events);
        let _request = ready.request();
        let ready_id = *ready.id();
        let ready = Arc::new(Mutex::new(ready));

        let found = find_ready_out_link_candidate(vec![busy.clone(), ready.clone()], &ready_id)
            .expect("ready output link should be found");

        assert!(Arc::ptr_eq(&found, &ready));
    }

    #[tokio::test]
    async fn link_bound_dispatch_skips_busy_link_instead_of_waiting_for_iface() {
        let identity = PrivateIdentity::new_from_rand(OsRng);
        let transport = Transport::new(TransportConfig::new("test", &identity, true));
        let destination = DestinationDesc {
            identity: *identity.as_identity(),
            address_hash: identity.as_identity().address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (events, _) = tokio::sync::broadcast::channel(8);
        let link = Arc::new(Mutex::new(Link::new(destination, events)));
        let _busy_guard = link.lock().await;

        let outcome = tokio::time::timeout(
            Duration::from_millis(200),
            transport.send_link_packet_on_bound_iface(&link, Packet::default()),
        )
        .await
        .expect("link-bound dispatch should not wait for a busy link");

        assert_eq!(outcome, SendPacketOutcome::DroppedNoRoute);
    }

    #[tokio::test]
    async fn link_reuse_skips_busy_existing_out_link_status_check() {
        let identity = PrivateIdentity::new_from_rand(OsRng);
        let transport = Transport::new(TransportConfig::new("test", &identity, true));
        let destination = DestinationDesc {
            identity: *identity.as_identity(),
            address_hash: identity.as_identity().address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (events, _) = tokio::sync::broadcast::channel(8);
        let link = Arc::new(Mutex::new(Link::new(destination, events)));
        transport
            .get_handler()
            .lock()
            .await
            .out_links
            .insert(destination.address_hash, link.clone());

        let _busy_guard = link.lock().await;
        let reused = tokio::time::timeout(Duration::from_millis(200), transport.link(destination))
            .await
            .expect("link reuse should not wait for a busy existing out link");

        assert!(Arc::ptr_eq(&reused, &link));
    }
}
