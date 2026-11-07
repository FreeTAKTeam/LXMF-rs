#[tokio::test]
async fn resource_completion_is_registered_before_failed_dispatch_and_survives_event_lag() {
    use crate::resource::ResourceOutcome;

    let local = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("completion", &local, false));
    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (outbound, _) = activated_link_pair(destination, &signer);
    let link_id = *outbound.id();
    transport
        .get_handler()
        .lock()
        .await
        .out_links
        .insert(destination.address_hash, Arc::new(Mutex::new(outbound)));
    let mut legacy = transport.resource_events();
    let mut observed = None;
    let result = transport
        .send_resource_observed_with_completion(
            &link_id,
            b"no registered interface".to_vec(),
            None,
            |hash, completion| {
                assert_eq!(completion.current(), None, "registration precedes dispatch");
                observed = Some((hash, completion));
            },
        )
        .await;
    assert!(matches!(result, Err(RnsError::ConnectionError)));
    let (hash, mut completion) = observed.expect("registration survives send error");
    let event = legacy.recv().await.expect("legacy failure still emitted");
    assert_eq!(event.hash, hash);
    assert!(matches!(event.kind, ResourceEventKind::OutboundFailed));
    for _ in 0..2048 {
        assert!(transport.send_resource(&link_id, vec![0x51], None).await.is_err());
    }
    assert!(matches!(
        legacy.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_))
    ));
    assert_eq!(completion.current(), Some(ResourceOutcome::Failed));
    assert_eq!(
        completion.wait().await.expect("retained dispatch failure"),
        ResourceOutcome::Failed
    );
}

#[tokio::test]
async fn delivery_resource_uses_retained_link_after_map_replacement() {
    use crate::delivery::{send_on_link_observed, LinkSendResult};

    let local = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("retained-resource", &local, false));
    let mut selected_channel =
        transport.iface_manager().lock().await.new_channel_with_role(8, IfaceRole::Unicast);
    let mut replacement_channel =
        transport.iface_manager().lock().await.new_channel_with_role(8, IfaceRole::Unicast);
    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (mut selected, peer) = activated_link_pair(destination, &signer);
    selected.set_ingress_iface(*selected_channel.address());
    let selected_id = *selected.id();
    let selected = Arc::new(Mutex::new(selected));
    let (mut replacement, _) = activated_link_pair(destination, &signer);
    replacement.set_ingress_iface(*replacement_channel.address());
    let replacement = Arc::new(Mutex::new(replacement));
    let handler = transport.get_handler();
    handler.lock().await.out_links.insert(destination.address_hash, selected.clone());
    // Selection and map replacement are separate from explicit Link closure.
    handler.lock().await.out_links.insert(destination.address_hash, replacement.clone());

    let mut observed = None;
    let result = send_on_link_observed(
        &transport,
        &selected,
        &[0x51; 1024],
        |_| panic!("oversized payload must use Resource representation"),
        |hash| {
            assert!(selected_channel.tx_channel.try_recv().is_err(), "observe before handoff");
            observed = Some(hash);
        },
    )
    .await
    .expect("selected Link remains the dispatch context");
    let LinkSendResult::Resource(hash) = result else {
        panic!("expected Resource");
    };
    assert_eq!(observed, Some(hash));
    let sent = selected_channel.tx_channel.try_recv().expect("selected interface handoff");
    assert_eq!(sent.tx_type, TxMessageType::Direct(*selected_channel.address()));
    assert_eq!(sent.packet.destination, selected_id);
    assert_eq!(sent.packet.context, PacketContext::ResourceAdvrtisement);
    assert_eq!(decrypt_resource_advertisement(&peer, &sent.packet).hash, hash);
    assert!(replacement_channel.tx_channel.try_recv().is_err());
    assert!(Arc::ptr_eq(
        handler.lock().await.out_links.get(&destination.address_hash).expect("replacement"),
        &replacement
    ));
}

#[tokio::test]
async fn retained_resource_completion_survives_dispatch_failure_without_map_lookup() {
    use crate::resource::ResourceOutcome;

    let local = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("retained-failure", &local, false));
    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (outbound, _) = activated_link_pair(destination, &signer);
    let link_id = *outbound.id();
    let link = Arc::new(Mutex::new(outbound));
    let mut legacy = transport.resource_events();
    let mut observed = None;
    let result = transport
        .send_resource_on_link_observed_with_completion(
            &link,
            vec![0x51; 1024],
            None,
            |hash, completion| {
                assert_eq!(completion.current(), None);
                assert!(legacy.try_recv().is_err(), "registration precedes failure");
                observed = Some((hash, completion));
            },
        )
        .await;
    assert!(matches!(result, Err(RnsError::ConnectionError)));
    let (hash, mut completion) = observed.expect("native registration before failed handoff");
    let event = legacy.try_recv().expect("legacy failure still published");
    assert_eq!(event.hash, hash);
    assert_eq!(event.link_id, link_id);
    assert!(matches!(event.kind, ResourceEventKind::OutboundFailed));
    assert_eq!(completion.wait().await.expect("observed failure"), ResourceOutcome::Failed);
    assert!(transport.get_handler().lock().await.resource_manager.has_no_outbound_state());
}
