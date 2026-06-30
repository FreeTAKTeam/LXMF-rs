impl AutoInterfaceTransportRuntime {
    #[allow(dead_code)]
    pub(crate) fn from_channel(
        channel: InterfaceChannel,
        iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    ) -> Self {
        let host_iface = channel.address;
        Self {
            bridge: AutoInterfaceTransportBridge {
                host_iface,
                iface_manager,
                rx_channel: channel.rx_channel,
                peer_ifaces: Arc::new(tokio::sync::Mutex::new(BTreeMap::new())),
                outbound_routes: Arc::new(tokio::sync::Mutex::new(BTreeMap::new())),
            },
            tx_channel: channel.tx_channel,
        }
    }

    fn split(self) -> (AutoInterfaceTransportBridge, InterfaceTxReceiver) {
        (self.bridge, self.tx_channel)
    }
}

impl AutoInterfaceTransportBridge {
    async fn ensure_peer_iface(
        &self,
        peer: SocketAddr,
        route: AutoPeerOutboundRoute,
    ) -> Option<AddressHash> {
        if let Some(existing) = self.peer_ifaces.lock().await.get(&peer).copied() {
            self.outbound_routes.lock().await.insert(existing, route);
            return Some(existing);
        }

        let virtual_iface = {
            let mut manager = self.iface_manager.lock().await;
            manager.register_virtual_iface(self.host_iface, IfaceRole::VirtualUnicast)?
        };
        self.peer_ifaces.lock().await.insert(peer, virtual_iface);
        self.outbound_routes.lock().await.insert(virtual_iface, route);
        Some(virtual_iface)
    }

    async fn forward_peer_data(
        &self,
        processed: &AutoProcessedPeerDataDatagram,
        socket: Arc<tokio::net::UdpSocket>,
    ) -> AutoPeerDataForwardResult {
        if !matches!(processed.decision, AutoPeerInboundDecision::Accepted { .. }) {
            return AutoPeerDataForwardResult::NotForwarded;
        }
        let Some(virtual_iface) = self
            .ensure_peer_iface(
                processed.datagram.source_addr,
                AutoPeerOutboundRoute { socket, destination: processed.datagram.source_addr },
            )
            .await
        else {
            log::warn!(
                "[daemon-auto] failed to register virtual peer iface for {}",
                processed.datagram.source_addr
            );
            return AutoPeerDataForwardResult::VirtualIfaceUnavailable;
        };
        let packet = match Packet::deserialize(&mut InputBuffer::new(&processed.datagram.payload)) {
            Ok(packet) => packet,
            Err(err) => {
                log::warn!(
                    "[daemon-auto] failed to decode peer data packet from {}: {:?}",
                    processed.datagram.source_addr,
                    err
                );
                return AutoPeerDataForwardResult::DecodeFailed;
            }
        };
        if self
            .rx_channel
            .send(RxMessage {
                address: virtual_iface,
                packet,
                source: IfaceSource::Udp(processed.datagram.source_addr),
            })
            .await
            .is_err()
        {
            log::warn!(
                "[daemon-auto] failed to forward peer data packet from {}: rx channel closed",
                processed.datagram.source_addr
            );
            return AutoPeerDataForwardResult::RxChannelClosed;
        }
        AutoPeerDataForwardResult::Delivered
    }

    async fn remove_outbound_routes_for_socket(&self, socket: &Arc<tokio::net::UdpSocket>) -> usize {
        let mut routes = self.outbound_routes.lock().await;
        let before = routes.len();
        routes.retain(|_, route| !Arc::ptr_eq(&route.socket, socket));
        before.saturating_sub(routes.len())
    }

    async fn send_outbound(&self, message: TxMessage) {
        match message.tx_type {
            TxMessageType::Direct(iface) => {
                self.send_to_route(iface, message.packet).await;
            }
            TxMessageType::Broadcast(_) => {
                let routes = self.outbound_routes.lock().await.clone();
                for (iface, _) in routes {
                    self.send_to_route(iface, message.packet.clone()).await;
                }
            }
        }
    }

    async fn send_to_route(&self, iface: AddressHash, packet: Packet) {
        let Some(route) = self.outbound_routes.lock().await.get(&iface).cloned() else {
            return;
        };
        let payload = match packet.to_bytes() {
            Ok(payload) => payload,
            Err(err) => {
                log::warn!("[daemon-auto] failed to serialize outbound peer data packet: {err:?}");
                return;
            }
        };
        if let Err(err) = route.socket.send_to(&payload, route.destination).await {
            log::warn!(
                "[daemon-auto] failed to send outbound peer data packet to {}: {err}",
                route.destination
            );
        }
    }
}
