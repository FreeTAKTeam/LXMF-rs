use super::*;
use rns_transport::destination::link::{Link, LinkStatus};
use rns_transport::destination::SingleInputDestination;
use rns_transport::transport::SendPacketOutcome;

pub(super) struct NativeLink {
    pub label: String,
    pub link: Arc<tokio::sync::Mutex<Link>>,
    pub id: AddressHash,
    activation: Option<Packet>,
    old_signal: Option<Packet>,
    pub creator: Option<usize>,
}
pub(super) struct Wire {
    pub sender: Arc<Transport>,
    peer: Transport,
    sending: InterfaceChannel,
    receiving: InterfaceChannel,
    destination: Arc<tokio::sync::Mutex<SingleInputDestination>>,
    pub descriptor: DestinationDesc,
    pub links: Vec<NativeLink>,
    proofs: Vec<Packet>,
    held_resources: Vec<Packet>,
    resource_complete: bool,
    resource_proofs: usize,
    pub packet_handoffs: usize,
    pub resource_handoffs: usize,
    pub received: Vec<Vec<u8>>,
    pub resource_received: Vec<Vec<u8>>,
    resources: tokio::sync::broadcast::Receiver<rns_transport::resource::ResourceEvent>,
    data: tokio::sync::broadcast::Receiver<rns_transport::transport::ReceivedData>,
}
impl Wire {
    pub async fn new(sender: Arc<Transport>, propagated: bool) -> Self {
        let peer =
            Transport::new(TransportConfig::new("reference-trace-peer", &identity(0x22), true));
        let destination = peer
            .add_destination(
                identity(0x22),
                DestinationName::new("lxmf", if propagated { "propagation" } else { "delivery" }),
            )
            .await;
        let descriptor = destination.lock().await.desc;
        let sending =
            sender.iface_manager().lock().await.new_channel_with_role(4096, IfaceRole::Unicast);
        let receiving =
            peer.iface_manager().lock().await.new_channel_with_role(4096, IfaceRole::Unicast);
        let data = peer.received_data_events();
        let resources = peer.resource_events();
        Self {
            sender,
            peer,
            sending,
            receiving,
            destination,
            descriptor,
            links: Vec::new(),
            proofs: Vec::new(),
            held_resources: Vec::new(),
            resource_complete: false,
            resource_proofs: 0,
            packet_handoffs: 0,
            resource_handoffs: 0,
            received: Vec::new(),
            resource_received: Vec::new(),
            resources,
            data,
        }
    }
    async fn to_sender(&self, packet: Packet) {
        self.sending
            .rx_channel
            .send(RxMessage { address: *self.sending.address(), packet, source: IfaceSource::None })
            .await
            .expect("native sender ingress");
    }
    async fn to_peer(&self, packet: Packet) {
        self.receiving
            .rx_channel
            .send(RxMessage {
                address: *self.receiving.address(),
                packet,
                source: IfaceSource::None,
            })
            .await
            .expect("native peer ingress");
    }
    pub async fn set_path(&mut self, known: bool) {
        if known {
            let announce =
                self.destination.lock().await.announce(OsRng, None).expect("real signed announce");
            self.to_sender(announce).await;
            for _ in 0..10000 {
                if self.sender.has_path(&self.descriptor.address_hash).await {
                    return;
                }
                tokio::task::yield_now().await;
            }
            panic!("signed path was not learned");
        } else {
            self.sender.expire_path(&self.descriptor.address_hash).await;
            assert!(!self.sender.has_path(&self.descriptor.address_hash).await);
        }
    }
    pub async fn pump(&mut self) {
        for _ in 0..32 {
            while let Ok(frame) = self.sending.tx_channel.try_recv() {
                let packet = frame.packet;
                if packet.header.packet_type == PacketType::LinkRequest {
                    let id = AddressHash::from(&packet);
                    if !self.links.iter().any(|l| l.id == id) {
                        let link = self
                            .sender
                            .find_out_link(&id)
                            .await
                            .expect("published native outbound Link");
                        self.links.push(NativeLink {
                            label: format!("outbound-{}", self.links.len() + 1),
                            link,
                            id,
                            activation: None,
                            old_signal: None,
                            creator: None,
                        });
                    }
                }
                // Path presence is a declared oracle input. Discovery frames
                // are observed at their real producer but not answered implicitly.
                if packet.header.packet_type != PacketType::LinkRequest
                    && packet.header.destination_type
                        != rns_transport::packet::DestinationType::Link
                {
                    continue;
                }
                if packet.header.packet_type == PacketType::Data
                    && packet.context == PacketContext::None
                    && packet.header.destination_type
                        == rns_transport::packet::DestinationType::Link
                {
                    self.packet_handoffs += 1;
                }
                if packet.context == PacketContext::ResourceAdvrtisement {
                    self.resource_handoffs += 1;
                }
                self.to_peer(packet).await;
            }
            while let Ok(frame) = self.receiving.tx_channel.try_recv() {
                let packet = frame.packet;
                match packet.context {
                    PacketContext::LinkRequestProof => {
                        let id = packet.destination;
                        self.links
                            .iter_mut()
                            .find(|link| link.id == id)
                            .expect("activation target")
                            .activation = Some(packet);
                    }
                    PacketContext::None | PacketContext::LinkProof
                        if packet.header.packet_type == PacketType::Proof =>
                    {
                        self.proofs.push(packet)
                    }
                    PacketContext::ResourceRequest
                    | PacketContext::ResourceProof
                    | PacketContext::ResourceHashUpdate
                        if !self.resource_complete =>
                    {
                        self.held_resources.push(packet)
                    }
                    PacketContext::ResourceProof => {
                        self.resource_proofs += 1;
                        self.to_sender(packet).await;
                    }
                    _ => self.to_sender(packet).await,
                }
            }
            while let Ok(data) = self.data.try_recv() {
                if data.context == Some(PacketContext::None) {
                    self.received.push(data.data.as_slice().to_vec());
                }
            }
            while let Ok(event) = self.resources.try_recv() {
                if let rns_transport::resource::ResourceEventKind::Complete(value) = event.kind {
                    self.resource_received.push(value.data);
                }
            }
            tokio::task::yield_now().await;
        }
    }
    pub async fn activate_latest(&mut self) {
        self.pump().await;
        let selected = self
            .sender
            .delivery_link(&self.descriptor.address_hash)
            .await
            .expect("native selected Link");
        let index = self
            .links
            .iter()
            .position(|l| Arc::ptr_eq(&l.link, &selected))
            .expect("selected native Link recorded");
        // Cancellation can retire the owner while its pending native Link remains.
        if self.links[index].link.lock().await.status() == LinkStatus::Active {
            return;
        }
        let proof = self.links[index].activation.take().expect("real pending handshake proof");
        self.to_sender(proof).await;
        self.pump().await;
        assert_eq!(self.links[index].link.lock().await.status(), LinkStatus::Active);
        let remote =
            self.peer.find_in_link(&self.links[index].id).await.expect("recipient native Link");
        assert_eq!(remote.lock().await.status(), LinkStatus::Active);
        self.links[index].old_signal = Some(
            remote
                .lock()
                .await
                .data_packet(&[0x91, 0xcc, 245])
                .expect("authenticated original-generation signal"),
        );
    }
    pub async fn close_latest(&mut self) {
        let link = self
            .sender
            .delivery_link(&self.descriptor.address_hash)
            .await
            .expect("native selected Link to close");
        // Explicit native close input, matching the oracle. Resource failure is
        // injected separately through Transport cleanup, never a fabricated result.
        link.lock().await.close();
        assert_eq!(link.lock().await.status(), LinkStatus::Closed);
    }
    pub async fn prove(&mut self, index: usize) {
        self.pump().await;
        self.to_sender(self.proofs.get(index).expect("native recipient packet proof").clone())
            .await;
        self.pump().await;
    }
    pub async fn resource(&mut self, outcome: &str) {
        match outcome {
            "complete" => {
                let before = self.resource_proofs;
                self.resource_complete = true;
                for packet in std::mem::take(&mut self.held_resources) {
                    self.to_sender(packet).await;
                }
                for _ in 0..500 {
                    self.pump().await;
                    if self.resource_proofs > before {
                        self.resource_complete = false;
                        return;
                    }
                }
                panic!("native Resource did not complete");
            }
            "failed" => {
                let link = self.links.last().expect("Resource Link").link.clone();
                self.sender
                    .teardown_link(&link)
                    .await
                    .expect("native Resource failure via teardown");
                self.pump().await;
                self.held_resources.clear();
            }
            "rejected" => {
                // RCL carries the advertised hash, encrypted by the recipient's
                // native Link; the sender's ResourceManager publishes Rejected.
                let request = self.held_resources.first().expect("native Resource request");
                let remote =
                    self.peer.find_in_link(&request.destination).await.expect("recipient Link");
                let mut plain = vec![0; 500];
                let decoded = remote
                    .lock()
                    .await
                    .decrypt(request.data.as_slice(), &mut plain)
                    .expect("request decrypt")
                    .to_vec();
                let request =
                    rns_transport::resource::ResourceRequest::decode(&decoded).expect("request");
                let mut packet = remote
                    .lock()
                    .await
                    .data_packet(request.resource_hash.as_slice())
                    .expect("encrypted RCL");
                packet.context = PacketContext::ResourceReceiverCancel;
                self.to_sender(packet).await;
                self.pump().await;
                self.held_resources.clear();
            }
            input => panic!("unsupported native Resource input {input}"),
        }
    }
    pub async fn signal(&mut self, index: usize, data: &[u8]) {
        let native = &self.links[index];
        let id = native.id;
        let closed = native.link.lock().await.status() == LinkStatus::Closed;
        let packet = if closed {
            assert_eq!(data, [0x91, 0xcc, 245]);
            native.old_signal.clone().expect("old authenticated signal captured before close")
        } else {
            let remote = self.peer.find_in_link(&native.id).await.expect("recipient signal Link");
            let packet =
                remote.lock().await.data_packet(data).expect("authenticated native signal");
            packet
        };
        let mut events = self.sender.out_link_events();
        self.to_sender(packet).await;
        if !closed {
            // The oracle invokes its authenticated data callback before the next
            // processing turn. Queueing an RX frame is not that boundary: native
            // signal observation precedes this exact decrypted Data event.
            for _ in 0..10000 {
                loop {
                    let event = match events.try_recv() {
                        Ok(event) => event,
                        Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
                        Err(error) => panic!("native signal input observation failed: {error}"),
                    };
                    if let rns_transport::destination::link::LinkEvent::Data(payload) = event.event
                    {
                        if event.id == id
                            && payload.context() == PacketContext::None
                            && payload.as_slice() == data
                        {
                            self.pump().await;
                            return;
                        }
                    }
                }
                tokio::task::yield_now().await;
            }
            panic!("authenticated signal input did not reach its native Link callback");
        }
        // A Closed generation has already dropped its signal producer and
        // cannot affect a replacement owner, even if the old frame arrives later.
        self.pump().await;
    }
    pub async fn initial_links(
        &mut self,
        initial: Option<&str>,
        cache: &Arc<tokio::sync::Mutex<Option<crate::bridge::propagation::CachedPropagationLink>>>,
    ) {
        let back_only = initial == Some("backchannel");
        let pending_back = initial == Some("pending_and_backchannel");
        let both = initial == Some("both_active");
        let active = initial == Some("active");
        assert!(
            initial.is_none() || active || back_only || pending_back || both,
            "unsupported initial Link input"
        );
        if active || both || pending_back {
            let link = self.sender.link(self.descriptor).await;
            self.pump().await;
            self.links.last_mut().expect("preexisting outbound").label = "existing-outbound".into();
            if !pending_back {
                self.activate_latest().await;
            }
            if self.descriptor.name.as_name_hash_slice()
                == DestinationName::new("lxmf", "propagation").as_name_hash_slice()
            {
                *cache.lock().await = Some(crate::bridge::propagation::CachedPropagationLink {
                    node_hex: self.descriptor.address_hash.to_hex_string(),
                    link,
                });
            }
        }
        if back_only || both || pending_back {
            self.backchannel().await;
        }
    }
    async fn backchannel(&mut self) {
        let local = self
            .sender
            .add_destination(identity(0x11), DestinationName::new("lxmf", "delivery"))
            .await;
        let descriptor = local.lock().await.desc;
        let remote = self.peer.link(descriptor).await;
        let request =
            next_frame(&mut self.receiving, PacketType::LinkRequest, None, "inbound request").await;
        assert_eq!(request.header.packet_type, PacketType::LinkRequest);
        self.to_sender(request).await;
        let proof = next_frame(
            &mut self.sending,
            PacketType::Proof,
            Some(PacketContext::LinkRequestProof),
            "inbound handshake proof",
        )
        .await;
        self.to_peer(proof).await;
        let rtt = next_frame(
            &mut self.receiving,
            PacketType::Data,
            Some(PacketContext::LinkRTT),
            "inbound RTT",
        )
        .await;
        self.to_sender(rtt).await;
        let id = *remote.lock().await.id();
        let identification = {
            let link = remote.lock().await;
            link.identify_packet(&link.identify_payload(&identity(0x22)))
                .expect("native peer identification")
        };
        assert_eq!(
            self.peer.send_link_packet_on_bound_iface(&remote, identification).await,
            SendPacketOutcome::SentDirect
        );
        let identified = next_frame(
            &mut self.receiving,
            PacketType::Data,
            Some(PacketContext::LinkIdentify),
            "native identification",
        )
        .await;
        assert_eq!(identified.context, PacketContext::LinkIdentify);
        self.to_sender(identified).await;
        let link = self.sender.find_in_link(&id).await.expect("native inbound Link");
        for _ in 0..10000 {
            if link.lock().await.identified_peer_identity().is_some() {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(
            link.lock().await.identified_peer_identity().expect("verified peer").address_hash,
            *identity(0x22).address_hash()
        );
        self.links.push(NativeLink {
            label: "existing-backchannel".into(),
            link,
            id,
            activation: None,
            old_signal: None,
            creator: None,
        });
    }
}

async fn next_frame(
    channel: &mut InterfaceChannel,
    kind: PacketType,
    context: Option<PacketContext>,
    label: &str,
) -> Packet {
    for _ in 0..10000 {
        if let Ok(frame) = channel.tx_channel.try_recv() {
            if frame.packet.header.packet_type == kind
                && context.is_none_or(|context| frame.packet.context == context)
            {
                return frame.packet;
            }
        }
        tokio::task::yield_now().await;
    }
    panic!("bounded native frame wait: {label}");
}
