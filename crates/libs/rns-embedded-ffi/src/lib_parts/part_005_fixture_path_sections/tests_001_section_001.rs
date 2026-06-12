    use std::{fs, path::PathBuf};

    use serde_json::Value;

    use super::{
        ffi_v1_node_error_boundary, rns_embedded_node_free, rns_embedded_node_new,
        rns_embedded_node_push_inbound_wire, rns_embedded_node_queue_message,
        rns_embedded_node_set_link_state, rns_embedded_node_take_outbound_wire,
        rns_embedded_node_tick, rns_embedded_v1_abi_version, rns_embedded_v1_get_capabilities,
        rns_embedded_v1_node_broadcast, rns_embedded_v1_node_config_default,
        rns_embedded_v1_node_free, rns_embedded_v1_node_get_status, rns_embedded_v1_node_new,
        rns_embedded_v1_node_restart, rns_embedded_v1_node_send,
        rns_embedded_v1_node_set_log_level, rns_embedded_v1_node_start, rns_embedded_v1_node_stop,
        rns_embedded_v1_node_subscribe_events, rns_embedded_v1_subscription_close,
        rns_embedded_v1_subscription_next, RnsEmbeddedLinkState, RnsEmbeddedNodeConfig,
        RnsEmbeddedStatus, RnsEmbeddedV1Capabilities, RnsEmbeddedV1EventKind,
        RnsEmbeddedV1LogLevel, RnsEmbeddedV1NodeError, RnsEmbeddedV1NodeErrorCode,
        RnsEmbeddedV1NodeEvent, RnsEmbeddedV1NodeStatus, RnsEmbeddedV1PollResult,
        RnsEmbeddedV1PollResultKind, RnsEmbeddedV1RunState, RnsEmbeddedV1SendReceipt,
        RNS_EMBEDDED_V1_CAPABILITY_SCHEMA_VERSION, RNS_EMBEDDED_V1_CAP_BLOCKING_NEXT,
        RNS_EMBEDDED_V1_CAP_BROADCAST_EXPLICIT_LIST, RNS_EMBEDDED_V1_CAP_COMPAT_LEGACY_FFI,
        RNS_EMBEDDED_V1_CAP_EVENT_GAP_SIGNALING, RNS_EMBEDDED_V1_CAP_MANAGED_RUNTIME,
        RNS_EMBEDDED_V1_DRIVER_TICK_MAX_MS, RNS_EMBEDDED_V1_DRIVER_TICK_TARGET_MS,
        RNS_EMBEDDED_V1_KNOWN_CAPABILITY_BITS, RNS_EMBEDDED_V1_MAX_BLOCKING_TIMEOUT_MS,
    };

    use rns_embedded_core::packet::{decode_frame, encode_frame, PacketFrame};

    use rns_embedded_runtime::node::{
        is_valid_extension_id, NODE_EXTENSION_ID_BOOTSTRAPPED, NODE_EXTENSION_ID_MESSAGE_QUEUED,
        NODE_EXTENSION_ID_RECEIVED_SUMMARY,
    };

    fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../docs/fixtures/embedded/public-node-api-v1")
            .join(name)
    }

    fn contract_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../docs/contracts").join(name)
    }

    fn json_fixture(name: &str) -> Value {
        serde_json::from_str(&fs::read_to_string(fixture_path(name)).expect("read fixture"))
            .expect("parse fixture")
    }

    fn contract_json(name: &str) -> Value {
        serde_json::from_str(&fs::read_to_string(contract_path(name)).expect("read contract"))
            .expect("parse contract")
    }

    fn capability_bit(name: &str) -> u64 {
        match name {
            "RNS_EMBEDDED_V1_CAP_MANAGED_RUNTIME" => RNS_EMBEDDED_V1_CAP_MANAGED_RUNTIME,
            "RNS_EMBEDDED_V1_CAP_BLOCKING_NEXT" => RNS_EMBEDDED_V1_CAP_BLOCKING_NEXT,
            "RNS_EMBEDDED_V1_CAP_BROADCAST_EXPLICIT_LIST" => {
                RNS_EMBEDDED_V1_CAP_BROADCAST_EXPLICIT_LIST
            }
            "RNS_EMBEDDED_V1_CAP_COMPAT_LEGACY_FFI" => RNS_EMBEDDED_V1_CAP_COMPAT_LEGACY_FFI,
            "RNS_EMBEDDED_V1_CAP_EVENT_GAP_SIGNALING" => RNS_EMBEDDED_V1_CAP_EVENT_GAP_SIGNALING,
            other => panic!("unknown capability bit {other}"),
        }
    }

    fn node_error_code(name: &str) -> RnsEmbeddedV1NodeErrorCode {
        match name {
            "Unknown" => RnsEmbeddedV1NodeErrorCode::Unknown,
            "InvalidConfig" => RnsEmbeddedV1NodeErrorCode::InvalidConfig,
            "IoError" => RnsEmbeddedV1NodeErrorCode::IoError,
            "NetworkError" => RnsEmbeddedV1NodeErrorCode::NetworkError,
            "ReticulumError" => RnsEmbeddedV1NodeErrorCode::ReticulumError,
            "AlreadyRunning" => RnsEmbeddedV1NodeErrorCode::AlreadyRunning,
            "NotRunning" => RnsEmbeddedV1NodeErrorCode::NotRunning,
            "Timeout" => RnsEmbeddedV1NodeErrorCode::Timeout,
            "InternalError" => RnsEmbeddedV1NodeErrorCode::InternalError,
            "InvalidHandle" => RnsEmbeddedV1NodeErrorCode::InvalidHandle,
            "InvalidPointer" => RnsEmbeddedV1NodeErrorCode::InvalidPointer,
            "ModeConflict" => RnsEmbeddedV1NodeErrorCode::ModeConflict,
            "SubscriptionClosed" => RnsEmbeddedV1NodeErrorCode::SubscriptionClosed,
            "NodeRestarted" => RnsEmbeddedV1NodeErrorCode::NodeRestarted,
            "EventGap" => RnsEmbeddedV1NodeErrorCode::EventGap,
            "QueuePressure" => RnsEmbeddedV1NodeErrorCode::QueuePressure,
            other => panic!("unknown node error code {other}"),
        }
    }

    fn poll_kind(name: &str) -> RnsEmbeddedV1PollResultKind {
        match name {
            "Event" => RnsEmbeddedV1PollResultKind::Event,
            "Timeout" => RnsEmbeddedV1PollResultKind::Timeout,
            "Closed" => RnsEmbeddedV1PollResultKind::Closed,
            "Gap" => RnsEmbeddedV1PollResultKind::Gap,
            "NodeStopped" => RnsEmbeddedV1PollResultKind::NodeStopped,
            "NodeRestarted" => RnsEmbeddedV1PollResultKind::NodeRestarted,
            other => panic!("unknown poll kind {other}"),
        }
    }

    fn status_code(name: &str) -> RnsEmbeddedStatus {
        match name {
            "Ok" => RnsEmbeddedStatus::Ok,
            "Backpressure" => RnsEmbeddedStatus::Backpressure,
            other => panic!("unknown status {other}"),
        }
    }

    #[test]
    fn ffi_node_ticks_and_drains_outbound_wire() {
        let config = RnsEmbeddedNodeConfig::default();
        let node = rns_embedded_node_new(&config);
        assert!(!node.is_null());

        assert_eq!(
            rns_embedded_node_set_link_state(node, RnsEmbeddedLinkState::Up),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(rns_embedded_node_tick(node, 0), RnsEmbeddedStatus::Ok);

        let mut out = [0_u8; 256];
        let mut out_len = 0usize;
        assert_eq!(
            rns_embedded_node_take_outbound_wire(node, out.as_mut_ptr(), out.len(), &mut out_len),
            RnsEmbeddedStatus::Ok
        );
        let frame = decode_frame(&out[..out_len]).expect("decode frame");
        assert_eq!(frame.kind, 0x11);

        rns_embedded_node_free(node);
    }

    #[test]
    fn ffi_node_accepts_inbound_and_queues_message() {
        let config = RnsEmbeddedNodeConfig::default();
        let node = rns_embedded_node_new(&config);
        assert!(!node.is_null());

        assert_eq!(
            rns_embedded_node_set_link_state(node, RnsEmbeddedLinkState::Up),
            RnsEmbeddedStatus::Ok
        );

        let inbound = encode_frame(&PacketFrame::new(0x44, 9, b"ping".to_vec()).expect("frame"))
            .expect("encode");
        assert_eq!(
            rns_embedded_node_push_inbound_wire(node, inbound.as_ptr(), inbound.len()),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(rns_embedded_node_tick(node, 0), RnsEmbeddedStatus::Ok);

        let destination = [0x7A_u8; 16];
        let mut sequence = 0_u32;
        assert_eq!(
            rns_embedded_node_queue_message(
                node,
                destination.as_ptr(),
                b"hello".as_ptr(),
                b"hello".len(),
                &mut sequence,
            ),
            RnsEmbeddedStatus::Ok
        );
        assert!(sequence > 0);

        rns_embedded_node_free(node);
    }

    #[test]
    fn ffi_v1_reports_capabilities_and_status() {
        assert_eq!(rns_embedded_v1_abi_version(), 1);
        let fixture = json_fixture("capability-probe.json");

        let mut capabilities = RnsEmbeddedV1Capabilities::default();
        assert_eq!(rns_embedded_v1_get_capabilities(&mut capabilities), RnsEmbeddedStatus::Ok);
        assert_eq!(capabilities.abi_version, 1);
        assert_eq!(
            capabilities.capability_schema_version,
            fixture["capability_schema_version"].as_u64().expect("schema version") as u32
        );
        assert_eq!(
            capabilities.capability_schema_version,
            RNS_EMBEDDED_V1_CAPABILITY_SCHEMA_VERSION
        );
        assert_eq!(capabilities.known_capability_bits, RNS_EMBEDDED_V1_KNOWN_CAPABILITY_BITS);
        assert_eq!(
            capabilities.known_capability_bits,
            fixture["known_capability_bits"].as_u64().expect("known bits")
        );
        assert_eq!(capabilities.capability_bits, capabilities.compile_time_capability_bits);
        assert_ne!(capabilities.capability_bits, 0);
        assert_eq!(fixture["unknown_bit_policy"].as_str().expect("policy"), "ignore");
        assert_eq!(
            (capabilities.capability_bits | (1_u64 << 63)) & capabilities.known_capability_bits,
            capabilities.capability_bits
        );
        assert_ne!(capabilities.capability_bits & RNS_EMBEDDED_V1_CAP_BROADCAST_EXPLICIT_LIST, 0);
        assert_ne!(capabilities.capability_bits & RNS_EMBEDDED_V1_CAP_COMPAT_LEGACY_FFI, 0);
        assert_ne!(capabilities.capability_bits & RNS_EMBEDDED_V1_CAP_EVENT_GAP_SIGNALING, 0);
        for name in
            fixture["compile_time_required_bits"].as_array().expect("compile time required bits")
        {
            let bit = capability_bit(name.as_str().expect("bit name"));
            assert_ne!(capabilities.compile_time_capability_bits & bit, 0);
            assert_ne!(capabilities.capability_bits & bit, 0);
        }
        assert_eq!(
            capabilities.max_subscriptions,
            fixture["max_subscriptions"].as_u64().expect("max subscriptions") as u32
        );

        #[cfg(feature = "std")]
        {
            for name in fixture["std_required_bits"].as_array().expect("std bits") {
                let bit = capability_bit(name.as_str().expect("bit"));
                assert_ne!(capabilities.capability_bits & bit, 0);
            }
            assert_eq!(
                capabilities.max_blocking_timeout_ms,
                RNS_EMBEDDED_V1_MAX_BLOCKING_TIMEOUT_MS
            );
            assert_eq!(capabilities.driver_tick_target_ms, RNS_EMBEDDED_V1_DRIVER_TICK_TARGET_MS);
            assert_eq!(capabilities.driver_tick_max_ms, RNS_EMBEDDED_V1_DRIVER_TICK_MAX_MS);
        }

        #[cfg(not(feature = "std"))]
        {
            for name in fixture["alloc_forbidden_bits"].as_array().expect("alloc forbidden bits") {
                let bit = capability_bit(name.as_str().expect("bit"));
                assert_eq!(capabilities.capability_bits & bit, 0);
            }
            assert_eq!(capabilities.max_blocking_timeout_ms, 0);
            assert_eq!(capabilities.driver_tick_target_ms, 0);
            assert_eq!(capabilities.driver_tick_max_ms, 0);
        }

        let node = rns_embedded_v1_node_new();
        assert!(!node.is_null());

        let mut config = rns_embedded_v1_node_config_default();
        let mut error = RnsEmbeddedV1NodeError::default();
        assert_eq!(rns_embedded_v1_node_start(node, &config, &mut error), RnsEmbeddedStatus::Ok);
        assert_eq!(error.code, RnsEmbeddedV1NodeErrorCode::Unknown);

        let mut status = RnsEmbeddedV1NodeStatus::default();
        assert_eq!(rns_embedded_v1_node_get_status(node, &mut status), RnsEmbeddedStatus::Ok);
        assert_eq!(status.run_state, RnsEmbeddedV1RunState::Running);
        assert_eq!(status.epoch, 1);

        config.announce_interval_ms = 2_000;
        assert_eq!(rns_embedded_v1_node_restart(node, &config, &mut error), RnsEmbeddedStatus::Ok);
        assert_eq!(rns_embedded_v1_node_get_status(node, &mut status), RnsEmbeddedStatus::Ok);
        assert_eq!(status.epoch, 2);

        assert_eq!(rns_embedded_v1_node_stop(node, &mut error), RnsEmbeddedStatus::Ok);
        assert_eq!(rns_embedded_v1_node_get_status(node, &mut status), RnsEmbeddedStatus::Ok);
        assert_eq!(status.run_state, RnsEmbeddedV1RunState::Stopped);

        rns_embedded_v1_node_free(node);
    }

    #[test]
    fn ffi_v1_send_and_broadcast_surface_node_errors() {
        let node = rns_embedded_v1_node_new();
        assert!(!node.is_null());

        let config = rns_embedded_v1_node_config_default();
        let mut error = RnsEmbeddedV1NodeError::default();
        assert_eq!(rns_embedded_v1_node_start(node, &config, &mut error), RnsEmbeddedStatus::Ok);

        assert_eq!(
            rns_embedded_v1_node_set_log_level(node, RnsEmbeddedV1LogLevel::Debug, &mut error),
            RnsEmbeddedStatus::Ok
        );

        let destination = [0xA5_u8; 16];
        let mut receipt = RnsEmbeddedV1SendReceipt::default();
        assert_eq!(
            rns_embedded_v1_node_send(
                node,
                destination.as_ptr(),
                b"hello".as_ptr(),
                b"hello".len(),
                &mut receipt,
                &mut error,
            ),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(receipt.target_count, 1);
        assert_eq!(receipt.epoch, 1);

        let destinations = [[0x10_u8; 16], [0x20_u8; 16]];
        assert_eq!(
            rns_embedded_v1_node_broadcast(
                node,
                destinations.as_ptr().cast::<u8>(),
                destinations.len(),
                b"fanout".as_ptr(),
                b"fanout".len(),
                &mut receipt,
                &mut error,
            ),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(receipt.target_count, 2);

        assert_eq!(
            rns_embedded_v1_node_broadcast(
                node,
                core::ptr::null(),
                0,
                b"fanout".as_ptr(),
                b"fanout".len(),
                &mut receipt,
                &mut error,
            ),
            RnsEmbeddedStatus::InvalidInput
        );
        assert_eq!(error.code, RnsEmbeddedV1NodeErrorCode::InvalidConfig);

        rns_embedded_v1_node_free(node);
    }

    #[test]
    fn ffi_legacy_truncation_reports_required_length() {
        let fixture = json_fixture("truncation-reporting.json");
        let config = RnsEmbeddedNodeConfig::default();
        let node = rns_embedded_node_new(&config);
        assert!(!node.is_null());

        assert_eq!(
            rns_embedded_node_set_link_state(node, RnsEmbeddedLinkState::Up),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(rns_embedded_node_tick(node, 0), RnsEmbeddedStatus::Ok);

        let mut out = [0_u8; 1];
        let mut out_len = 0usize;
        assert_eq!(
            rns_embedded_node_take_outbound_wire(
                node,
                out.as_mut_ptr(),
                fixture["small_buffer_len"].as_u64().expect("small buffer len") as usize,
                &mut out_len,
            ),
            status_code(fixture["expected_status"].as_str().expect("status"))
        );
        assert!(out_len >= fixture["required_len_min"].as_u64().expect("required len") as usize);

        rns_embedded_node_free(node);
    }

    #[test]
    fn ffi_v1_queue_pressure_maps_to_stable_error_code() {
        let node = rns_embedded_v1_node_new();
        assert!(!node.is_null());

        let mut config = rns_embedded_v1_node_config_default();
        config.max_outbound_queue = 1;
        let mut error = RnsEmbeddedV1NodeError::default();
        let mut receipt = RnsEmbeddedV1SendReceipt::default();
        assert_eq!(rns_embedded_v1_node_start(node, &config, &mut error), RnsEmbeddedStatus::Ok);

        let destination = [0x42_u8; 16];
        assert_eq!(
            rns_embedded_v1_node_send(
                node,
                destination.as_ptr(),
                b"one".as_ptr(),
                3,
                &mut receipt,
                &mut error,
            ),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(
            rns_embedded_v1_node_send(
                node,
                destination.as_ptr(),
                b"two".as_ptr(),
                3,
                &mut receipt,
                &mut error,
            ),
            RnsEmbeddedStatus::Backpressure
        );
        assert_eq!(error.code, RnsEmbeddedV1NodeErrorCode::QueuePressure);

        rns_embedded_v1_node_free(node);
    }

    #[test]
    fn ffi_v1_subscriptions_surface_restart_and_status_events() {
        let node = rns_embedded_v1_node_new();
        assert!(!node.is_null());

        let config = rns_embedded_v1_node_config_default();
        let mut error = RnsEmbeddedV1NodeError::default();
        let mut subscription = core::ptr::null_mut();
        assert_eq!(
            rns_embedded_v1_node_subscribe_events(node, &mut subscription, &mut error),
            RnsEmbeddedStatus::Ok
        );
        assert!(!subscription.is_null());

        assert_eq!(rns_embedded_v1_node_start(node, &config, &mut error), RnsEmbeddedStatus::Ok);

        let mut poll = RnsEmbeddedV1PollResult::default();
        let mut event = RnsEmbeddedV1NodeEvent::default();
        assert_eq!(
            rns_embedded_v1_subscription_next(subscription, 100, &mut poll, &mut event, &mut error),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(poll.kind, RnsEmbeddedV1PollResultKind::NodeRestarted);
        assert_eq!(poll.epoch, 1);
        assert_eq!(error.code, RnsEmbeddedV1NodeErrorCode::NodeRestarted);

        assert_eq!(
            rns_embedded_v1_subscription_next(subscription, 100, &mut poll, &mut event, &mut error),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(poll.kind, RnsEmbeddedV1PollResultKind::Event);
        assert_eq!(event.kind, RnsEmbeddedV1EventKind::StatusChanged);
        assert_eq!(event.epoch, 1);
        assert_eq!(error.code, RnsEmbeddedV1NodeErrorCode::Unknown);

        assert_eq!(rns_embedded_v1_node_stop(node, &mut error), RnsEmbeddedStatus::Ok);
        assert_eq!(
            rns_embedded_v1_subscription_next(subscription, 100, &mut poll, &mut event, &mut error),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(poll.kind, RnsEmbeddedV1PollResultKind::NodeStopped);
        assert_eq!(error.code, RnsEmbeddedV1NodeErrorCode::NotRunning);

        assert_eq!(
            rns_embedded_v1_subscription_close(subscription, &mut error),
            RnsEmbeddedStatus::Ok
        );

        rns_embedded_v1_node_free(node);
    }
