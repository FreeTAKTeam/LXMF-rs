#[test]
fn sdk_conformance_app_mode_event_ordering_fixture_is_monotonic() {
    let fixture = fixture("events.delivery_ordering.json");
    let events = fixture["expected_events"]
        .as_array()
        .expect("ordering expected events")
        .iter()
        .map(|value| value.as_str().expect("event"))
        .collect::<Vec<_>>();
    assert_eq!(
        events,
        vec!["MessageQueued", "MessageDispatching", "MessageSent", "MessageDelivered"]
    );

    let assertions = fixture["assertions"].as_object().expect("ordering assertions");
    assert_eq!(
        assertions.get("seq_no_strictly_monotonic").and_then(JsonValue::as_bool),
        Some(true)
    );
    assert_eq!(
        assertions.get("delivery_progression_monotonic").and_then(JsonValue::as_bool),
        Some(true)
    );
    assert_eq!(assertions.get("terminal_event_final").and_then(JsonValue::as_bool), Some(true));
}

#[test]
fn sdk_conformance_app_mode_timeout_fixture_treats_timeout_as_non_error_outcome() {
    let fixture = fixture("timeout.poll_timeout.json");
    assert_eq!(fixture["kind"].as_str(), Some("timeout"));
    assert_eq!(fixture["operation"].as_str(), Some("next_event"));
    assert_eq!(fixture["expected_outcome"].as_str(), Some("timeout"));
    assert_eq!(fixture["returns_error"].as_bool(), Some(false));
    assert_eq!(fixture["expected_events"].as_array().map(Vec::len), Some(0));
}

#[test]
fn sdk_conformance_app_mode_queue_pressure_fixture_requires_typed_visibility() {
    let fixture = fixture("delivery.queue_pressure.json");
    assert_eq!(fixture["kind"].as_str(), Some("queue_pressure"));
    assert_eq!(fixture["expected_error"].as_str(), Some("SDK_APP_DELIVERY_QUEUE_PRESSURE"));

    let events = fixture["expected_events"]
        .as_array()
        .expect("queue pressure events")
        .iter()
        .map(|value| value.as_str().expect("event"))
        .collect::<Vec<_>>();
    assert_eq!(events, vec!["QueuePressureRaised", "RetryScheduled"]);

    let assertions = fixture["assertions"].as_object().expect("queue pressure assertions");
    assert_eq!(
        assertions.get("partial_acceptance_visible").and_then(JsonValue::as_bool),
        Some(true)
    );
    assert_eq!(
        assertions.get("queue_pressure_hidden_in_ok").and_then(JsonValue::as_bool),
        Some(false)
    );
}

#[test]
fn sdk_conformance_app_mode_reconnect_fixture_orders_recovery_explicitly() {
    let fixture = fixture("connectivity.reconnect_recovery.json");
    let events = fixture["expected_events"]
        .as_array()
        .expect("reconnect expected events")
        .iter()
        .map(|value| value.as_str().expect("event"))
        .collect::<Vec<_>>();
    assert_eq!(events, vec!["RuntimeDegraded", "ReconnectScheduled", "RuntimeRecovered"]);

    let assertions = fixture["assertions"].as_object().expect("reconnect assertions");
    assert_eq!(
        assertions.get("schedule_precedes_recovery").and_then(JsonValue::as_bool),
        Some(true)
    );
    assert_eq!(
        assertions.get("silent_recovery_forbidden").and_then(JsonValue::as_bool),
        Some(true)
    );
}

#[test]
fn sdk_conformance_app_mode_typed_error_mapping_fixture_freezes_core_fields() {
    let fixture = fixture("errors.typed_mapping.json");
    let mappings = fixture["mappings"].as_array().expect("typed error mappings");
    assert!(mappings.len() >= 4, "expected a core typed error set");

    for mapping in mappings {
        assert!(mapping["retryable"].is_boolean(), "retryable must be boolean");
        assert!(mapping["terminal"].is_boolean(), "terminal must be boolean");
        assert!(
            mapping["user_action_required"].is_boolean(),
            "user_action_required must be boolean"
        );
    }

    let queue_pressure = mappings
        .iter()
        .find(|mapping| mapping["code"].as_str() == Some("SDK_APP_DELIVERY_QUEUE_PRESSURE"))
        .expect("queue pressure mapping");
    assert_eq!(queue_pressure["category"].as_str(), Some("Delivery"));
    assert_eq!(queue_pressure["retryable"].as_bool(), Some(true));
    assert_eq!(queue_pressure["terminal"].as_bool(), Some(false));
}

#[test]
fn sdk_conformance_app_mode_unknown_additive_fixture_requires_safe_ignore_policy() {
    let fixture = fixture("compatibility.unknown_additive.json");
    let policy = fixture["expected_policy"].as_object().expect("unknown additive policy");
    assert_eq!(policy.get("ignore_unknown_capabilities").and_then(JsonValue::as_bool), Some(true));
    assert_eq!(policy.get("ignore_unknown_fields").and_then(JsonValue::as_bool), Some(true));
    assert_eq!(policy.get("preserve_known_fields").and_then(JsonValue::as_bool), Some(true));
    assert_eq!(
        policy.get("fail_only_on_required_by_profile").and_then(JsonValue::as_bool),
        Some(true)
    );
}
