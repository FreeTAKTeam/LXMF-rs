/// `RNS.Link`'s watchdog closes a link that outlives its establishment timeout
/// while still pending, and on a non-transport instance the path it was
/// tried on is expired and requested again — the same handling a link that
/// was closed by hand already got here. Without it the request was repeated
/// every `INTERVAL_OUTPUT_LINK_REPEAT` for the life of the process.
#[tokio::test]
async fn a_pending_out_link_that_outlives_its_establishment_timeout_is_closed_and_the_path_expired() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let mut iface_channel = transport.iface_manager().lock().await.new_channel(16);
    let iface = *iface_channel.address();
    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let destination_hash = destination.desc.address_hash;

    {
        let mut guard = handler.lock().await;
        assert!(guard.path_table.restore_tunnel_path(
            destination_hash,
            destination_hash,
            1,
            iface,
            Hash::new_from_slice(b"packet"),
            std::time::Instant::now(),
        ));
    }

    let link = transport.link(destination.desc).await;
    let _initial_request = timeout(Duration::from_millis(200), iface_channel.tx_channel.recv())
        .await
        .expect("initial link request should be queued")
        .expect("tx channel open");
    // Well past the first hop plus one hop it was sized for.
    link.lock().await.set_establishment_start_for_test(std::time::Instant::now() - Duration::from_secs(60));

    super::jobs::handle_check_links(handler.lock().await).await;

    assert_eq!(link.lock().await.status(), LinkStatus::Closed, "the link is closed, not requested again");
    assert!(!handler.lock().await.out_links.contains_key(&destination_hash));
    assert!(handler.lock().await.path_table.get(&destination_hash).is_none(), "the path it was tried on is expired");
    let rediscovery = timeout(Duration::from_millis(200), iface_channel.tx_channel.recv())
        .await
        .expect("rediscovery path request should be queued")
        .expect("tx channel open");
    assert_eq!(rediscovery.tx_type, crate::iface::TxMessageType::Broadcast(None));
    assert_eq!(&rediscovery.packet.data.as_slice()[..ADDRESS_HASH_SIZE], destination_hash.as_slice());
}

/// Inside the timeout the link is left pending and its request repeated,
/// which is what the job did before.
#[tokio::test]
async fn a_pending_out_link_inside_its_establishment_timeout_is_requested_again() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let mut iface_channel = transport.iface_manager().lock().await.new_channel(16);
    let iface = *iface_channel.address();
    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let destination_hash = destination.desc.address_hash;

    {
        let mut guard = handler.lock().await;
        assert!(guard.path_table.restore_tunnel_path(
            destination_hash,
            destination_hash,
            1,
            iface,
            Hash::new_from_slice(b"packet"),
            std::time::Instant::now(),
        ));
    }

    let link = transport.link(destination.desc).await;
    let _initial_request = timeout(Duration::from_millis(200), iface_channel.tx_channel.recv())
        .await
        .expect("initial link request should be queued")
        .expect("tx channel open");
    // Past the repeat interval, inside the establishment timeout.
    link.lock().await.set_request_time_for_test(std::time::Instant::now() - Duration::from_secs(7));

    super::jobs::handle_check_links(handler.lock().await).await;

    assert_eq!(link.lock().await.status(), LinkStatus::Pending);
    assert!(handler.lock().await.path_table.get(&destination_hash).is_some(), "the path is kept");
    let repeat = timeout(Duration::from_millis(200), iface_channel.tx_channel.recv())
        .await
        .expect("the link request should be repeated")
        .expect("tx channel open");
    assert_eq!(repeat.packet.destination, destination_hash, "a repeated link request, not a path request");
}

/// A pending link, aged past the timeout it was given, with a request already
/// sent.
fn a_pending_link_past_its_establishment_timeout() -> Link {
    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _rx) = tokio::sync::broadcast::channel(4);
    let mut link = Link::new(destination, tx);
    let _request = link.request();
    link.set_establishment_start_for_test(std::time::Instant::now() - Duration::from_secs(60));
    link
}

/// Repeating a request that is still in flight is the retransmit this timeout
/// exists to bound, so it must not restart the clock. Without this the link
/// maintenance job's own repeat would keep a dead link alive forever, which is
/// the behaviour the timeout replaced.
#[test]
fn repeating_a_still_pending_request_does_not_restart_the_establishment_clock() {
    let mut link = a_pending_link_past_its_establishment_timeout();

    let _repeat = link.request();

    assert!(
        link.establishment_timed_out(std::time::Instant::now()),
        "the clock runs from the attempt, not from its latest retransmit"
    );
}

/// `restart` starts over, so the attempt it begins gets the whole timeout
/// rather than inheriting what the previous one had already spent.
#[test]
fn restarting_a_link_begins_a_new_establishment_attempt() {
    let mut link = a_pending_link_past_its_establishment_timeout();

    link.restart();

    assert!(!link.establishment_timed_out(std::time::Instant::now()));
}

/// The same seen through the maintenance job: a restarted out-link is left
/// pending, not closed for the time its predecessor spent.
#[tokio::test]
async fn a_restarted_out_link_is_not_closed_for_the_attempt_it_replaced() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let mut iface_channel = transport.iface_manager().lock().await.new_channel(16);
    let iface = *iface_channel.address();
    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let destination_hash = destination.desc.address_hash;

    {
        let mut guard = handler.lock().await;
        assert!(guard.path_table.restore_tunnel_path(
            destination_hash,
            destination_hash,
            1,
            iface,
            Hash::new_from_slice(b"packet"),
            std::time::Instant::now(),
        ));
    }

    let link = transport.link(destination.desc).await;
    let _initial_request = timeout(Duration::from_millis(200), iface_channel.tx_channel.recv())
        .await
        .expect("initial link request should be queued")
        .expect("tx channel open");
    {
        let mut guard = link.lock().await;
        guard.set_establishment_start_for_test(
            std::time::Instant::now() - Duration::from_secs(60),
        );
        guard.restart();
    }

    super::jobs::handle_check_links(handler.lock().await).await;

    assert_eq!(link.lock().await.status(), LinkStatus::Pending, "the restarted attempt is left alone");
    assert!(handler.lock().await.out_links.contains_key(&destination_hash));
    assert!(handler.lock().await.path_table.get(&destination_hash).is_some(), "the path is kept");
}
