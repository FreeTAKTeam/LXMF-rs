impl Transport {
    /// Reports whether the LXMF router can reuse a direct or backchannel link.
    ///
    /// This mirrors Python LXMRouter membership semantics while excluding
    /// links that have already reached the closed state.
    pub async fn delivery_link_available(&self, destination: &AddressHash) -> bool {
        self.delivery_link(destination).await.is_some()
    }

    /// The link a send to `destination` can reuse: the direct link first, then
    /// the peer's backchannel — Python `LXMRouter.direct_links` before
    /// `LXMRouter.backchannel_links`.
    ///
    /// A caller about to send needs the link itself, not
    /// [`delivery_link_available`](Self::delivery_link_available)'s answer.
    /// Reaching a peer over their backchannel means sending on that inbound
    /// link; [`Transport::link`] searches `out_links` alone, so asking it for
    /// the same destination opens a second link to a peer already reachable on
    /// the first.
    pub async fn delivery_link(&self, destination: &AddressHash) -> Option<Arc<Mutex<Link>>> {
        let (out_link, in_links) = {
            let handler = self.handler.lock().await;
            (
                handler.out_links.get(destination).cloned(),
                handler.in_links.values().cloned().collect::<Vec<_>>(),
            )
        };

        if let Some(link) = out_link {
            if link.lock().await.status() != LinkStatus::Closed {
                return Some(link);
            }
        }

        // A peer who identified over a link to one of our destinations is
        // reachable at their own destination of the same name: Python
        // `LXMRouter.backchannel_links`, keyed by the hash
        // `delivery_remote_identified` derives from the identified identity.
        // The link's own destination is ours, and never the one being asked
        // about.
        for link in in_links {
            let guard = link.lock().await;
            if guard.status() == LinkStatus::Closed {
                continue;
            }
            let Some(peer) = guard.identified_peer_identity() else {
                continue;
            };
            let peer_destination =
                crate::destination::SingleOutputDestination::new(*peer, guard.destination().name)
                    .desc
                    .address_hash;
            if peer_destination == *destination {
                drop(guard);
                return Some(link);
            }
        }

        None
    }

    pub async fn reset_out_link(&self, destination: &AddressHash) {
        loop {
            let mut handler = self.handler.lock().await;
            let Some(link) = handler.out_links.get(destination).cloned() else {
                return;
            };
            let mut owner = match link.try_lock() {
                Ok(owner) => owner,
                Err(_) => {
                    drop(handler);
                    drop(link.lock().await);
                    continue;
                }
            };
            super::resource_wire::publish_resource_events_for_link(
                &mut handler,
                *owner.id(),
                Some(&mut owner),
            );
            owner.close();
            super::requests::retire_closed_link(&mut handler, &mut owner);
            handler.out_links.remove(destination);
            return;
        }
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

    pub(super) async fn find_any_link(&self, link_id: &AddressHash) -> Option<Arc<Mutex<Link>>> {
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

    pub(super) async fn resource_mtu_for_iface(&self, iface: Option<AddressHash>) -> usize {
        let Some(iface) = iface else {
            return crate::resource::DEFAULT_RESOURCE_INTERFACE_MTU;
        };
        self.iface_manager
            .lock()
            .await
            .mtu(&iface)
            .unwrap_or(crate::resource::DEFAULT_RESOURCE_INTERFACE_MTU)
    }

    pub async fn send_response_resource(
        &self,
        link_id: &AddressHash,
        request_id: Vec<u8>,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
    ) -> Result<Hash, RnsError> {
        self.send_response_resource_with_compression(link_id, request_id, data, metadata, true)
            .await
    }

    /// Send a response Resource with explicit control over opportunistic
    /// outbound compression. Some registered response handlers, such as
    /// NomadNet media, deliberately return raw binary data and set
    /// `auto_compress=False` in the Python implementation.
    pub async fn send_response_resource_with_compression(
        &self,
        link_id: &AddressHash,
        request_id: Vec<u8>,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        auto_compress: bool,
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
            ResourceManager::prepare_send_with_compression(
                &link_guard,
                data,
                metadata,
                Some(request_id),
                true,
                interface_mtu,
                auto_compress,
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
        super::resource_wire::publish_resource_events(&mut handler).await;
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
