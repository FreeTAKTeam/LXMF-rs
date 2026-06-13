    use super::{
        annotate_inbound_record_stamp_status, decode_inbound_payload_with_diagnostics,
        evaluate_inbound_stamp_policy, inbound_record_allowed_by_delivery_policy,
        inbound_stamp_policy_allows_payload, InboundStampStatus,
    };

    use lxmf::inbound_decode::InboundPayloadMode;

    use lxmf::{Payload, WireMessage};

    use rns_core::identity::PrivateIdentity;

    use rns_rpc::{RpcDaemon, RpcRequest};

    use crate::lxmf_bridge::build_wire_message_with_options;

    #[test]
    fn decode_inbound_payload_accepts_integer_timestamp_wire() {
        let destination = [0x11; 16];
        let source = [0x22; 16];
        let signature = [0x33; 64];
        let payload = rmp_serde::to_vec(&rmpv::Value::Array(vec![
            rmpv::Value::from(1_770_000_000_i64),
            rmpv::Value::from("title"),
            rmpv::Value::from("hello from python-like payload"),
            rmpv::Value::Nil,
        ]))
        .expect("payload encoding");
        let mut wire = Vec::new();
        wire.extend_from_slice(&destination);
        wire.extend_from_slice(&source);
        wire.extend_from_slice(&signature);
        wire.extend_from_slice(&payload);

        let (record, _) = decode_inbound_payload_with_diagnostics(
            destination,
            &wire,
            InboundPayloadMode::FullWire,
        );
        let record = record.expect("decoded record");
        assert_eq!(record.source, hex::encode(source));
        assert_eq!(record.destination, hex::encode(destination));
        assert_eq!(record.title, "title");
        assert_eq!(record.content, "hello from python-like payload");
        assert_eq!(record.timestamp, 1_770_000_000_i64);
        assert_eq!(record.direction, "in");
    }

    #[test]
    fn decode_inbound_payload_preserves_float_timestamp_and_binary_fields_in_metadata() {
        let identity = PrivateIdentity::new_from_name("inbound-fidelity-binary");
        let mut source = [0u8; 16];
        source.copy_from_slice(identity.address_hash().as_slice());
        let destination = [0x73u8; 16];
        let payload = Payload::new(
            1_770_000_000.25,
            Some(b"body\0\xff".to_vec()),
            Some(b"\xfftitle".to_vec()),
            Some(rmpv::Value::Map(vec![(
                rmpv::Value::String("meta".into()),
                rmpv::Value::String("python-storage".into()),
            )])),
            None,
        );
        let mut wire = WireMessage::new(destination, source, payload);
        wire.sign(&identity).expect("sign");
        let packed = wire.pack().expect("pack");

        let (record, _) = decode_inbound_payload_with_diagnostics(
            destination,
            &packed,
            InboundPayloadMode::FullWire,
        );
        let record = record.expect("decoded record");
        assert_eq!(record.timestamp, 1_770_000_000_i64);
        assert_eq!(record.title, "");
        assert_eq!(record.content, "");
        let fields = record.fields.expect("fields");
        assert_eq!(fields["meta"], serde_json::json!("python-storage"));
        assert_eq!(fields["_lxmf"]["timestamp_f64"], serde_json::json!(1_770_000_000.25));
        assert_eq!(fields["_lxmf"]["title_base64"], serde_json::json!("/3RpdGxl"));
        assert_eq!(fields["_lxmf"]["content_base64"], serde_json::json!("Ym9keQD/"));
    }

    #[test]
    fn inbound_stamp_policy_rejects_missing_stamp_when_required() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "stamp_policy_set".to_string(),
                params: Some(serde_json::json!({"target_cost": 4, "flexibility": 0})),
            })
            .expect("set stamp policy");
        let identity = PrivateIdentity::new_from_name("inbound-missing-stamp");
        let mut source = [0u8; 16];
        source.copy_from_slice(identity.address_hash().as_slice());
        let destination = [0x71u8; 16];
        let wire = build_wire_message_with_options(
            source,
            destination,
            "title",
            "content",
            None,
            &identity,
            None,
            None,
            None,
        )
        .expect("wire");

        let err = inbound_stamp_policy_allows_payload(
            &daemon,
            destination,
            &wire,
            InboundPayloadMode::FullWire,
        )
        .expect_err("missing stamp must be rejected");
        assert!(err.contains("invalid LXMF stamp"));
    }

    #[test]
    fn inbound_stamp_policy_reports_invalid_status_when_enforcement_disabled() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 11,
                method: "stamp_policy_set".to_string(),
                params: Some(serde_json::json!({
                    "target_cost": 4,
                    "flexibility": 0,
                    "enforce": false,
                })),
            })
            .expect("set stamp policy");
        let identity = PrivateIdentity::new_from_name("inbound-invalid-stamp-observed");
        let mut source = [0u8; 16];
        source.copy_from_slice(identity.address_hash().as_slice());
        let destination = [0x75u8; 16];
        let wire = build_wire_message_with_options(
            source,
            destination,
            "title",
            "content",
            None,
            &identity,
            None,
            None,
            None,
        )
        .expect("wire");

        let status = evaluate_inbound_stamp_policy(
            &daemon,
            destination,
            &wire,
            InboundPayloadMode::FullWire,
        )
        .expect("invalid stamp should be observable when enforcement is disabled");

        assert!(status.checked);
        assert!(!status.valid);
        assert!(status.value.is_none());

        let mut record = decode_inbound_payload_with_diagnostics(
            destination,
            &wire,
            InboundPayloadMode::FullWire,
        )
        .0
        .expect("decoded record");
        annotate_inbound_record_stamp_status(&mut record, status);
        let fields = record.fields.expect("fields");
        assert_eq!(fields["_lxmf"]["stamp_checked"], serde_json::json!(true));
        assert_eq!(fields["_lxmf"]["stamp_valid"], serde_json::json!(false));
        assert_eq!(fields["_lxmf"].get("stamp_value"), None);
    }

    #[test]
    fn inbound_stamp_status_annotation_sets_lxmf_flags() {
        let mut record = rns_rpc::MessageRecord {
            id: "msg-1".into(),
            source: "aa".into(),
            destination: "bb".into(),
            title: String::new(),
            content: String::new(),
            timestamp: 0,
            direction: "in".into(),
            fields: Some(serde_json::json!({"meta": 1})),
            receipt_status: None,
        };

        annotate_inbound_record_stamp_status(
            &mut record,
            InboundStampStatus { checked: true, valid: true, value: Some(17) },
        );
        let fields = record.fields.expect("fields");
        assert_eq!(fields["meta"], serde_json::json!(1));
        assert_eq!(fields["_lxmf"]["stamp_checked"], serde_json::json!(true));
        assert_eq!(fields["_lxmf"]["stamp_valid"], serde_json::json!(true));
        assert_eq!(fields["_lxmf"]["stamp_value"], serde_json::json!(17));
    }

    fn record_from_source(source: &str) -> rns_rpc::MessageRecord {
        rns_rpc::MessageRecord {
            id: "msg".to_string(),
            source: source.to_string(),
            destination: "dst".to_string(),
            title: "title".to_string(),
            content: "content".to_string(),
            timestamp: 1_700_000_000,
            direction: "in".to_string(),
            fields: None,
            receipt_status: None,
        }
    }

    #[test]
    fn inbound_delivery_policy_rejects_ignored_source_like_python() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "set_delivery_policy".to_string(),
                params: Some(serde_json::json!({
                    "ignored_destinations": ["aabbcc"],
                })),
            })
            .expect("set delivery policy");

        assert!(!inbound_record_allowed_by_delivery_policy(&daemon, &record_from_source("AABBCC")));
    }

    #[test]
    fn inbound_delivery_policy_allows_non_ignored_source() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "set_delivery_policy".to_string(),
                params: Some(serde_json::json!({
                    "ignored_destinations": ["aabbcc"],
                })),
            })
            .expect("set delivery policy");

        assert!(inbound_record_allowed_by_delivery_policy(&daemon, &record_from_source("ddeeff")));
    }

    #[test]
    fn inbound_stamp_policy_returns_checked_status_when_valid() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 20,
                method: "stamp_policy_set".to_string(),
                params: Some(serde_json::json!({"target_cost": 1, "flexibility": 0})),
            })
            .expect("set stamp policy");
        let identity = PrivateIdentity::new_from_name("inbound-status-stamp");
        let mut source = [0u8; 16];
        source.copy_from_slice(identity.address_hash().as_slice());
        let destination = [0x79u8; 16];
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

        let status = evaluate_inbound_stamp_policy(
            &daemon,
            destination,
            &wire,
            InboundPayloadMode::FullWire,
        )
        .expect("valid stamp status");
        assert!(status.checked);
        assert!(status.valid);
        assert!(status.value.is_some_and(|value| value >= 1));
    }

    #[test]
    fn inbound_stamp_policy_accepts_generated_pow_stamp() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 2,
                method: "stamp_policy_set".to_string(),
                params: Some(serde_json::json!({"target_cost": 1, "flexibility": 0})),
            })
            .expect("set stamp policy");
        let identity = PrivateIdentity::new_from_name("inbound-pow-stamp");
        let mut source = [0u8; 16];
        source.copy_from_slice(identity.address_hash().as_slice());
        let destination = [0x72u8; 16];
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

        inbound_stamp_policy_allows_payload(
            &daemon,
            destination,
            &wire,
            InboundPayloadMode::FullWire,
        )
        .expect("valid pow stamp should pass");
    }

    #[test]
    fn inbound_stamp_policy_accepts_pow_stamp_within_flexibility_window_like_python() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 22,
                method: "stamp_policy_set".to_string(),
                params: Some(serde_json::json!({"target_cost": 3, "flexibility": 2})),
            })
            .expect("set stamp policy");
        let identity = PrivateIdentity::new_from_name("inbound-flexible-stamp");
        let mut source = [0u8; 16];
        source.copy_from_slice(identity.address_hash().as_slice());
        let destination = [0x7a_u8; 16];
        let payload = Payload::new(
            1_770_000_001.0,
            Some(b"content".to_vec()),
            Some(b"title".to_vec()),
            None,
            None,
        );
        let message_id = WireMessage::new(destination, source, payload.clone()).message_id();
        let stamp = stamp_with_value_range(&message_id, 1, 3);
        let mut wire = WireMessage::new(
            destination,
            source,
            Payload::new(
                payload.timestamp,
                payload.content.as_ref().map(|value| value.to_vec()),
                payload.title.as_ref().map(|value| value.to_vec()),
                payload.fields.clone(),
                Some(stamp),
            ),
        );
        wire.sign(&identity).expect("sign");
        let packed = wire.pack().expect("pack");

        let status = evaluate_inbound_stamp_policy(
            &daemon,
            destination,
            &packed,
            InboundPayloadMode::FullWire,
        )
        .expect("stamp within flexibility window should pass");

        assert!(status.checked);
        assert!(status.valid);
        assert!(status.value.is_some_and(|value| (1..3).contains(&value)));
    }

    #[test]
    fn inbound_stamp_policy_accepts_issued_ticket_stamp() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 3,
                method: "stamp_policy_set".to_string(),
                params: Some(serde_json::json!({"target_cost": 16, "flexibility": 0})),
            })
            .expect("set stamp policy");
        let identity = PrivateIdentity::new_from_name("inbound-ticket-stamp");
        let mut source = [0u8; 16];
        source.copy_from_slice(identity.address_hash().as_slice());
        let source_hex = hex::encode(source);
        let ticket = daemon.ensure_ticket(source_hex.as_str(), None).expect("issue ticket");
        let destination = [0x73u8; 16];
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

        inbound_stamp_policy_allows_payload(
            &daemon,
            destination,
            &wire,
            InboundPayloadMode::FullWire,
        )
        .expect("ticket-validated stamp should pass");
    }
