use super::*;
use crate::destination::link::{RequestFailure, RequestSource, RequestStatus};

fn link_pair(iface: AddressHash, peer_iface: AddressHash) -> (Link, Link) {
    let signer = PrivateIdentity::new_from_rand(OsRng);
    let destination = SingleInputDestination::new(signer, DestinationName::new("lxmf", "requests"));
    let events = broadcast::channel(8).0;
    let mut local = Link::new(destination.desc, events.clone());
    let request = local.request();
    let mut peer =
        Link::new_from_request(&request, destination.sign_key().clone(), destination.desc, events)
            .expect("native responder");
    assert!(matches!(local.handle_packet(&peer.prove(), iface), LinkHandleResult::Activated));
    peer.set_ingress_iface(peer_iface);
    peer.handle_packet(&local.create_rtt(), peer_iface);
    (local, peer)
}

#[tokio::test]
async fn cancelled_resource_publisher_retains_native_request_and_completion_outcomes() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("request-queue", &identity, true));
    let iface = AddressHash::new([71; 16]);
    let (mut link, _) = link_pair(iface, iface);
    let link_id = *link.id();
    let mut handler = transport.handler.lock().await;
    let prepared =
        ResourceManager::prepare_send(&link, vec![1; 1_000], None, Some(vec![3; 16]), false, 500)
            .expect("request Resource");
    let (hash, _) = handler.resource_manager.track_prepared(prepared);
    let mut completion = handler.resource_manager.observe_outbound(hash);
    let (mut receipt, mut dispatch) = link
        .register_request(
            vec![3; 16],
            RequestSource::Resource(hash),
            Some(Duration::from_secs(1)),
            None,
        )
        .expect("native request owner");
    dispatch.confirm_handoff(true);
    dispatch.returned();
    let link = Arc::new(Mutex::new(link));
    handler.in_links.insert(link_id, link.clone());
    handler.resource_manager.confirm_outbound_dispatch(hash, false);
    drop(handler);
    let mut legacy = transport.resource_events();
    let owner_guard = link.lock().await;
    let handler = transport.handler.clone();
    let (entered, entered_rx) = tokio::sync::oneshot::channel();
    let publisher = tokio::spawn(async move {
        let mut handler = handler.lock().await;
        entered.send(()).expect("test observer");
        super::super::resource_wire::publish_resource_events(&mut handler).await;
    });
    entered_rx.await.expect("publisher acquired handler");
    tokio::task::yield_now().await;
    publisher.abort();
    assert!(publisher.await.expect_err("cancelled publisher").is_cancelled());
    assert_eq!(receipt.status(), RequestStatus::Sent);
    assert_eq!(completion.current(), None);
    assert_eq!(
        transport.handler.lock().await.resource_manager.pending_event_link_id(),
        Some(link_id)
    );
    drop(owner_guard);
    super::super::jobs::handle_check_links(transport.handler.lock().await).await;
    assert_eq!(receipt.wait().await, Err(RequestFailure::TransferFailed));
    assert_eq!(
        completion.wait().await.expect("retained completion"),
        crate::resource::ResourceOutcome::Failed
    );
    assert!(matches!(
        legacy.try_recv().expect("one legacy outcome").kind,
        ResourceEventKind::OutboundFailed
    ));
    assert!(legacy.try_recv().is_err());
    assert!(transport.handler.lock().await.resource_manager.pending_event_link_id().is_none());
}

struct NativePair {
    local: Arc<Transport>,
    peer: Arc<Transport>,
    link: Arc<Mutex<Link>>,
    peer_link: Arc<Mutex<Link>>,
    to_peer: crate::iface::InterfaceChannel,
    to_local: crate::iface::InterfaceChannel,
}

impl NativePair {
    async fn new(capacity: usize) -> Self {
        let identity = PrivateIdentity::new_from_rand(OsRng);
        let local = Arc::new(Transport::new(TransportConfig::new("requester", &identity, true)));
        let peer = Arc::new(Transport::new(TransportConfig::new("responder", &identity, true)));
        let to_peer =
            local.iface_manager.lock().await.new_channel_with_role(capacity, IfaceRole::Unicast);
        let to_local =
            peer.iface_manager.lock().await.new_channel_with_role(capacity, IfaceRole::Unicast);
        let (link, peer_link) = link_pair(*to_peer.address(), *to_local.address());
        let id = *link.id();
        let destination = link.destination().address_hash;
        let link = Arc::new(Mutex::new(link));
        let peer_link = Arc::new(Mutex::new(peer_link));
        local.handler.lock().await.out_links.insert(destination, link.clone());
        peer.handler.lock().await.in_links.insert(id, peer_link.clone());
        Self { local, peer, link, peer_link, to_peer, to_local }
    }

    async fn deliver(transport: &Transport, packet: Packet, iface: AddressHash) {
        timeout(Duration::from_secs(2), async {
            if packet.header.packet_type == PacketType::Proof {
                handle_proof(packet, transport.handler.clone(), iface).await;
            } else {
                handle_data(&packet, iface, transport.handler.lock().await).await;
            }
        })
        .await
        .expect("native wire handler must return without a lock cycle");
    }

    async fn pump(&mut self) {
        for _ in 0..1_000 {
            let mut progressed = false;
            while let Ok(message) = self.to_peer.tx_channel.try_recv() {
                Self::deliver(&self.peer, message.packet, *self.to_local.address()).await;
                progressed = true;
            }
            while let Ok(message) = self.to_local.tx_channel.try_recv() {
                Self::deliver(&self.local, message.packet, *self.to_peer.address()).await;
                progressed = true;
            }
            if !progressed {
                return;
            }
        }
        panic!("native request exchange did not quiesce");
    }

    async fn fill_outbound(&self) {
        let packet = self.link.lock().await.data_packet(b"queue blocker").expect("queue filler");
        assert_eq!(
            self.local.send_link_packet_on_bound_iface(&self.link, packet).await,
            SendPacketOutcome::SentDirect
        );
    }
}

fn packed_response(id: &[u8], body: Vec<u8>) -> Vec<u8> {
    let mut data = Vec::new();
    rmpv::encode::write_value(
        &mut data,
        &rmpv::Value::Array(vec![rmpv::Value::Binary(id.to_vec()), rmpv::Value::Binary(body)]),
    )
    .expect("response envelope");
    data
}

#[tokio::test]
async fn unaccepted_response_advertisements_cannot_adopt_an_ordinary_transfer() {
    use crate::resource::{build_link_packet, ResourceAdvertisement};
    for rejected in ["duplicate", "invalid", "out_of_order"] {
        let mut pair = NativePair::new(128).await;
        let request = Link::request_payload("/unrelated", rmpv::Value::Nil).expect("request");
        let mut receipt = pair
            .local
            .send_request_on_link(&pair.link, request, Duration::from_secs(5), None)
            .await
            .expect("owned request");
        pair.pump().await;
        let data = b"ordinary Resource must survive an unrelated request".to_vec();
        let ordinary_hash = pair
            .peer
            .send_resource(&receipt.link_id(), data.clone(), Some(vec![7]))
            .await
            .expect("ordinary Resource");
        let ordinary = pair.to_local.tx_channel.try_recv().expect("ordinary advertisement").packet;
        let mut buffer = PacketDataBuffer::new();
        let mut advertisement = {
            let link = pair.link.lock().await;
            let plain = link
                .decrypt(ordinary.data.as_slice(), buffer.accuire_buf_max())
                .expect("decrypt advertisement");
            ResourceAdvertisement::unpack(plain).expect("advertisement")
        };
        NativePair::deliver(&pair.local, ordinary, *pair.to_peer.address()).await;
        advertisement.flags |= 0x10;
        advertisement.request_id = Some(receipt.request_id().to_vec().into());
        if rejected != "duplicate" {
            advertisement.hash = Hash::new_from_slice(rejected.as_bytes());
        }
        if rejected == "out_of_order" {
            advertisement.flags |= 0x04;
            advertisement.total_segments = 2;
            advertisement.segment_index = 2;
        }
        let forged = build_link_packet(
            &*pair.peer_link.lock().await,
            PacketType::Data,
            PacketContext::ResourceAdvrtisement,
            &advertisement.pack().expect("relabelled advertisement"),
        )
        .expect("authenticated advertisement");
        NativePair::deliver(&pair.local, forged, *pair.to_peer.address()).await;
        assert_eq!(
            receipt.status(),
            RequestStatus::Sent,
            "unaccepted {rejected} cannot start a response"
        );
        assert!(pair.local.cancel_request_on_link(&pair.link, &receipt).await);
        assert_eq!(receipt.wait().await, Err(RequestFailure::Cancelled));
        assert!(
            pair.local
                .handler
                .lock()
                .await
                .resource_manager
                .has_incoming_resource(receipt.link_id(), ordinary_hash),
            "cleanup of {rejected} must preserve the ordinary receiver"
        );
        let mut events = pair.local.resource_events();
        pair.pump().await;
        let mut completed = Vec::new();
        while let Ok(event) = events.try_recv() {
            if let ResourceEventKind::Complete(resource) = event.kind {
                completed.push((event.hash, resource.data));
            }
        }
        assert_eq!(completed, vec![(ordinary_hash, data)]);
    }
}

#[tokio::test]
async fn duplicate_response_cannot_be_rebound_to_another_request() {
    use crate::resource::{build_link_packet, ResourceAdvertisement};
    let mut pair = NativePair::new(128).await;
    let mut receipts = Vec::new();
    for path in ["/first", "/other"] {
        receipts.push(
            pair.local
                .send_request_on_link(
                    &pair.link,
                    Link::request_payload(path, rmpv::Value::Nil).expect("request"),
                    Duration::from_secs(5),
                    None,
                )
                .await
                .expect("owned request"),
        );
    }
    pair.pump().await;
    let body = packed_response(receipts[1].request_id(), b"belongs to other request".to_vec());
    pair.peer
        .send_response_resource(
            &receipts[1].link_id(),
            receipts[1].request_id().to_vec(),
            body.clone(),
            None,
        )
        .await
        .expect("other response");
    let advertisement = pair.to_local.tx_channel.try_recv().expect("response advertisement").packet;
    let mut buffer = PacketDataBuffer::new();
    let mut relabelled = {
        let link = pair.link.lock().await;
        let plain =
            link.decrypt(advertisement.data.as_slice(), buffer.accuire_buf_max()).expect("decrypt");
        ResourceAdvertisement::unpack(plain).expect("advertisement")
    };
    NativePair::deliver(&pair.local, advertisement, *pair.to_peer.address()).await;
    assert_eq!(receipts[1].status(), RequestStatus::Receiving);
    relabelled.request_id = Some(receipts[0].request_id().to_vec().into());
    let packet = build_link_packet(
        &*pair.peer_link.lock().await,
        PacketType::Data,
        PacketContext::ResourceAdvrtisement,
        &relabelled.pack().expect("relabelled response"),
    )
    .expect("packet");
    NativePair::deliver(&pair.local, packet, *pair.to_peer.address()).await;
    assert_eq!(receipts[0].status(), RequestStatus::Sent);
    assert!(pair.local.cancel_request_on_link(&pair.link, &receipts[0]).await);
    pair.pump().await;
    assert_eq!(receipts[1].wait().await.expect("other request completes").data, body);
}

#[tokio::test]
async fn cancellation_commit_can_abandon_optional_io_on_a_full_live_queue() {
    use std::future::Future;
    let mut pair = NativePair::new(2).await;
    let request = Link::request_payload("/full", rmpv::Value::Nil).expect("request");
    let mut receipt = pair
        .local
        .send_request_on_link(&pair.link, request, Duration::from_secs(5), None)
        .await
        .expect("request");
    pair.pump().await;
    let hash = pair
        .peer
        .send_response_resource(
            &receipt.link_id(),
            receipt.request_id().to_vec(),
            packed_response(receipt.request_id(), vec![7; 1_000]),
            None,
        )
        .await
        .expect("response");
    let advertisement = pair.to_local.tx_channel.try_recv().expect("advertisement").packet;
    NativePair::deliver(&pair.local, advertisement, *pair.to_peer.address()).await;
    assert_eq!(receipt.status(), RequestStatus::Receiving);
    let ordinary_data = b"unrelated transfer survives the full queue".to_vec();
    let ordinary_hash = pair
        .peer
        .send_resource(&receipt.link_id(), ordinary_data.clone(), None)
        .await
        .expect("unrelated Resource");
    let ordinary_adv = pair.to_local.tx_channel.try_recv().expect("ordinary advertisement").packet;
    NativePair::deliver(&pair.local, ordinary_adv, *pair.to_peer.address()).await;
    let (committed, mut commit) = tokio::sync::oneshot::channel();
    let mut cancellation =
        Box::pin(pair.local.cancel_request_on_link_observed(&pair.link, &receipt, |cancelled| {
            assert!(pair.link.try_lock().is_ok(), "observer holds no Link lock");
            assert!(pair.local.handler.try_lock().is_ok(), "observer holds no handler lock");
            committed.send(cancelled).expect("local completion latch");
        }));
    std::future::poll_fn(|cx| {
        assert!(
            cancellation.as_mut().poll(cx).is_pending(),
            "optional frame is blocked on a full live queue"
        );
        std::task::Poll::Ready(())
    })
    .await;
    assert!(commit.try_recv().expect("native cleanup already committed"));
    drop(cancellation);
    assert_eq!(receipt.wait().await, Err(RequestFailure::Cancelled));
    assert!(!pair
        .local
        .handler
        .lock()
        .await
        .resource_manager
        .has_incoming_resource(receipt.link_id(), hash));
    assert!(pair
        .local
        .handler
        .lock()
        .await
        .resource_manager
        .has_incoming_resource(receipt.link_id(), ordinary_hash));
    assert_eq!(
        pair.to_peer.tx_channel.try_recv().expect("original queued request").packet.context,
        PacketContext::ResourceRequest
    );
    let ordinary_request =
        pair.to_peer.tx_channel.try_recv().expect("ordinary queued request").packet;
    assert_eq!(ordinary_request.context, PacketContext::ResourceRequest);
    assert!(
        pair.to_peer.tx_channel.try_recv().is_err(),
        "dropping optional I/O cannot invent a frame handoff"
    );
    let mut events = pair.local.resource_events();
    NativePair::deliver(&pair.peer, ordinary_request, *pair.to_local.address()).await;
    pair.pump().await;
    let mut completed = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let ResourceEventKind::Complete(complete) = event.kind {
            completed.push((event.hash, complete.data));
        }
    }
    assert_eq!(completed, vec![(ordinary_hash, ordinary_data)]);
}

#[tokio::test]
async fn native_request_packet_and_resource_cross_both_response_representations() {
    for resource_request in [false, true] {
        for resource_response in [false, true] {
            let mut pair = NativePair::new(128).await;
            let payload = if resource_request { vec![3; 2_000] } else { vec![3] };
            let request =
                Link::request_payload("/native", rmpv::Value::Binary(payload)).expect("request");
            let request_body = request.packed.clone();
            let mut peer_resources = pair.peer.resource_events();
            let mut receipt = pair
                .local
                .send_request_on_link(&pair.link, request, Duration::from_secs(5), None)
                .await
                .expect("native request dispatch");
            assert_eq!(receipt.status(), RequestStatus::Sent);
            let first = pair.to_peer.tx_channel.try_recv().expect("initial request handoff").packet;
            assert_eq!(
                first.context,
                if resource_request {
                    PacketContext::ResourceAdvrtisement
                } else {
                    PacketContext::Request
                }
            );
            if !resource_request {
                assert_eq!(receipt.request_id(), &first.hash().to_bytes()[..16]);
            }
            NativePair::deliver(&pair.peer, first, *pair.to_local.address()).await;
            pair.pump().await;
            if resource_request {
                assert_eq!(receipt.status(), RequestStatus::Delivered);
                let mut complete = None;
                while let Ok(event) = peer_resources.try_recv() {
                    if let ResourceEventKind::Complete(resource) = event.kind {
                        complete = Some(resource);
                    }
                }
                let complete = complete.expect("actual request Resource completion");
                assert!(complete.is_request);
                assert_eq!(complete.request_id.as_deref(), Some(receipt.request_id()));
                assert_eq!(complete.data, request_body);
                assert_eq!(receipt.request_id(), crate::hash::address_hash(&request_body));
            } else {
                assert_eq!(receipt.status(), RequestStatus::Sent);
                assert!(
                    !receipt.current().packet_acknowledged,
                    "Request ingress does not manufacture a packet ACK"
                );
            }
            let body = if resource_response { vec![9; 2_000] } else { vec![9] };
            let response = packed_response(receipt.request_id(), body.clone());
            if resource_response {
                pair.peer
                    .send_response_resource(
                        &receipt.link_id(),
                        receipt.request_id().to_vec(),
                        response,
                        None,
                    )
                    .await
                    .expect("native response Resource");
            } else {
                let packet = pair
                    .peer_link
                    .lock()
                    .await
                    .response_packet(&response)
                    .expect("response packet");
                assert_eq!(
                    pair.peer.send_link_packet_on_bound_iface(&pair.peer_link, packet).await,
                    SendPacketOutcome::SentDirect
                );
            }
            pair.pump().await;
            assert_eq!(
                receipt.status(),
                RequestStatus::Ready,
                "native response reached its request owner"
            );
            let result =
                receipt.wait().await.expect("retained response without broadcast listeners");
            assert_eq!(result.is_resource, resource_response);
            let (id, response) =
                crate::destination::link::unpack_response_envelope(&result.data).expect("response");
            assert_eq!(id.as_slice(), receipt.request_id());
            assert_eq!(response.as_slice(), Some(body.as_slice()));
            super::super::jobs::handle_check_links(pair.local.handler.lock().await).await;
            assert!(pair.local.handler.lock().await.resource_manager.has_no_outbound_state());
        }
    }
}

#[tokio::test]
async fn early_native_response_survives_blocked_send_and_handoff_confirmation() {
    for resource in [false, true] {
        let mut pair = NativePair::new(1).await;
        pair.fill_outbound().await;
        let request = Link::request_payload(
            "/early",
            rmpv::Value::Binary(vec![4; if resource { 2_000 } else { 1 }]),
        )
        .expect("request");
        let (observed, observed_rx) = tokio::sync::oneshot::channel();
        let transport = pair.local.clone();
        let link = pair.link.clone();
        let send = tokio::spawn(async move {
            transport
                .send_request_on_link_observed(
                    &link,
                    request,
                    Duration::from_secs(5),
                    None,
                    |receipt| {
                        assert!(link.try_lock().is_ok(), "observer holds no Link guard");
                        assert!(
                            transport.handler.try_lock().is_ok(),
                            "observer holds no handler guard"
                        );
                        observed.send(receipt).expect("external observer");
                    },
                )
                .await
        });
        let mut receipt = observed_rx.await.expect("registration observed before blocked handoff");
        assert!(!send.is_finished());
        let response = pair
            .peer_link
            .lock()
            .await
            .response_packet(&packed_response(receipt.request_id(), b"early".to_vec()))
            .expect("early encrypted response");
        pair.link.lock().await.handle_packet(&response, *pair.to_peer.address());
        let before_return = receipt.current();
        assert_eq!(before_return.status, RequestStatus::Ready);
        pair.to_peer.tx_channel.recv().await.expect("release blocked handoff");
        let returned = send.await.expect("send task").expect("handoff");
        assert_eq!(
            returned.current(),
            before_return,
            "confirmation cannot overwrite an early terminal outcome"
        );
        assert_eq!(
            receipt.wait().await.expect("early outcome").data,
            before_return.response.expect("response").data
        );
        pair.to_peer.tx_channel.recv().await.expect("request handoff");
        // Successful early completion retires its own still-outgoing request Resource.
        super::super::jobs::handle_check_links(pair.local.handler.lock().await).await;
        assert!(pair.local.handler.lock().await.resource_manager.has_no_outbound_state());
    }
}

#[tokio::test]
async fn abandoning_blocked_request_send_seals_registration_and_scopes_native_cleanup() {
    for resource in [false, true] {
        let pair = NativePair::new(1).await;
        pair.fill_outbound().await;
        let request = Link::request_payload(
            "/abandoned",
            rmpv::Value::Binary(vec![5; if resource { 2_000 } else { 1 }]),
        )
        .expect("request");
        let (observed, observed_rx) = tokio::sync::oneshot::channel();
        let transport = pair.local.clone();
        let link = pair.link.clone();
        let send = tokio::spawn(async move {
            transport
                .send_request_on_link_observed(
                    &link,
                    request,
                    Duration::from_secs(5),
                    None,
                    |receipt| {
                        observed.send(receipt).expect("external observer");
                    },
                )
                .await
        });
        let mut receipt = observed_rx.await.expect("registration observed");
        send.abort();
        assert!(send.await.expect_err("abandoned send").is_cancelled());
        assert_eq!(receipt.wait().await, Err(RequestFailure::Cancelled));
        assert!(
            !pair.local.cancel_request_on_link(&pair.peer_link, &receipt).await,
            "another Arc with the same Link ID cannot cancel this registration"
        );
        let id = receipt.request_id().to_vec();
        super::super::jobs::handle_check_links(pair.local.handler.lock().await).await;
        assert!(pair.local.handler.lock().await.resource_manager.has_no_outbound_state());
        pair.link.lock().await.set_response_size_limit(&id, usize::MAX);
        assert!(
            !pair.link.lock().await.take_response_limit_if_allowed(&id, 0),
            "abandonment retains the terminal ID fence"
        );
    }
}

#[tokio::test]
async fn native_response_resource_limits_use_uncompressed_total_across_segments() {
    for resource_request in [false, true] {
        for split in [false, true] {
            for allowed in [false, true] {
                let mut pair = NativePair::new(128).await;
                let body =
                    vec![7; if split { crate::resource::MAX_EFFICIENT_SIZE + 40 } else { 2_000 }];
                // The envelope length is fixed for a 16-byte request ID.
                let response_size = packed_response(&[0; 16], body.clone()).len();
                let limit = response_size - usize::from(!allowed);
                let request = Link::request_payload(
                    "/bounded",
                    if resource_request {
                        rmpv::Value::Binary(vec![1; 2_000])
                    } else {
                        rmpv::Value::Nil
                    },
                )
                .expect("request");
                let mut receipt = pair
                    .local
                    .send_request_on_link(&pair.link, request, Duration::from_secs(5), Some(limit))
                    .await
                    .expect("bounded request");
                pair.pump().await;
                let response = packed_response(receipt.request_id(), body.clone());
                pair.peer
                    .send_response_resource(
                        &receipt.link_id(),
                        receipt.request_id().to_vec(),
                        response,
                        None,
                    )
                    .await
                    .expect("compressed native response");
                let first =
                    pair.to_local.tx_channel.try_recv().expect("response advertisement").packet;
                let mut plaintext = vec![0; first.data.len()];
                let owner = pair.link.lock().await;
                let advertisement = ResourceAdvertisement::unpack(
                    owner
                        .decrypt(first.data.as_slice(), &mut plaintext)
                        .expect("authenticated advertisement"),
                )
                .expect("advertisement fields");
                drop(owner);
                assert!(advertisement.compressed());
                assert_eq!(advertisement.data_size, response_size as u64);
                assert!(
                    advertisement.transfer_size < limit as u64,
                    "wire size alone would incorrectly admit oversized data"
                );
                assert_eq!(advertisement.total_segments > 1, split);
                NativePair::deliver(&pair.local, first, *pair.to_peer.address()).await;
                pair.pump().await;
                if allowed {
                    assert_eq!(receipt.status(), RequestStatus::Ready);
                    let response =
                        receipt.wait().await.expect("exact uncompressed boundary accepted");
                    let (_, value) =
                        crate::destination::link::unpack_response_envelope(&response.data)
                            .expect("full response");
                    assert_eq!(value.as_slice(), Some(body.as_slice()));
                } else {
                    assert_eq!(receipt.wait().await, Err(RequestFailure::Rejected));
                    let before = receipt.current();
                    let packet = pair
                        .peer_link
                        .lock()
                        .await
                        .response_packet(&packed_response(receipt.request_id(), vec![1]))
                        .expect("later valid-sized response");
                    NativePair::deliver(&pair.local, packet, *pair.to_peer.address()).await;
                    assert_eq!(
                        receipt.current(),
                        before,
                        "valid replay cannot reopen an oversized rejection"
                    );
                }
            }
        }
    }
}

#[tokio::test]
async fn receiving_request_ignores_pending_timeout_and_size_rejection_callbacks() {
    let mut pair = NativePair::new(128).await;
    let request = Link::request_payload("/receiving", rmpv::Value::Nil).expect("request");
    let mut receipt = pair
        .local
        .send_request_on_link(&pair.link, request, Duration::from_secs(5), Some(200))
        .await
        .expect("bounded request");
    pair.pump().await;
    let response = packed_response(receipt.request_id(), vec![8; 100]);
    pair.peer
        .send_response_resource(&receipt.link_id(), receipt.request_id().to_vec(), response, None)
        .await
        .expect("ordinary response Resource");
    let advertisement = pair.to_local.tx_channel.try_recv().expect("advertisement").packet;
    NativePair::deliver(&pair.local, advertisement, *pair.to_peer.address()).await;
    assert_eq!(receipt.status(), RequestStatus::Receiving);
    let before = receipt.current();
    let oversized = packed_response(receipt.request_id(), vec![9; 500]);
    // A larger Resource advertisement for the same request is rejected without
    // failing the already accepted response transfer.
    pair.peer
        .send_response_resource(&receipt.link_id(), receipt.request_id().to_vec(), oversized, None)
        .await
        .expect("second response advertisement");
    let rejected = pair.to_local.tx_channel.try_recv().expect("oversized advertisement").packet;
    NativePair::deliver(&pair.local, rejected, *pair.to_peer.address()).await;
    assert_eq!(receipt.current(), before);
    // Packet admission and the maintenance deadline preserve the same guard.
    let oversized_packet = pair
        .peer_link
        .lock()
        .await
        .response_packet(&packed_response(receipt.request_id(), vec![0; 210]))
        .expect("oversized packet response fits the negotiated MDU");
    NativePair::deliver(&pair.local, oversized_packet, *pair.to_peer.address()).await;
    assert_eq!(receipt.current(), before);
    assert!(pair
        .link
        .lock()
        .await
        .poll_requests(std::time::Instant::now() + Duration::from_secs(60))
        .is_empty());
    assert_eq!(receipt.current(), before);
    pair.pump().await;
    assert_eq!(
        receipt.status(),
        RequestStatus::Ready,
        "the originally accepted response still completes"
    );
    assert_eq!(
        receipt.wait().await.expect("original response").data.len(),
        packed_response(receipt.request_id(), vec![8; 100]).len()
    );
}

#[tokio::test]
async fn malformed_owned_resource_response_fails_instead_of_leaving_receiving_or_reopening() {
    for payload in [vec![0xff, 0xff], packed_response(&[99; 16], vec![1])] {
        let mut pair = NativePair::new(128).await;
        let request = Link::request_payload("/invalid", rmpv::Value::Nil).expect("request");
        let mut receipt = pair
            .local
            .send_request_on_link(&pair.link, request, Duration::from_secs(5), None)
            .await
            .expect("request");
        pair.pump().await;
        pair.peer
            .send_response_resource(
                &receipt.link_id(),
                receipt.request_id().to_vec(),
                payload,
                None,
            )
            .await
            .expect("authenticated invalid ordinary response");
        pair.pump().await;
        assert_eq!(receipt.wait().await, Err(RequestFailure::InvalidResponse));
        let terminal = receipt.current();
        let valid = pair
            .peer_link
            .lock()
            .await
            .response_packet(&packed_response(receipt.request_id(), vec![1]))
            .expect("late valid packet");
        NativePair::deliver(&pair.local, valid, *pair.to_peer.address()).await;
        assert_eq!(receipt.current(), terminal);
    }
}

#[tokio::test]
async fn closed_replacement_cleans_abandoned_prepared_request_and_preserves_unrelated_resource() {
    use std::future::Future;
    let mut pair = NativePair::new(1).await;
    pair.fill_outbound().await;
    let destination = *pair.link.lock().await.destination();
    let request =
        Link::request_payload("/replace", rmpv::Value::Binary(vec![7; 2_000])).expect("request");
    let mut observed = None;
    let mut sending = Box::pin(pair.local.send_request_on_link_observed(
        &pair.link,
        request,
        Duration::from_secs(5),
        None,
        |receipt| {
            observed = Some(receipt);
        },
    ));
    std::future::poll_fn(|cx| {
        assert!(sending.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    pair.link.lock().await.close();
    drop(sending);
    let mut receipt = observed.expect("registered before blocked handoff");
    assert_eq!(receipt.wait().await, Err(RequestFailure::LinkClosed));
    let mut handler = pair.local.handler.lock().await;
    assert!(
        !handler.resource_manager.has_no_outbound_state(),
        "old prepared request exists before replacement hook"
    );
    let (unrelated, _) = link_pair(*pair.to_peer.address(), *pair.to_local.address());
    let (unrelated_hash, _) = handler
        .resource_manager
        .start_send(&unrelated, vec![1], None)
        .expect("unrelated pending Resource");
    let mut unrelated_completion = handler.resource_manager.observe_outbound(unrelated_hash);
    assert!(handler.path_table.restore_tunnel_path(
        destination.address_hash,
        destination.address_hash,
        1,
        *pair.to_peer.address(),
        Hash::new_from_slice(b"replacement path"),
        std::time::Instant::now()
    ));
    drop(handler);
    pair.to_peer.tx_channel.try_recv().expect("release carrier blocker");
    let replacement = pair.local.link(destination).await;
    assert!(!Arc::ptr_eq(&replacement, &pair.link));
    let mut handler = pair.local.handler.lock().await;
    assert_eq!(
        unrelated_completion.current(),
        None,
        "replacement did not touch unrelated Resource"
    );
    handler.resource_manager.confirm_outbound_dispatch(unrelated_hash, false);
    super::super::resource_wire::publish_resource_events(&mut handler).await;
    assert_eq!(
        unrelated_completion.wait().await.expect("unrelated retained outcome"),
        crate::resource::ResourceOutcome::Failed
    );
    assert!(
        handler.resource_manager.has_no_outbound_state(),
        "old prepared request was removed before its Arc was detached"
    );
}

#[tokio::test]
async fn transport_drop_settles_live_requests_and_resources_with_retained_link_arcs() {
    for resource in [false, true] {
        let pair = NativePair::new(8).await;
        let request = Link::request_payload(
            "/shutdown",
            rmpv::Value::Binary(vec![1; if resource { 2_000 } else { 1 }]),
        )
        .expect("request");
        let mut receipt = pair
            .local
            .send_request_on_link(&pair.link, request, Duration::from_secs(60), None)
            .await
            .expect("live request");
        let handler = pair.local.handler.clone();
        let NativePair {
            local,
            link,
            peer: _peer,
            peer_link: _peer_link,
            to_peer: _to_peer,
            to_local: _to_local,
        } = pair;
        drop(local);
        assert_eq!(
            timeout(Duration::from_secs(2), receipt.wait())
                .await
                .expect("native shutdown settles retained observers"),
            Err(RequestFailure::LinkClosed)
        );
        assert_eq!(link.lock().await.status(), LinkStatus::Closed);
        let handler = handler.lock().await;
        assert!(handler.resource_manager.has_no_outbound_state());
        assert!(handler.out_links.is_empty());
    }
}

#[tokio::test]
async fn cancelling_receiving_request_preserves_unrelated_inbound_transfer_even_when_frame_handoff_fails(
) {
    for fail_frame in [false, true] {
        let mut pair = NativePair::new(128).await;
        let request = Link::request_payload("/cancel", rmpv::Value::Nil).expect("request");
        let mut receipt = pair
            .local
            .send_request_on_link(&pair.link, request, Duration::from_secs(5), None)
            .await
            .expect("request");
        pair.pump().await;
        let response_hash = pair
            .peer
            .send_response_resource(
                &receipt.link_id(),
                receipt.request_id().to_vec(),
                packed_response(receipt.request_id(), vec![1; 1_000]),
                None,
            )
            .await
            .expect("response Resource");
        let response_adv =
            pair.to_local.tx_channel.try_recv().expect("response advertisement").packet;
        NativePair::deliver(&pair.local, response_adv, *pair.to_peer.address()).await;
        let unrelated_data = b"independent ordinary transfer".to_vec();
        let unrelated_hash = pair
            .peer
            .send_resource(&receipt.link_id(), unrelated_data.clone(), Some(vec![7]))
            .await
            .expect("unrelated ordinary Resource");
        let unrelated_adv =
            pair.to_local.tx_channel.try_recv().expect("unrelated advertisement").packet;
        NativePair::deliver(&pair.local, unrelated_adv, *pair.to_peer.address()).await;
        let mut events = pair.local.resource_events();
        assert_eq!(receipt.status(), RequestStatus::Receiving);
        assert!(
            !pair.local.cancel_request_on_link(&pair.peer_link, &receipt).await,
            "another native owner with the same Link ID cannot cancel this request"
        );
        if fail_frame {
            pair.to_peer.tx_channel.close();
        }
        assert!(pair.local.cancel_request_on_link(&pair.link, &receipt).await);
        assert_eq!(receipt.wait().await, Err(RequestFailure::Cancelled));
        assert!(
            !pair.local.cancel_request_on_link(&pair.link, &receipt).await,
            "cancellation is singular"
        );
        {
            let handler = pair.local.handler.lock().await;
            assert!(!handler
                .resource_manager
                .has_incoming_resource(receipt.link_id(), response_hash));
            assert!(handler
                .resource_manager
                .has_incoming_resource(receipt.link_id(), unrelated_hash));
        }
        pair.pump().await;
        let mut completed = Vec::new();
        while let Ok(event) = events.try_recv() {
            if let ResourceEventKind::Complete(resource) = event.kind {
                completed.push((event.hash, resource));
            }
        }
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].0, unrelated_hash);
        assert_eq!(completed[0].1.data, unrelated_data);
        assert_eq!(
            receipt.status(),
            RequestStatus::Failed,
            "late cancelled-response parts cannot reopen a request"
        );
    }
}

#[tokio::test]
async fn request_handoff_clock_precedes_blocked_link_bookkeeping() {
    use std::future::Future;
    let mut pair = NativePair::new(1).await;
    pair.fill_outbound().await;
    let request = Link::request_payload("/clock", rmpv::Value::Nil).expect("request");
    let (observed, mut observed_rx) = tokio::sync::oneshot::channel();
    let mut sending = Box::pin(pair.local.send_request_on_link_observed(
        &pair.link,
        request,
        Duration::from_secs(5),
        None,
        |receipt| {
            observed.send(receipt).expect("external observer");
        },
    ));
    std::future::poll_fn(|cx| {
        assert!(sending.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    let receipt = observed_rx.try_recv().expect("observed before handoff");
    assert_eq!(receipt.current().sent_at, None);
    let locked_link = pair.link.lock().await;
    pair.to_peer.tx_channel.try_recv().expect("release carrier handoff");
    std::future::poll_fn(|cx| {
        assert!(sending.as_mut().poll(cx).is_pending(), "Link bookkeeping is still blocked");
        std::task::Poll::Ready(())
    })
    .await;
    let before_bookkeeping =
        receipt.current().sent_at.expect("carrier handoff already started the packet timer");
    drop(locked_link);
    let returned = sending.await.expect("request dispatch");
    assert_eq!(returned.current().sent_at, Some(before_bookkeeping));
}

#[tokio::test]
async fn restart_during_mtu_preparation_prevents_registration_and_dispatch() {
    use std::future::Future;
    let pair = NativePair::new(8).await;
    let request =
        Link::request_payload("/generation", rmpv::Value::Binary(vec![0; 2_000])).expect("request");
    let manager = pair.local.iface_manager();
    let blocked = manager.lock().await;
    let mut observed = false;
    let mut sending = Box::pin(pair.local.send_request_on_link_observed(
        &pair.link,
        request,
        Duration::from_secs(5),
        None,
        |_| {
            observed = true;
        },
    ));
    std::future::poll_fn(|cx| {
        assert!(sending.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    let before = *pair.link.lock().await.id();
    pair.link.lock().await.restart();
    pair.link.lock().await.request();
    assert_ne!(*pair.link.lock().await.id(), before);
    drop(blocked);
    assert!(matches!(sending.await, Err(RnsError::ConnectionError)));
    assert!(!observed);
    assert!(pair.local.handler.lock().await.resource_manager.has_no_outbound_state());
}

#[tokio::test]
async fn existing_link_maintenance_expires_sent_packet_and_delivered_resource_requests() {
    for resource in [false, true] {
        let mut pair = NativePair::new(128).await;
        let request = Link::request_payload(
            "/no-response",
            rmpv::Value::Binary(vec![1; if resource { 2_000 } else { 1 }]),
        )
        .expect("request");
        let mut receipt = pair
            .local
            .send_request_on_link(&pair.link, request, Duration::from_millis(100), None)
            .await
            .expect("request handoff");
        pair.pump().await;
        assert_eq!(
            receipt.status(),
            if resource { RequestStatus::Delivered } else { RequestStatus::Sent }
        );
        assert_eq!(
            timeout(Duration::from_secs(2), receipt.wait())
                .await
                .expect("existing maintenance owns the deadline"),
            Err(RequestFailure::Timeout)
        );
        assert_eq!(pair.link.lock().await.status(), LinkStatus::Active);
        assert!(pair.local.handler.lock().await.resource_manager.has_no_outbound_state());
    }
}

#[tokio::test]
async fn first_resource_request_waiting_at_handoff_sees_confirmed_sender_without_retry() {
    use std::future::Future;
    let mut pair = NativePair::new(1).await;
    pair.fill_outbound().await;
    let request =
        Link::request_payload("/fast-peer", rmpv::Value::Binary(vec![1; 2_000])).expect("request");
    let (observed, mut observed_rx) = tokio::sync::oneshot::channel();
    let mut sending = Box::pin(pair.local.send_request_on_link_observed(
        &pair.link,
        request,
        Duration::from_secs(5),
        None,
        |receipt| {
            observed.send(receipt).expect("external observer");
        },
    ));
    std::future::poll_fn(|cx| {
        assert!(sending.as_mut().poll(cx).is_pending(), "post-handoff Link bookkeeping is blocked");
        std::task::Poll::Ready(())
    })
    .await;
    let receipt = observed_rx.try_recv().expect("native request observer");
    let locked_link = pair.link.lock().await;
    pair.to_peer.tx_channel.try_recv().expect("release carrier blocker");
    std::future::poll_fn(|cx| {
        assert!(
            sending.as_mut().poll(cx).is_pending(),
            "post-handoff Link bookkeeping is now blocked"
        );
        std::task::Poll::Ready(())
    })
    .await;
    let advertisement =
        pair.to_peer.tx_channel.try_recv().expect("actual advertisement handoff").packet;
    assert!(receipt.current().sent_at.is_some());
    NativePair::deliver(&pair.peer, advertisement, *pair.to_local.address()).await;
    let first = pair.to_local.tx_channel.try_recv().expect("first peer request for parts").packet;
    assert_eq!(first.context, PacketContext::ResourceRequest);
    let mut ingress = Box::pin(NativePair::deliver(&pair.local, first, *pair.to_peer.address()));
    std::future::poll_fn(|cx| {
        assert!(
            ingress.as_mut().poll(cx).is_pending(),
            "fast ingress waits for the current send handler"
        );
        std::task::Poll::Ready(())
    })
    .await;
    drop(locked_link);
    let ((), sent) = tokio::join!(biased; ingress, sending);
    sent.expect("request dispatch returns");
    assert!(!pair.to_peer.tx_channel.is_empty(), "the first peer request was served immediately");
    pair.pump().await;
    assert_eq!(receipt.status(), RequestStatus::Delivered, "no retry timer was needed");
}
