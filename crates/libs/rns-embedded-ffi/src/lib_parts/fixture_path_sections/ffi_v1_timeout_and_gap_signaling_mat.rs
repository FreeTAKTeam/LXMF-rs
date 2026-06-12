    #[test]
    fn ffi_v1_timeout_and_gap_signaling_match_fixtures() {
        let timeout_fixture = json_fixture("poll-timeout.json");
        let gap_fixture = json_fixture("gap-restart-signaling.json");

        let node = rns_embedded_v1_node_new();
        assert!(!node.is_null());
        let mut error = RnsEmbeddedV1NodeError::default();
        let mut subscription = core::ptr::null_mut();
        assert_eq!(
            rns_embedded_v1_node_subscribe_events(node, &mut subscription, &mut error),
            RnsEmbeddedStatus::Ok
        );

        let mut poll = RnsEmbeddedV1PollResult::default();
        let mut event = RnsEmbeddedV1NodeEvent::default();
        assert_eq!(
            rns_embedded_v1_subscription_next(
                subscription,
                timeout_fixture["timeout_ms"].as_u64().expect("timeout"),
                &mut poll,
                &mut event,
                &mut error,
            ),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(
            poll.kind,
            poll_kind(timeout_fixture["expected_poll_kind"].as_str().expect("timeout poll kind"))
        );
        assert_eq!(
            error.code,
            node_error_code(timeout_fixture["expected_error_code"].as_str().expect("timeout code"))
        );

        let mut config = rns_embedded_v1_node_config_default();
        config.max_events = 1;
        assert_eq!(rns_embedded_v1_node_start(node, &config, &mut error), RnsEmbeddedStatus::Ok);
        assert_eq!(
            rns_embedded_v1_subscription_next(subscription, 100, &mut poll, &mut event, &mut error),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(
            poll.kind,
            poll_kind(gap_fixture["expected_restart_poll_kind"].as_str().expect("restart kind"))
        );
        assert_eq!(
            error.code,
            node_error_code(
                gap_fixture["expected_restart_error_code"].as_str().expect("restart code")
            )
        );

        assert_eq!(
            rns_embedded_v1_node_set_log_level(node, RnsEmbeddedV1LogLevel::Debug, &mut error),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(
            rns_embedded_v1_node_set_log_level(node, RnsEmbeddedV1LogLevel::Trace, &mut error),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(
            rns_embedded_v1_subscription_next(subscription, 0, &mut poll, &mut event, &mut error),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(
            poll.kind,
            poll_kind(gap_fixture["expected_gap_poll_kind"].as_str().expect("gap kind"))
        );
        assert_eq!(
            error.code,
            node_error_code(gap_fixture["expected_gap_error_code"].as_str().expect("gap code"))
        );

        assert_eq!(
            rns_embedded_v1_subscription_close(subscription, &mut error),
            RnsEmbeddedStatus::Ok
        );
        rns_embedded_v1_node_free(node);
    }

    #[test]
    fn ffi_v1_restart_epoch_matches_fixture() {
        let fixture = json_fixture("restart-epoch.json");
        let node = rns_embedded_v1_node_new();
        let mut error = RnsEmbeddedV1NodeError::default();
        let mut status = RnsEmbeddedV1NodeStatus::default();
        let config = rns_embedded_v1_node_config_default();

        assert_eq!(rns_embedded_v1_node_start(node, &config, &mut error), RnsEmbeddedStatus::Ok);
        assert_eq!(rns_embedded_v1_node_get_status(node, &mut status), RnsEmbeddedStatus::Ok);
        assert_eq!(status.epoch, fixture["initial_epoch"].as_u64().expect("initial epoch"));

        assert_eq!(rns_embedded_v1_node_restart(node, &config, &mut error), RnsEmbeddedStatus::Ok);
        assert_eq!(rns_embedded_v1_node_get_status(node, &mut status), RnsEmbeddedStatus::Ok);
        assert_eq!(status.epoch, fixture["restarted_epoch"].as_u64().expect("restarted epoch"));

        rns_embedded_v1_node_free(node);
    }

    #[test]
    fn ffi_v1_boundary_maps_panic_to_internal_error() {
        let mut error = RnsEmbeddedV1NodeError::default();

        let status = ffi_v1_node_error_boundary(&mut error, || -> RnsEmbeddedStatus {
            panic!("simulated ffi boundary panic");
        });

        assert_eq!(status, RnsEmbeddedStatus::InvalidState);
        assert_eq!(error.code, RnsEmbeddedV1NodeErrorCode::InternalError);
    }

    #[test]
    fn error_code_registry_matches_contract_artifact() {
        let artifact = contract_json("node-error-codes-v1.json");
        assert_eq!(artifact["schema_version"].as_u64().expect("schema version"), 1);

        for code in artifact["codes"].as_array().expect("error codes") {
            let variant = code["rust_variant"].as_str().expect("variant");
            let value = code["value"].as_u64().expect("value") as u32;
            assert_eq!(node_error_code(variant) as u32, value, "{variant}");
        }
    }

    #[test]
    fn extension_ids_follow_registry_fixture() {
        let fixture = json_fixture("extension-ids.json");
        let expected = [
            NODE_EXTENSION_ID_BOOTSTRAPPED,
            NODE_EXTENSION_ID_MESSAGE_QUEUED,
            NODE_EXTENSION_ID_RECEIVED_SUMMARY,
        ];

        for (index, entry) in fixture.as_array().expect("extension ids").iter().enumerate() {
            let numeric_id = entry["numeric_id"].as_u64().expect("numeric id") as u32;
            let registry_id = entry["registry_id"].as_str().expect("registry id");
            assert_eq!(numeric_id, expected[index]);
            assert!(is_valid_extension_id(numeric_id));
            assert!(registry_id.starts_with("event."));
            assert!(registry_id.ends_with(".v1"));
            assert!(registry_id.split('.').count() >= 4);
        }
        assert!(!is_valid_extension_id(99));
    }
