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
