async fn lifecycle_fixture(
    inbound: bool,
) -> (Transport, Arc<Mutex<Link>>, Link, crate::iface::InterfaceChannel, PrivateIdentity) {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("lifecycle", &identity, false));
    let channel =
        transport.iface_manager().lock().await.new_channel_with_role(8, IfaceRole::Unicast);
    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (mut outbound, mut incoming) = activated_link_pair(destination, &signer);
    outbound.set_ingress_iface(*channel.address());
    incoming.handle_packet(&outbound.create_rtt(), *channel.address());
    assert_eq!(incoming.status(), LinkStatus::Active);
    let (mut retained, peer) = if inbound { (incoming, outbound) } else { (outbound, incoming) };
    retained.set_ingress_iface(*channel.address());
    let id = *retained.id();
    let retained = Arc::new(Mutex::new(retained));
    if inbound {
        transport.handler.lock().await.in_links.insert(id, retained.clone());
    } else {
        transport.handler.lock().await.out_links.insert(destination.address_hash, retained.clone());
    }
    (transport, retained, peer, channel, signer)
}

async fn observed_lifecycle_resource(
    transport: &Transport,
    link: &Arc<Mutex<Link>>,
) -> (Hash, crate::resource::ResourceCompletion) {
    let mut observed = None;
    let hash = transport
        .send_resource_on_link_observed_with_completion(
            link,
            b"live Resource".to_vec(),
            None,
            |hash, result| observed = Some((hash, result)),
        )
        .await
        .expect("advertise Resource");
    let (observed_hash, result) = observed.expect("registered before dispatch");
    assert_eq!(hash, observed_hash);
    assert_eq!(result.current(), None);
    (hash, result)
}

fn lifecycle_plaintext(peer: &Link, packet: &Packet) -> Vec<u8> {
    let mut buffer = PacketDataBuffer::new();
    peer.decrypt(packet.data.as_slice(), buffer.accuire_buf_max())
        .expect("original peer decrypts lifecycle frame")
        .to_vec()
}

#[tokio::test]
async fn dropped_resource_dispatch_can_explicitly_cancel_registration_on_both_sides_of_handoff() {
    use crate::resource::ResourceOutcome;
    for advertised in [false, true] {
        let (transport, retained, _, mut channel, _) = lifecycle_fixture(false).await;
        let manager = transport.iface_manager();
        let gate = StdMutex::new(None);
        let mut observed = None;
        let mut sending = Box::pin(transport.send_resource_on_link_observed_with_completion(
            &retained,
            b"interrupted advertisement".to_vec(),
            None,
            |hash, result| {
                *gate.lock().expect("gate") =
                    Some(manager.try_lock().expect("gate interface dispatch"));
                observed = Some((hash, result));
            },
        ));
        std::future::poll_fn(|cx| {
            assert!(sending.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        if advertised {
            let handler = transport.get_handler();
            let mut next_handler = Box::pin(handler.lock());
            std::future::poll_fn(|cx| {
                assert!(next_handler.as_mut().poll(cx).is_pending());
                std::task::Poll::Ready(())
            })
            .await;
            drop(gate.lock().expect("gate").take());
            std::future::poll_fn(|cx| {
                assert!(sending.as_mut().poll(cx).is_pending());
                std::task::Poll::Ready(())
            })
            .await;
            let blocked_confirmation = next_handler.await;
            let sent =
                channel.tx_channel.try_recv().expect("advertisement queued before confirmation");
            assert_eq!(sent.packet.context, PacketContext::ResourceAdvrtisement);
            drop(sending);
            drop(blocked_confirmation);
        } else {
            drop(sending);
            drop(gate.lock().expect("gate").take());
            assert!(channel.tx_channel.try_recv().is_err(), "no advertisement handoff");
        }
        let (hash, mut completion) = observed.expect("registered before blocked dispatch");
        assert_eq!(completion.current(), None, "dropping alone leaves native state");
        assert!(!transport
            .cancel_resource_on_link(&retained, hash)
            .await
            .expect("explicit pending cleanup"));
        assert_eq!(
            completion.wait().await.expect("native cancellation"),
            ResourceOutcome::Cancelled
        );
        assert!(transport.handler.lock().await.resource_manager.has_no_outbound_state());
        assert!(channel.tx_channel.try_recv().is_err(), "pending cancellation emits no frame");
    }
}

#[tokio::test]
async fn retained_resource_cancel_preserves_replacement_and_native_result_after_send_failure() {
    use crate::resource::ResourceOutcome;
    for fail_handoff in [false, true] {
        let (transport, retained, peer, mut channel, _) = lifecycle_fixture(false).await;
        let id = *retained.lock().await.id();
        let destination = *retained.lock().await.destination();
        let (hash, mut completion) = observed_lifecycle_resource(&transport, &retained).await;
        channel.tx_channel.try_recv().expect("first advertisement");
        let mut replacement = Link::new(destination, transport.link_out_event_tx.clone());
        replacement.request();
        let replacement = Arc::new(Mutex::new(replacement));
        transport
            .handler
            .lock()
            .await
            .out_links
            .insert(destination.address_hash, replacement.clone());
        let mut events = transport.resource_events();
        assert!(matches!(
            transport.cancel_resource_on_link(&replacement, hash).await,
            Err(RnsError::InvalidArgument)
        ));
        assert_eq!(completion.current(), None);
        assert!(events.try_recv().is_err());
        if fail_handoff {
            channel.tx_channel.close();
        }
        let result = transport.cancel_resource_on_link(&retained, hash).await;
        if fail_handoff {
            assert!(matches!(result, Err(RnsError::ConnectionError)));
        } else {
            assert!(result.expect("cancel handoff"));
            let sent = channel.tx_channel.try_recv().expect("cancel frame");
            assert_eq!(sent.tx_type, TxMessageType::Direct(*channel.address()));
            assert_eq!(sent.packet.destination, id);
            assert_eq!(sent.packet.context, PacketContext::ResourceInitiatorCancel);
            assert_eq!(lifecycle_plaintext(&peer, &sent.packet), hash.as_slice());
        }
        assert_eq!(
            completion.wait().await.expect("retained native cancellation"),
            ResourceOutcome::Cancelled
        );
        let event = events.try_recv().expect("one legacy cancellation");
        assert_eq!(event.hash, hash);
        assert!(matches!(event.kind, ResourceEventKind::OutboundCancelled));
        assert!(events.try_recv().is_err());
        assert_eq!(retained.lock().await.status(), LinkStatus::Active);
        assert!(Arc::ptr_eq(
            transport
                .handler
                .lock()
                .await
                .out_links
                .get(&destination.address_hash)
                .expect("replacement"),
            &replacement
        ));
        assert!(!transport
            .cancel_resource_on_link(&retained, hash)
            .await
            .expect("absent resource"));
    }
}

#[tokio::test]
async fn retained_teardown_preserves_replacement_resource_and_closes_inbound_or_outbound() {
    use crate::resource::ResourceOutcome;
    for inbound in [false, true] {
        let (transport, retained, mut peer, mut channel, signer) = lifecycle_fixture(inbound).await;
        let id = *retained.lock().await.id();
        let destination = *retained.lock().await.destination();
        let (hash, mut completion) = observed_lifecycle_resource(&transport, &retained).await;
        channel.tx_channel.try_recv().expect("retained advertisement");
        let (mut replacement, _) = activated_link_pair(destination, &signer);
        replacement.set_ingress_iface(*channel.address());
        let replacement = Arc::new(Mutex::new(replacement));
        transport
            .handler
            .lock()
            .await
            .out_links
            .insert(destination.address_hash, replacement.clone());
        let (_, replacement_completion) =
            observed_lifecycle_resource(&transport, &replacement).await;
        channel.tx_channel.try_recv().expect("replacement advertisement");
        let mut events = transport.resource_events();
        assert!(transport.teardown_link(&retained).await.expect("teardown frame handoff"));
        assert_eq!(retained.lock().await.status(), LinkStatus::Closed);
        assert_eq!(
            completion.wait().await.expect("retained native failure"),
            ResourceOutcome::Failed
        );
        let event = events.try_recv().expect("one legacy failure");
        assert_eq!(event.hash, hash);
        assert_eq!(event.link_id, id);
        assert!(matches!(event.kind, ResourceEventKind::OutboundFailed));
        assert!(events.try_recv().is_err());
        assert_eq!(replacement_completion.current(), None, "replacement transfer stays live");
        assert_eq!(replacement.lock().await.status(), LinkStatus::Active);
        let sent = channel.tx_channel.try_recv().expect("teardown packet");
        assert_eq!(sent.packet.destination, id);
        assert_eq!(sent.packet.context, PacketContext::LinkClose);
        assert_eq!(lifecycle_plaintext(&peer, &sent.packet), id.as_slice());
        peer.handle_packet(&sent.packet, *channel.address());
        assert_eq!(peer.status(), LinkStatus::Closed);
        {
            let handler = transport.handler.lock().await;
            assert!(Arc::ptr_eq(
                handler.out_links.get(&destination.address_hash).expect("replacement"),
                &replacement
            ));
            assert!(!handler.in_links.values().any(|current| Arc::ptr_eq(current, &retained)));
        }
        assert!(!transport.teardown_link(&retained).await.expect("idempotent native cleanup"));
        assert!(events.try_recv().is_err());
        assert!(channel.tx_channel.try_recv().is_err());
        assert_eq!(replacement_completion.current(), None);
    }
}

#[tokio::test]
async fn retained_teardown_keeps_old_interface_and_releases_link_before_dispatch() {
    let (transport, retained, peer, mut channel, _) = lifecycle_fixture(false).await;
    let id = *retained.lock().await.id();
    let destination = *retained.lock().await.destination();
    let manager = transport.iface_manager();
    let blocked = manager.lock().await;
    let mut teardown = Box::pin(transport.teardown_link(&retained));
    std::future::poll_fn(|cx| {
        assert!(teardown.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    assert!(
        transport.handler.try_lock().is_err(),
        "frame waits inside existing transport dispatch"
    );
    {
        let mut restarting =
            retained.try_lock().expect("teardown releases Link before interface I/O");
        assert_eq!(restarting.status(), LinkStatus::Closed);
        restarting.restart();
        restarting.request();
        assert_ne!(restarting.id(), &id);
        assert!(restarting.ingress_iface().is_none());
    }
    drop(blocked);
    assert!(teardown.await.expect("captured original interface still used"));
    let sent = channel.tx_channel.try_recv().expect("old teardown frame");
    assert_eq!(sent.tx_type, TxMessageType::Direct(*channel.address()));
    assert_eq!(sent.packet.destination, id);
    assert_eq!(lifecycle_plaintext(&peer, &sent.packet), id.as_slice());
    assert!(!transport.handler.lock().await.out_links.contains_key(&destination.address_hash));
    assert_eq!(
        retained.lock().await.status(),
        LinkStatus::Pending,
        "later restart is a separate native action"
    );
}

#[tokio::test]
async fn retained_lifecycle_waits_for_busy_link_without_holding_handler() {
    for cancel in [false, true] {
        let (transport, retained, _, mut channel, _) = lifecycle_fixture(false).await;
        let (hash, _) = observed_lifecycle_resource(&transport, &retained).await;
        channel.tx_channel.try_recv().expect("advertisement");
        let busy = retained.lock().await;
        let mut action: std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<bool, RnsError>>>,
        > = if cancel {
            Box::pin(transport.cancel_resource_on_link(&retained, hash))
        } else {
            Box::pin(transport.teardown_link(&retained))
        };
        std::future::poll_fn(|cx| {
            assert!(action.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        assert!(transport.handler.try_lock().is_ok(), "busy Link must not stall another peer");
        drop(busy);
        assert!(action.await.expect("lifecycle action"));
    }
}

#[tokio::test]
async fn retained_teardown_closes_pending_and_publishes_failure_before_failed_handoff() {
    use crate::resource::ResourceOutcome;
    for pending in [false, true] {
        let (transport, retained, _, mut channel, _) = lifecycle_fixture(false).await;
        let (hash, mut completion) = observed_lifecycle_resource(&transport, &retained).await;
        channel.tx_channel.try_recv().expect("advertisement");
        if pending {
            retained.lock().await.restart();
            // restart keeps the old ID until a request establishes a new generation.
        }
        channel.tx_channel.close();
        let result = transport.teardown_link(&retained).await;
        if pending {
            assert!(!result.expect("unestablished Link needs no close frame"));
        } else {
            assert!(matches!(result, Err(RnsError::ConnectionError)));
        }
        assert_eq!(retained.lock().await.status(), LinkStatus::Closed);
        assert_eq!(
            completion.wait().await.expect("native failure survives handoff"),
            ResourceOutcome::Failed
        );
        assert!(!transport
            .cancel_resource_on_link(&retained, hash)
            .await
            .expect("Resource removed"));
    }
}
