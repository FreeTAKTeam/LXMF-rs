impl Transport {
    /// Reports whether the LXMF router can reuse a direct or backchannel link.
    ///
    /// This mirrors Python LXMRouter membership semantics while excluding
    /// links that have already reached the closed state.
    pub async fn delivery_link_available(&self, destination: &AddressHash) -> bool {
        let (out_link, in_links) = {
            let handler = self.handler.lock().await;
            (
                handler.out_links.get(destination).cloned(),
                handler.in_links.values().cloned().collect::<Vec<_>>(),
            )
        };

        if let Some(link) = out_link {
            if link.lock().await.status() != LinkStatus::Closed {
                return true;
            }
        }

        for link in in_links {
            let link = link.lock().await;
            if link.destination().address_hash == *destination && link.status() != LinkStatus::Closed
            {
                return true;
            }
        }

        false
    }


    pub async fn reset_out_link(&self, destination: &AddressHash) {
        let removed = {
            let mut handler = self.handler.lock().await;
            handler.out_links.remove(destination)
        };
        let Some(link) = removed else {
            return;
        };

        let link_id = {
            let mut link = link.lock().await;
            let link_id = *link.id();
            link.close();
            link_id
        };
        self.handler.lock().await.resource_manager.remove_link_state(link_id);
    }

    /// The correct way to send any packet addressed to an already-open
    /// Link (`destination_type: Link` — identify, keepalive, channel,
    /// plain data) rather than a fresh destination. `Transport::
    /// send_packet`/`send_packet_with_outcome` route via the path table,
    /// keyed by real announced destination hashes — never by a Link's own
    /// ephemeral id, which is what a Link-context packet's `destination`
    /// field actually holds. Public (not `pub(crate)`) specifically so a
    /// downstream consumer building its own Link-context packet via
    /// `Link::identify_packet`/`data_packet`/`channel_packet` — anything
    /// this crate doesn't already wrap in a higher-level helper like
    /// `crate::delivery::send_on_link_observed` — has a correct way to
    /// send it at all.
    pub async fn send_link_packet_on_bound_iface(
        &self,
        link: &Arc<Mutex<Link>>,
        packet: Packet,
    ) -> SendPacketOutcome {
        let Some(iface) = link.lock().await.ingress_iface() else {
            return SendPacketOutcome::DroppedNoRoute;
        };
        let dispatch = self
            .handler
            .lock()
            .await
            .send(TxMessage { tx_type: TxMessageType::Direct(iface), packet })
            .await;
        if dispatch.sent_ifaces > 0 {
            SendPacketOutcome::SentDirect
        } else {
            SendPacketOutcome::DroppedNoRoute
        }
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

        for link in out_links {
            if *link.lock().await.id() == *link_id {
                return Some(link);
            }
        }

        None
    }

    async fn resource_mtu_for_iface(&self, iface: Option<AddressHash>) -> usize {
        let Some(iface) = iface else {
            return crate::resource::DEFAULT_RESOURCE_INTERFACE_MTU;
        };
        self.iface_manager
            .lock()
            .await
            .mtu(&iface)
            .unwrap_or(crate::resource::DEFAULT_RESOURCE_INTERFACE_MTU)
    }

    pub async fn send_resource(
        &self,
        link_id: &AddressHash,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
    ) -> Result<Hash, RnsError> {
        self.send_resource_observed(link_id, data, metadata, |_| {}).await
    }

    pub async fn send_resource_observed(
        &self,
        link_id: &AddressHash,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        observe_resource: impl FnOnce(Hash),
    ) -> Result<Hash, RnsError> {
        let link = self.find_any_link(link_id).await.ok_or(RnsError::InvalidArgument)?;
        let iface = {
            let link_guard = link.lock().await;
            link_guard.ingress_iface()
        };
        let interface_mtu = self.resource_mtu_for_iface(iface).await;
        // Prepared before the handler lock is taken, not under it. Building the
        // advertisement compresses, encrypts and chunks the payload, which on a
        // large send is a long time to hold a mutex every other link, announce
        // and packet on this node also needs.
        let prepared = {
            let link_guard = link.lock().await;
            // See `resource_wire.rs`: the negotiated link MTU, not the local
            // interface alone, is what a fragment has to fit through.
            let interface_mtu = interface_mtu.min(link_guard.link_mtu());
            ResourceManager::prepare_send(&link_guard, data, metadata, None, false, interface_mtu)?
        };
        let mut handler = self.handler.lock().await;
        let (resource_hash, packet) = handler.resource_manager.track_prepared(prepared);
        observe_resource(resource_hash);
        drop(handler);
        let outcome = self.send_link_packet_on_bound_iface(&link, packet).await;
        let mut handler = self.handler.lock().await;
        let sent =
            matches!(outcome, SendPacketOutcome::SentDirect | SendPacketOutcome::SentBroadcast);
        handler.resource_manager.confirm_outbound_dispatch(resource_hash, sent);
        let events = handler.resource_manager.drain_events();
        super::resource_wire::publish_resource_events(&handler, events);
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
        let iface = {
            let link_guard = link.lock().await;
            link_guard.ingress_iface()
        };
        let interface_mtu = self.resource_mtu_for_iface(iface).await;
        // See `send_resource_observed`: prepared off the handler lock.
        let prepared = {
            let link_guard = link.lock().await;
            // See `resource_wire.rs`: the negotiated link MTU, not the local
            // interface alone, is what a fragment has to fit through.
            let interface_mtu = interface_mtu.min(link_guard.link_mtu());
            ResourceManager::prepare_send(
                &link_guard,
                data,
                metadata,
                Some(request_id),
                true,
                interface_mtu,
            )?
        };
        let mut handler = self.handler.lock().await;
        let (resource_hash, packet) = handler.resource_manager.track_prepared(prepared);
        drop(handler);
        let outcome = self.send_link_packet_on_bound_iface(&link, packet).await;
        let mut handler = self.handler.lock().await;
        let sent =
            matches!(outcome, SendPacketOutcome::SentDirect | SendPacketOutcome::SentBroadcast);
        handler.resource_manager.confirm_outbound_dispatch(resource_hash, sent);
        let events = handler.resource_manager.drain_events();
        super::resource_wire::publish_resource_events(&handler, events);
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
        self.send_request_resource_with_max_response_size(link_id, request_id, data, metadata, None)
            .await
    }

    pub async fn send_request_resource_with_max_response_size(
        &self,
        link_id: &AddressHash,
        request_id: Vec<u8>,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        max_response_size: Option<usize>,
    ) -> Result<Hash, RnsError> {
        let link = self.find_any_link(link_id).await.ok_or(RnsError::InvalidArgument)?;
        let response_request_id = request_id.clone();
        let iface = {
            let link_guard = link.lock().await;
            link_guard.ingress_iface()
        };
        let interface_mtu = self.resource_mtu_for_iface(iface).await;
        // See `send_resource_observed`: prepared off the handler lock.
        let prepared = {
            let link_guard = link.lock().await;
            // See `resource_wire.rs`: the negotiated link MTU, not the local
            // interface alone, is what a fragment has to fit through.
            let interface_mtu = interface_mtu.min(link_guard.link_mtu());
            ResourceManager::prepare_send(
                &link_guard,
                data,
                metadata,
                Some(request_id),
                false,
                interface_mtu,
            )?
        };
        let mut handler = self.handler.lock().await;
        let (resource_hash, packet) = handler.resource_manager.track_prepared(prepared);
        drop(handler);
        if let Some(max_response_size) = max_response_size {
            link.lock().await.set_response_size_limit(&response_request_id, max_response_size);
        }
        let outcome = self.send_link_packet_on_bound_iface(&link, packet).await;
        if !matches!(outcome, SendPacketOutcome::SentDirect | SendPacketOutcome::SentBroadcast)
            && max_response_size.is_some()
        {
            link.lock().await.clear_response_size_limit(&response_request_id);
        }
        let mut handler = self.handler.lock().await;
        let sent =
            matches!(outcome, SendPacketOutcome::SentDirect | SendPacketOutcome::SentBroadcast);
        handler.resource_manager.confirm_outbound_dispatch(resource_hash, sent);
        let events = handler.resource_manager.drain_events();
        super::resource_wire::publish_resource_events(&handler, events);
        if sent {
            Ok(resource_hash)
        } else {
            Err(RnsError::ConnectionError)
        }
    }

    pub async fn cancel_resource(
        &self,
        link_id: &AddressHash,
        resource_hash: Hash,
    ) -> Result<bool, RnsError> {
        let link = self.find_any_link(link_id).await.ok_or(RnsError::InvalidArgument)?;
        let packet = {
            let mut handler = self.handler.lock().await;
            let link_guard = link.lock().await;
            let packet = handler.resource_manager.cancel_outgoing(resource_hash, &link_guard)?;
            let events = handler.resource_manager.drain_events();
            super::resource_wire::publish_resource_events(&handler, events);
            packet
        };
        let Some(packet) = packet else {
            return Ok(false);
        };

        let outcome = self.send_link_packet_on_bound_iface(&link, packet).await;
        if matches!(outcome, SendPacketOutcome::SentDirect | SendPacketOutcome::SentBroadcast) {
            Ok(true)
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

        let dispatch = self
            .handler
            .lock()
            .await
            .send(TxMessage { tx_type: TxMessageType::Direct(iface), packet })
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
}
