    #[test]
    fn inbound_stamp_policy_accepts_issued_ticket_stamp_above_ticket_cost_like_python() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 4,
                method: "stamp_policy_set".to_string(),
                params: Some(serde_json::json!({"target_cost": 257, "flexibility": 0})),
            })
            .expect("set stamp policy");
        let identity = PrivateIdentity::new_from_name("inbound-high-cost-ticket-stamp");
        let mut source = [0u8; 16];
        source.copy_from_slice(identity.address_hash().as_slice());
        let source_hex = hex::encode(source);
        let ticket = daemon.ensure_ticket(source_hex.as_str(), None).expect("issue ticket");
        let destination = [0x74u8; 16];
        let wire = build_wire_message_with_options(
            source,
            destination,
            "title",
            "content",
            None,
            &identity,
            None,
            Some(ticket.ticket.as_str()),
            None,
        )
        .expect("wire");

        let status = evaluate_inbound_stamp_policy(
            &daemon,
            destination,
            &wire,
            InboundPayloadMode::FullWire,
        )
        .expect("ticket-validated stamp should pass");
        assert!(status.checked);
        assert!(status.valid);
        assert_eq!(status.value, Some(crate::lxmf_stamps::COST_TICKET));
    }

    #[test]
    fn inbound_stamp_policy_accepts_destination_stripped_pow_stamp() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 4,
                method: "stamp_policy_set".to_string(),
                params: Some(serde_json::json!({"target_cost": 1, "flexibility": 0})),
            })
            .expect("set stamp policy");
        let identity = PrivateIdentity::new_from_name("inbound-destination-stripped-stamp");
        let mut source = [0u8; 16];
        source.copy_from_slice(identity.address_hash().as_slice());
        let destination = [0x74u8; 16];
        let wire = build_wire_message_with_options(
            source,
            destination,
            "title",
            "content",
            None,
            &identity,
            Some(1),
            None,
            None,
        )
        .expect("wire");
        let stripped = &wire[16..];

        inbound_stamp_policy_allows_payload(
            &daemon,
            destination,
            stripped,
            InboundPayloadMode::DestinationStripped,
        )
        .expect("valid destination-stripped stamp should pass");
    }

    fn stamp_with_value_range(
        message_id: &[u8; 32],
        min_value: u32,
        max_exclusive: u32,
    ) -> Vec<u8> {
        let workblock = crate::lxmf_stamps::stamp_workblock(message_id, 3000);
        for nonce in 0u64.. {
            let stamp = nonce.to_le_bytes().to_vec();
            let value = crate::lxmf_stamps::stamp_value(&workblock, &stamp);
            if (min_value..max_exclusive).contains(&value) {
                return stamp;
            }
        }
        unreachable!("u64 nonce space should contain a matching low-cost stamp")
    }
