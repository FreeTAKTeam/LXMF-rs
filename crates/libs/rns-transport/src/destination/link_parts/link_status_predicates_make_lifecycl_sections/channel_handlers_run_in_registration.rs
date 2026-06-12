    #[test]
    fn channel_handlers_run_in_registration_order_and_short_circuit() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

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

        let calls = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let first_short_circuits = Arc::new(Mutex::new(false));

        let calls_clone = calls.clone();
        let first_flag = first_short_circuits.clone();
        outbound.register_channel_handler(0x5151, move |_| {
            calls_clone.lock().expect("lock").push("first");
            *first_flag.lock().expect("lock")
        });

        let calls_clone = calls.clone();
        outbound.register_channel_handler(0x5151, move |_| {
            calls_clone.lock().expect("lock").push("second");
            true
        });

        let (_sequence, packet) =
            inbound.send_channel_message(0x5151, b"fan-out".to_vec()).expect("channel message");
        assert!(matches!(outbound.handle_packet(&packet, iface), LinkHandleResult::Proof(_)));
        assert_eq!(calls.lock().expect("lock").as_slice(), ["first", "second"]);

        calls.lock().expect("lock").clear();
        *first_short_circuits.lock().expect("lock") = true;

        let (_sequence, packet) = inbound
            .send_channel_message(0x5151, b"short-circuit".to_vec())
            .expect("channel message");
        assert!(matches!(outbound.handle_packet(&packet, iface), LinkHandleResult::Proof(_)));
        assert_eq!(calls.lock().expect("lock").as_slice(), ["first"]);
    }

    #[test]
    fn removing_last_channel_handler_keeps_explicit_channel_open_state() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

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

        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_clone = seen.clone();
        let handler_id = outbound.register_channel_handler(0x6161, move |envelope| {
            seen_clone.lock().expect("lock").push(envelope);
            true
        });
        assert!(outbound.remove_channel_handler(handler_id));
        assert!(!outbound.remove_channel_handler(handler_id));

        let (_sequence, packet) =
            inbound.send_channel_message(0x6161, b"no-consumer".to_vec()).expect("channel message");
        assert!(matches!(outbound.handle_packet(&packet, iface), LinkHandleResult::Proof(_)));
        assert!(seen.lock().expect("lock").is_empty());
    }

    #[test]
    fn channel_handler_panics_do_not_unwind_receive_path() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

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

        outbound.register_channel_handler(0x9999, |_| -> bool { panic!("boom") });

        let (_sequence, packet) =
            inbound.send_channel_message(0x9999, b"panic".to_vec()).expect("channel message");

        let result = catch_unwind(AssertUnwindSafe(|| outbound.handle_packet(&packet, iface)));
        assert!(result.is_ok(), "channel handler panic should be contained");
        assert!(matches!(result.unwrap(), LinkHandleResult::Proof(_)));
    }

    #[test]
    fn channel_send_window_limits_outstanding_messages_until_proved() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

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
        inbound.register_channel_handler(0x7000, |_| true);

        let (_first_sequence, first_packet) = outbound
            .send_channel_message(0x7000, b"first".to_vec())
            .expect("first channel message");
        let (_second_sequence, _second_packet) = outbound
            .send_channel_message(0x7000, b"second".to_vec())
            .expect("second channel message");
        assert!(matches!(
            outbound.send_channel_message(0x7000, b"third".to_vec()),
            Err(ChannelError::LinkNotReady)
        ));

        let proof = match inbound.handle_packet(&first_packet, iface) {
            LinkHandleResult::Proof(proof) => proof,
            _ => panic!("first channel packet should generate proof"),
        };
        assert!(matches!(outbound.handle_packet(&proof, iface), LinkHandleResult::None));
        assert!(outbound.send_channel_message(0x7000, b"third".to_vec()).is_ok());
    }

    #[test]
    fn slow_rtt_links_start_with_single_channel_slot() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

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

        outbound.rtt = Duration::from_secs_f32(1.6);
        outbound.refresh_channel_flow_control();
        assert!(outbound.send_channel_message(0x7001, b"first".to_vec()).is_ok());
        assert!(matches!(
            outbound.send_channel_message(0x7001, b"second".to_vec()),
            Err(ChannelError::LinkNotReady)
        ));
    }

    #[test]
    fn channel_window_grows_after_successful_deliveries() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

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
        inbound.register_channel_handler(0x7200, |_| true);

        assert_eq!(outbound.channel_send_window(), 2);

        let (_sequence, packet) =
            outbound.send_channel_message(0x7200, b"first".to_vec()).expect("channel message");
        let proof = match inbound.handle_packet(&packet, iface) {
            LinkHandleResult::Proof(proof) => proof,
            _ => panic!("channel packet should generate proof"),
        };
        assert!(matches!(outbound.handle_packet(&proof, iface), LinkHandleResult::None));

        assert_eq!(outbound.channel_send_window(), 3);
    }

    #[test]
    fn channel_window_shrinks_after_retry_timeout() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

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
        inbound.register_channel_handler(0x7201, |_| true);

        let (_sequence, packet) =
            outbound.send_channel_message(0x7201, b"grow".to_vec()).expect("channel message");
        let proof = match inbound.handle_packet(&packet, iface) {
            LinkHandleResult::Proof(proof) => proof,
            _ => panic!("channel packet should generate proof"),
        };
        assert!(matches!(outbound.handle_packet(&proof, iface), LinkHandleResult::None));
        assert_eq!(outbound.channel_send_window(), 3);

        let (_sequence, _packet) =
            outbound.send_channel_message(0x7201, b"timeout".to_vec()).expect("channel message");
        let _ = outbound.poll_channel_timeouts(Instant::now() + Duration::from_secs(1));

        assert_eq!(outbound.channel_send_window(), 2);
    }

    #[test]
    fn timed_out_channel_messages_are_retransmitted() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

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

        let (sequence, packet) =
            outbound.send_channel_message(0x7100, b"retry-me".to_vec()).expect("channel message");
        let resend_packets =
            outbound.poll_channel_timeouts(Instant::now() + Duration::from_secs(1));

        assert_eq!(resend_packets.len(), 1);
        assert_eq!(resend_packets[0].hash(), packet.hash());
        assert_eq!(outbound.channel_state(sequence), ChannelMessageState::Sent);
        assert_eq!(outbound.status(), LinkStatus::Active);
    }

    #[test]
    fn channel_retry_exhaustion_closes_link_and_fails_pending_messages() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

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
        outbound.rtt = Duration::from_millis(10);

        let (sequence, _packet) = outbound
            .send_channel_message(0x7101, b"eventually-fails".to_vec())
            .expect("channel message");

        for seconds in 1..=4 {
            let resend_packets =
                outbound.poll_channel_timeouts(Instant::now() + Duration::from_secs(seconds));
            assert_eq!(resend_packets.len(), 1);
            assert_eq!(outbound.status(), LinkStatus::Active);
            assert_eq!(outbound.channel_state(sequence), ChannelMessageState::Sent);
        }

        let resend_packets =
            outbound.poll_channel_timeouts(Instant::now() + Duration::from_secs(5));
        assert!(resend_packets.is_empty());
        assert_eq!(outbound.status(), LinkStatus::Closed);
        assert_eq!(outbound.channel_state(sequence), ChannelMessageState::Failed);
    }

    #[test]
    fn channel_messages_mark_delivered_when_their_link_proof_arrives() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

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
        inbound.register_channel_handler(0x55AA, |_| true);

        let (sequence, packet) = outbound
            .send_channel_message(0x55AA, b"needs-proof".to_vec())
            .expect("channel message");
        assert_eq!(outbound.channel_state(sequence), ChannelMessageState::Sent);

        let proof = match inbound.handle_packet(&packet, iface) {
            LinkHandleResult::Proof(proof) => proof,
            _ => panic!("channel packet should generate link proof"),
        };
        assert!(matches!(outbound.handle_packet(&proof, iface), LinkHandleResult::None));
        assert_eq!(outbound.channel_state(sequence), ChannelMessageState::Delivered);
    }

    #[test]
    fn pending_channel_messages_fail_when_link_closes() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

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

        let (sequence, _packet) =
            outbound.send_channel_message(0x9001, b"will-fail".to_vec()).expect("channel message");
        assert_eq!(outbound.channel_state(sequence), ChannelMessageState::Sent);

        outbound.close();
        assert_eq!(outbound.channel_state(sequence), ChannelMessageState::Failed);
    }

    #[test]
    fn activity_timers_exclude_keepalives_from_data_timer() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(4);

        let mut link = Link::new(destination, tx);
        let stale_anchor = Instant::now() - Duration::from_secs(5);
        link.status = LinkStatus::Active;
        link.activated_at = Some(stale_anchor);
        link.last_inbound = Some(stale_anchor);
        link.last_outbound = Some(stale_anchor);
        link.last_data = Some(stale_anchor);

        link.note_inbound(PacketContext::KeepAlive);
        link.note_outbound(PacketContext::KeepAlive);

        assert!(link.no_inbound_for() < Duration::from_secs(1));
        assert!(link.no_outbound_for() < Duration::from_secs(1));
        assert!(link.no_data_for() >= Duration::from_secs(5));
        assert!(link.inactive_for() < Duration::from_secs(1));
    }
