    #[test]
    fn teardown_builds_link_close_packet_and_purges_session_state() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(4);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        let packet = outbound.teardown().expect("active teardown should produce close packet");
        assert_eq!(packet.header.packet_type, PacketType::Data);
        assert_eq!(packet.context, PacketContext::LinkClose);
        assert_eq!(outbound.status(), LinkStatus::Closed);
        assert_eq!(outbound.close_reason(), Some(LinkCloseReason::InitiatorClosed));
        assert!(outbound.session_cipher.is_none());
        assert_eq!(outbound.derived_key.as_bytes(), DerivedKey::new_empty().as_bytes());
        assert_eq!(outbound.peer_identity().address_hash, Identity::default().address_hash);

        let mut plain = [0u8; PACKET_MDU];
        let decrypted = inbound.decrypt(packet.data.as_slice(), &mut plain).expect("decrypt close");
        assert_eq!(decrypted, outbound.id().as_slice());
    }

    #[tokio::test]
    async fn close_events_expose_python_teardown_reason_codes() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };

        let (tx, mut events) = tokio::sync::broadcast::channel(4);
        let mut outbound = Link::new(destination, tx.clone());
        outbound.request();
        outbound.close();
        let event = events.recv().await.expect("local close event");
        assert!(matches!(event.event, LinkEvent::Closed));
        assert_eq!(event.close_reason, Some(LinkCloseReason::InitiatorClosed));
        assert_eq!(event.close_reason.expect("reason").as_u8(), 0x02);
        outbound.close();
        assert_eq!(outbound.close_reason(), Some(LinkCloseReason::InitiatorClosed));
        assert!(matches!(
            events.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));

        let (request_tx, _) = tokio::sync::broadcast::channel(4);
        let mut request_sender = Link::new(destination, request_tx);
        let request = request_sender.request();
        let (inbound_tx, mut inbound_events) = tokio::sync::broadcast::channel(4);
        let mut inbound = Link::new_from_request(
            &request,
            signer.sign_key().clone(),
            destination,
            inbound_tx.clone(),
        )
        .expect("inbound request");
        inbound.close();
        let event = inbound_events.recv().await.expect("destination close event");
        assert!(matches!(event.event, LinkEvent::Closed));
        assert_eq!(event.close_reason, Some(LinkCloseReason::DestinationClosed));
        assert_eq!(event.close_reason.expect("reason").as_u8(), 0x03);

        let (timeout_tx, mut timeout_events) = tokio::sync::broadcast::channel(4);
        let mut timed_out = Link::new(destination, timeout_tx);
        timed_out.request();
        timed_out.close_with_reason(LinkCloseReason::Timeout);
        let event = timeout_events.recv().await.expect("timeout close event");
        assert!(matches!(event.event, LinkEvent::Closed));
        assert_eq!(event.close_reason, Some(LinkCloseReason::Timeout));
        assert_eq!(event.close_reason.expect("reason").as_u8(), 0x01);
    }

    #[test]
    fn inbound_link_close_packet_tears_down_active_link() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(4);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        let close_packet =
            outbound.teardown().expect("active teardown should produce close packet");
        assert!(matches!(inbound.handle_packet(&close_packet, iface), LinkHandleResult::None));
        assert_eq!(inbound.status(), LinkStatus::Closed);
        assert_eq!(inbound.close_reason(), Some(LinkCloseReason::InitiatorClosed));
        assert!(inbound.session_cipher.is_none());
        assert_eq!(inbound.derived_key.as_bytes(), DerivedKey::new_empty().as_bytes());
    }

    #[test]
    fn link_rtt_packets_update_adaptive_keepalive_timing() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(4);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        outbound.request_time = Instant::now() - Duration::from_millis(250);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        let rtt_packet = outbound.create_rtt();
        assert!(matches!(inbound.handle_packet(&rtt_packet, iface), LinkHandleResult::None));
        assert!(outbound.rtt >= Duration::from_millis(250));
        assert!(inbound.rtt > Duration::ZERO);
        assert!(inbound.keepalive < Duration::from_secs_f32(KEEPALIVE_MAX_SECS));
        assert_eq!(
            inbound.stale_time,
            Duration::from_secs_f32(inbound.keepalive.as_secs_f32() * STALE_FACTOR)
        );
    }

    #[test]
    fn next_watchdog_deadline_tracks_keepalive_and_stale_timeouts() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(4);

        let mut link = Link::new(destination, tx);
        link.status = LinkStatus::Active;
        link.rtt = Duration::from_millis(200);
        link.update_keepalive_timing();
        let anchor = Instant::now();
        link.activated_at = Some(anchor);
        link.last_inbound = Some(anchor);
        let active_deadline = link.next_watchdog_deadline(true).expect("active deadline");
        assert!(active_deadline >= anchor + link.keepalive);

        link.status = LinkStatus::Stale;
        link.stale_since = Some(anchor);
        let stale_deadline = link.next_watchdog_deadline(false).expect("stale deadline");
        let expected = anchor
            + Duration::from_secs_f32(
                (link.rtt.as_secs_f32() * KEEPALIVE_TIMEOUT_FACTOR) + STALE_GRACE_SECS,
            );
        assert_eq!(stale_deadline, expected);
    }

    #[test]
    fn watchdog_transitions_active_links_to_stale_and_then_closed() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, mut events) = tokio::sync::broadcast::channel(4);

        let mut link = Link::new(destination, tx);
        link.status = LinkStatus::Active;
        link.rtt = Duration::from_millis(500);
        link.update_keepalive_timing();
        link.activated_at = Some(Instant::now() - link.stale_time - Duration::from_secs(1));
        link.last_inbound = link.activated_at;

        assert_eq!(link.check_watchdog(false), LinkWatchdogAction::None);
        assert_eq!(link.status, LinkStatus::Stale);
        assert!(link.stale_since.is_some());

        link.stale_since = Some(
            Instant::now()
                - Duration::from_secs_f32(
                    (link.rtt.as_secs_f32() * KEEPALIVE_TIMEOUT_FACTOR) + STALE_GRACE_SECS + 1.0,
                ),
        );
        let action = link.check_watchdog(false);
        let packet = match action {
            LinkWatchdogAction::SendTeardown(packet) => packet,
            _ => panic!("watchdog should emit teardown packet"),
        };
        assert_eq!(packet.context, PacketContext::LinkClose);
        assert_eq!(link.status, LinkStatus::Closed);
        assert_eq!(link.close_reason(), Some(LinkCloseReason::Timeout));
        let close_event = events.try_recv().expect("stale timeout publishes close event");
        assert!(matches!(close_event.event, LinkEvent::Closed));
        assert_eq!(close_event.close_reason, Some(LinkCloseReason::Timeout));
    }

    #[test]
    fn watchdog_requests_keepalive_for_initiator_links() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(4);

        let mut link = Link::new(destination, tx);
        link.status = LinkStatus::Active;
        link.rtt = Duration::from_millis(20);
        link.update_keepalive_timing();
        let anchor = Instant::now() - link.keepalive - Duration::from_secs(1);
        link.activated_at = Some(anchor);
        link.last_inbound = Some(anchor);
        link.last_keepalive = Some(anchor);

        assert_eq!(link.check_watchdog(true), LinkWatchdogAction::SendKeepAlive);
        assert_eq!(link.status, LinkStatus::Active);
    }
