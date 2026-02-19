use super::jobs::manage_transport;
use super::wire::handle_inbound_packet_for_test;
use super::*;

impl Transport {
    pub fn new(config: TransportConfig) -> Self {
        let (announce_tx, _) = tokio::sync::broadcast::channel(16);
        let (link_in_event_tx, _) = tokio::sync::broadcast::channel(16);
        let (link_out_event_tx, _) = tokio::sync::broadcast::channel(16);
        let (received_data_tx, _) = tokio::sync::broadcast::channel(16);
        let (iface_messages_tx, _) = tokio::sync::broadcast::channel(16);
        let (resource_events_tx, _) = tokio::sync::broadcast::channel(16);

        let iface_manager = InterfaceManager::new(128);

        let rx_receiver = iface_manager.receiver();

        let iface_manager = Arc::new(Mutex::new(iface_manager));

        let announce_cache_capacity = config.announce_cache_capacity;
        let announce_retry_limit = config.announce_retry_limit;
        let announce_queue_len = config.announce_queue_len;
        let announce_cap = config.announce_cap;
        let path_request_timeout_secs = config.path_request_timeout_secs;
        let link_proof_timeout_secs = config.link_proof_timeout_secs;
        let link_idle_timeout_secs = config.link_idle_timeout_secs;
        let resource_retry_interval_secs = config.resource_retry_interval_secs;
        let resource_retry_limit = config.resource_retry_limit;
        let ratchet_store = config.ratchet_store_path.as_ref().map(|path| {
            let mut store = RatchetStore::new(path.clone());
            store.clean_expired(now_secs());
            store
        });

        let transport_id =
            if config.retransmit { Some(*config.identity.address_hash()) } else { None };
        let path_requests = PathRequests::new(
            config.name.as_str(),
            transport_id,
            announce_queue_len,
            announce_cap,
            path_request_timeout_secs,
        );

        let path_request_dest = create_path_request_destination().desc.address_hash;

        let cancel = CancellationToken::new();
        let name = config.name.clone();
        let handler = Arc::new(Mutex::new(TransportHandler {
            config,
            iface_manager: iface_manager.clone(),
            announce_table: AnnounceTable::new(announce_cache_capacity, announce_retry_limit),
            link_table: LinkTable::new(
                Duration::from_secs(link_proof_timeout_secs),
                Duration::from_secs(link_idle_timeout_secs),
            ),
            path_table: PathTable::new(),
            single_in_destinations: HashMap::new(),
            single_out_destinations: HashMap::new(),
            announce_limits: AnnounceLimits::new(),
            out_links: HashMap::new(),
            in_links: HashMap::new(),
            packet_cache: Mutex::new(PacketCache::new()),
            path_requests,
            announce_tx,
            link_in_event_tx: link_in_event_tx.clone(),
            received_data_tx: received_data_tx.clone(),
            ratchet_store,
            resource_manager: ResourceManager::new_with_config(
                Duration::from_secs(resource_retry_interval_secs),
                resource_retry_limit,
            ),
            resource_events_tx: resource_events_tx.clone(),
            fixed_dest_path_requests: path_request_dest,
            cancel: cancel.clone(),
            receipt_handler: None,
        }));

        {
            let handler = handler.clone();
            tokio::spawn(manage_transport(handler, rx_receiver, iface_messages_tx.clone()))
        };
        fn spawn_link_data_forwarder(
            mut link_rx: broadcast::Receiver<LinkEventData>,
            received_data_tx: broadcast::Sender<ReceivedData>,
        ) {
            tokio::spawn(async move {
                loop {
                    match link_rx.recv().await {
                        Ok(event) => {
                            if let LinkEvent::Data(payload) = event.event {
                                let _ = received_data_tx.send(ReceivedData {
                                    destination: event.address_hash,
                                    data: PacketDataBuffer::new_from_slice(payload.as_slice()),
                                    payload_mode: ReceivedPayloadMode::FullWire,
                                    ratchet_used: false,
                                    context: Some(payload.context()),
                                    request_id: payload.request_id(),
                                    hops: None,
                                    interface: None,
                                });
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    }
                }
            });
        }
        {
            spawn_link_data_forwarder(link_in_event_tx.subscribe(), received_data_tx.clone());
            spawn_link_data_forwarder(link_out_event_tx.subscribe(), received_data_tx.clone());
        }

        Self {
            name,
            iface_manager,
            link_in_event_tx,
            link_out_event_tx,
            received_data_tx,
            iface_messages_tx,
            resource_events_tx,
            handler,
            cancel,
        }
    }

    pub async fn outbound(&self, packet: &Packet) {
        let (packet, maybe_iface) = self.handler.lock().await.path_table.handle_packet(packet);

        if let Some(iface) = maybe_iface {
            self.send_direct(iface, packet).await;
            log::trace!("Sent outbound packet to {}", iface);
        }
        if maybe_iface.is_none() {
            let handler = self.handler.lock().await;
            if handler.config.broadcast {
                handler.send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet }).await;
            } else {
                log::trace!(
                    "tp({}): no route for outbound packet dst={}",
                    self.name,
                    packet.destination
                );
            }
        }
    }

    pub fn iface_manager(&self) -> Arc<Mutex<InterfaceManager>> {
        self.iface_manager.clone()
    }

    pub fn iface_rx(&self) -> broadcast::Receiver<RxMessage> {
        self.iface_messages_tx.subscribe()
    }

    pub fn resource_events(&self) -> broadcast::Receiver<ResourceEvent> {
        self.resource_events_tx.subscribe()
    }

    pub async fn recv_announces(&self) -> broadcast::Receiver<AnnounceEvent> {
        self.handler.lock().await.announce_tx.subscribe()
    }

    pub async fn send_packet(&self, packet: Packet) {
        let mut handler = self.handler.lock().await;
        handler.send_packet(packet).await;
    }

    pub async fn send_packet_with_outcome(&self, packet: Packet) -> SendPacketOutcome {
        let mut handler = self.handler.lock().await;
        handler.send_packet_with_outcome(packet).await
    }

    pub async fn send_packet_with_trace(&self, packet: Packet) -> SendPacketTrace {
        let mut handler = self.handler.lock().await;
        handler.send_packet_with_trace(packet).await
    }

    pub async fn send_announce(
        &self,
        destination: &Arc<Mutex<SingleInputDestination>>,
        app_data: Option<&[u8]>,
    ) {
        let mut destination = destination.lock().await;
        eprintln!(
            "[tp] announce_tx dst={} app_data_len={}",
            destination.desc.address_hash,
            app_data.map(|value| value.len()).unwrap_or(0)
        );
        let packet = destination.announce(OsRng, app_data).expect("valid announce packet");
        let mut handler = self.handler.lock().await;
        handler.send_packet(packet).await;
    }

    pub async fn set_receipt_handler(&mut self, handler: Box<dyn ReceiptHandler>) {
        self.handler.lock().await.receipt_handler = Some(Arc::from(handler));
    }

    pub fn emit_receipt_for_test(&self, receipt: DeliveryReceipt) {
        let receipt_handler =
            self.handler.try_lock().ok().and_then(|handler| handler.receipt_handler.clone());

        if let Some(handler) = receipt_handler {
            handler.on_receipt(&receipt);
        }
    }

    pub async fn handle_inbound_for_test(&self, packet: Packet) {
        let (receipt, receipt_handler) = {
            let mut handler = self.handler.lock().await;
            let receipt = handle_inbound_packet_for_test(&packet, &mut handler);
            let receipt_handler = handler.receipt_handler.clone();
            (receipt, receipt_handler)
        };

        if let (Some(receipt), Some(handler)) = (receipt, receipt_handler) {
            handler.on_receipt(&receipt);
        }
    }

    pub async fn send_broadcast(&self, packet: Packet, from_iface: Option<AddressHash>) {
        self.handler
            .lock()
            .await
            .send(TxMessage { tx_type: TxMessageType::Broadcast(from_iface), packet })
            .await;
    }

    pub async fn send_direct(&self, addr: AddressHash, packet: Packet) {
        self.handler
            .lock()
            .await
            .send(TxMessage { tx_type: TxMessageType::Direct(addr), packet })
            .await;
    }
}
