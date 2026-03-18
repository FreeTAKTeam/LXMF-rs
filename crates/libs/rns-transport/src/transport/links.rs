use super::*;

impl Transport {
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

        for link in out_links {
            if *link.lock().await.id() == *link_id {
                return Some(link);
            }
        }

        None
    }

    pub async fn send_to_all_out_links(&self, payload: &[u8]) {
        let packets = {
            let handler = self.handler.lock().await;
            let mut packets = Vec::new();
            for link in handler.out_links.values() {
                let link = link.lock().await;
                if link.status() == LinkStatus::Active {
                    if let Ok(packet) = link.data_packet(payload) {
                        packets.push(packet);
                    }
                }
            }
            packets
        };
        if packets.is_empty() {
            return;
        }
        let mut handler = self.handler.lock().await;
        for packet in packets {
            handler.send_packet(packet).await;
        }
    }

    pub async fn send_channel_to_all_out_links(&self, payload: &[u8]) {
        let packets = {
            let handler = self.handler.lock().await;
            let mut packets = Vec::new();
            for link in handler.out_links.values() {
                let link = link.lock().await;
                if link.status() == LinkStatus::Active {
                    if let Ok(packet) = link.channel_packet(payload) {
                        packets.push(packet);
                    }
                }
            }
            packets
        };
        if packets.is_empty() {
            return;
        }
        let mut handler = self.handler.lock().await;
        for packet in packets {
            handler.send_packet(packet).await;
        }
    }

    pub async fn send_to_out_links(&self, destination: &AddressHash, payload: &[u8]) {
        let mut count = 0usize;
        let packets = {
            let handler = self.handler.lock().await;
            let mut packets = Vec::new();
            for link in handler.out_links.values() {
                let link = link.lock().await;
                if link.destination().address_hash == *destination
                    && link.status() == LinkStatus::Active
                {
                    if let Ok(packet) = link.data_packet(payload) {
                        packets.push(packet);
                    }
                }
            }
            packets
        };
        if !packets.is_empty() {
            let mut handler = self.handler.lock().await;
            for packet in packets {
                handler.send_packet(packet).await;
                count += 1;
            }
        }

        if count == 0 {
            log::trace!("tp({}): no output links for {} destination", self.name, destination);
        }
    }

    pub async fn send_channel_to_out_links(&self, destination: &AddressHash, payload: &[u8]) {
        let mut count = 0usize;
        let packets = {
            let handler = self.handler.lock().await;
            let mut packets = Vec::new();
            for link in handler.out_links.values() {
                let link = link.lock().await;
                if link.destination().address_hash == *destination
                    && link.status() == LinkStatus::Active
                {
                    if let Ok(packet) = link.channel_packet(payload) {
                        packets.push(packet);
                    }
                }
            }
            packets
        };
        if !packets.is_empty() {
            let mut handler = self.handler.lock().await;
            for packet in packets {
                handler.send_packet(packet).await;
                count += 1;
            }
        }

        if count == 0 {
            log::trace!("tp({}): no output links for {} destination", self.name, destination);
        }
    }

    pub async fn send_to_in_links(&self, destination: &AddressHash, payload: &[u8]) {
        let mut count = 0usize;
        let packets = {
            let handler = self.handler.lock().await;
            let mut packets = Vec::new();
            for link in handler.in_links.values() {
                let link = link.lock().await;

                if link.destination().address_hash == *destination
                    && link.status() == LinkStatus::Active
                {
                    if let Ok(packet) = link.data_packet(payload) {
                        packets.push(packet);
                    }
                }
            }
            packets
        };
        if !packets.is_empty() {
            let mut handler = self.handler.lock().await;
            for packet in packets {
                handler.send_packet(packet).await;
                count += 1;
            }
        }

        if count == 0 {
            log::trace!("tp({}): no input links for {} destination", self.name, destination);
        }
    }

    pub async fn send_channel_to_in_links(&self, destination: &AddressHash, payload: &[u8]) {
        let mut count = 0usize;
        let packets = {
            let handler = self.handler.lock().await;
            let mut packets = Vec::new();
            for link in handler.in_links.values() {
                let link = link.lock().await;

                if link.destination().address_hash == *destination
                    && link.status() == LinkStatus::Active
                {
                    if let Ok(packet) = link.channel_packet(payload) {
                        packets.push(packet);
                    }
                }
            }
            packets
        };
        if !packets.is_empty() {
            let mut handler = self.handler.lock().await;
            for packet in packets {
                handler.send_packet(packet).await;
                count += 1;
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
        let mut handler = self.handler.lock().await;
        let link_guard = link.lock().await;
        let (resource_hash, packet) =
            handler.resource_manager.start_send(&link_guard, data, metadata)?;
        drop(link_guard);
        handler.send_packet(packet).await;
        Ok(resource_hash)
    }

    pub async fn send_channel_message(
        &self,
        link_id: &AddressHash,
        msg_type: u16,
        payload: Vec<u8>,
    ) -> Result<u16, crate::channel::ChannelError> {
        let link =
            self.find_any_link(link_id).await.ok_or(crate::channel::ChannelError::LinkNotReady)?;

        let (sequence, packet) = {
            let mut link = link.lock().await;
            link.send_channel_message(msg_type, payload)?
        };
        self.handler.lock().await.send_packet(packet).await;
        Ok(sequence)
    }

    pub async fn register_channel_handler<F>(
        &self,
        link_id: &AddressHash,
        msg_type: u16,
        handler: F,
    ) -> Result<(), crate::channel::ChannelError>
    where
        F: FnMut(crate::channel::Envelope) -> bool + Send + 'static,
    {
        let link =
            self.find_any_link(link_id).await.ok_or(crate::channel::ChannelError::LinkNotReady)?;
        link.lock().await.register_channel_handler(msg_type, handler);
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
        let (resource_hash, packet) = {
            let mut handler = self.handler.lock().await;
            let link_guard = link.lock().await;
            handler.resource_manager.start_send(&link_guard, data, metadata)?
        };
        self.handler
            .lock()
            .await
            .send(TxMessage { tx_type: TxMessageType::Direct(iface), packet })
            .await;
        Ok(resource_hash)
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
            if link.lock().await.status() != LinkStatus::Closed {
                return link;
            } else {
                log::warn!("tp({}): link was closed", self.name);
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
        self.handler.lock().await.request_path(destination, on_iface, tag).await
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

impl Drop for Transport {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
