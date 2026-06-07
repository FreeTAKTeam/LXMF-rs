use serde_json::Value as JsonValue;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const REQUIRED_SCENARIOS: &[&str] = &[
    "lifecycle.start_stop_restart",
    "events.delivery_ordering",
    "timeout.poll_timeout",
    "delivery.queue_pressure",
    "connectivity.reconnect_recovery",
    "errors.typed_mapping",
    "compatibility.unknown_additive",
];

const ALLOWED_EVENTS: &[&str] = &[
    "RuntimeStarted",
    "RuntimeStopped",
    "RuntimeDegraded",
    "RuntimeRecovered",
    "MessageQueued",
    "MessageDispatching",
    "MessageSent",
    "MessageDelivered",
    "MessageFailed",
    "MessageCancelled",
    "InboundMessageReceived",
    "QueuePressureRaised",
    "RetryScheduled",
    "ReconnectScheduled",
    "StreamGapDetected",
    "SecurityActionRequired",
    "FatalErrorRaised",
];

const ALLOWED_ERROR_CODES: &[&str] = &[
    "SDK_APP_VALIDATION_INVALID_ARGUMENT",
    "SDK_APP_VALIDATION_UNKNOWN_FIELD",
    "SDK_APP_CAPABILITY_UNSUPPORTED_PROFILE",
    "SDK_APP_CAPABILITY_REQUIRED_FEATURE_MISSING",
    "SDK_APP_CONFIG_INVALID",
    "SDK_APP_RUNTIME_INVALID_STATE",
    "SDK_APP_RUNTIME_ALREADY_RUNNING_DIFFERENT_CONFIG",
    "SDK_APP_RUNTIME_STREAM_DEGRADED",
    "SDK_APP_RUNTIME_NOT_STARTED",
    "SDK_APP_DELIVERY_QUEUE_PRESSURE",
    "SDK_APP_DELIVERY_PARTIAL_ACCEPTANCE",
    "SDK_APP_DELIVERY_RETRY_EXHAUSTED",
    "SDK_APP_DELIVERY_CANCELLED",
    "SDK_APP_CONNECTIVITY_DISCONNECTED",
    "SDK_APP_CONNECTIVITY_RECONNECT_FAILED",
    "SDK_APP_PERSISTENCE_UNAVAILABLE",
    "SDK_APP_PERSISTENCE_RECOVERY_REQUIRED",
    "SDK_APP_TIMEOUT_OPERATION_EXPIRED",
    "SDK_APP_SECURITY_AUTH_REQUIRED",
    "SDK_APP_SECURITY_AUTHZ_DENIED",
    "SDK_APP_SECURITY_REDACTION_REQUIRED",
    "SDK_APP_INTERNAL_UNEXPECTED_FAILURE",
];

const ALLOWED_CATEGORIES: &[&str] = &[
    "Validation",
    "Capability",
    "Config",
    "Policy",
    "Delivery",
    "Connectivity",
    "Persistence",
    "Security",
    "Timeout",
    "Runtime",
    "Internal",
];

const ALLOWED_PROFILES: &[&str] =
    &["mobile_default", "desktop_default", "embedded_default", "testing_default"];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("workspace root")
        .to_path_buf()
}

fn fixture_dir() -> PathBuf {
    workspace_root().join("docs/fixtures/sdk-app-v1")
}

fn contract_doc(name: &str) -> String {
    fs::read_to_string(workspace_root().join("docs/contracts").join(name))
        .unwrap_or_else(|err| panic!("failed to read contract {name}: {err}"))
}

fn read_json(path: &Path) -> JsonValue {
    serde_json::from_str(
        &fs::read_to_string(path)
            .unwrap_or_else(|err| panic!("failed to read fixture {}: {err}", path.display())),
    )
    .unwrap_or_else(|err| panic!("failed to parse fixture {}: {err}", path.display()))
}

fn fixture(name: &str) -> JsonValue {
    read_json(&fixture_dir().join(name))
}

fn read_workspace_text(path: &str) -> String {
    let full_path = workspace_root().join(path);
    fs::read_to_string(&full_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", full_path.display()))
}

#[test]
fn sdk_conformance_app_mode_manifest_covers_required_scenarios() {
    let manifest = fixture("manifest.json");
    assert_eq!(
        manifest["fixture_schema_version"].as_u64(),
        Some(1),
        "fixture schema version must be frozen"
    );
    assert_eq!(manifest["contract_family"].as_str(), Some("sdk-app"));
    assert_eq!(manifest["contract_release"].as_str(), Some("v1"));

    let scenarios = manifest["scenarios"].as_array().expect("manifest scenarios");
    let mut seen = BTreeSet::new();
    let mut paths = BTreeSet::new();
    for scenario in scenarios {
        let id = scenario["id"].as_str().expect("scenario id");
        let kind = scenario["kind"].as_str().expect("scenario kind");
        let path = scenario["path"].as_str().expect("scenario path");
        assert!(seen.insert(id.to_owned()), "duplicate scenario id {id}");
        assert!(paths.insert(path.to_owned()), "duplicate scenario path {path}");
        assert!(fixture_dir().join(path).is_file(), "missing fixture file for {id}: {path}");

        let body = fixture(path);
        assert_eq!(
            body["scenario_id"].as_str(),
            Some(id),
            "fixture scenario_id mismatch for {path}"
        );
        assert_eq!(body["kind"].as_str(), Some(kind), "fixture kind mismatch for {path}");
    }

    assert_eq!(seen.len(), REQUIRED_SCENARIOS.len());
    for required in REQUIRED_SCENARIOS {
        assert!(seen.contains(*required), "missing required app-api scenario {required}");
    }
}

#[test]
fn sdk_easy_golden_paths_reference_conformance_scenarios() {
    let required_paths = [
        "examples/sdk-easy/rust-managed/Cargo.toml",
        "examples/sdk-easy/rust-managed/src/main.rs",
        "examples/sdk-easy/rust-managed/README.md",
        "examples/sdk-easy/kotlin-mobile/Main.kt",
        "examples/sdk-easy/kotlin-mobile/README.md",
        "docs/sdk/migration-to-easy.md",
    ];
    for path in required_paths {
        assert!(workspace_root().join(path).is_file(), "missing #33 golden-path artifact: {path}");
    }

    let rust_main = read_workspace_text("examples/sdk-easy/rust-managed/src/main.rs");
    for required in [
        "Config::desktop_default()",
        "start_async",
        "SubscriptionStart::Tail",
        "send_async",
        "EventKind::MessageDelivered",
        "EventKind::StreamGapDetected",
        "delivery.queue_pressure",
        "events.delivery_ordering",
    ] {
        assert!(rust_main.contains(required), "rust example missing required marker {required}");
    }

    let kotlin_main = read_workspace_text("examples/sdk-easy/kotlin-mobile/Main.kt");
    for required in [
        "mobile_default",
        "start",
        "subscribeEvents",
        "send",
        "MessageDelivered",
        "StreamGapDetected",
        "lifecycle.start_stop_restart",
        "events.delivery_ordering",
    ] {
        assert!(
            kotlin_main.contains(required),
            "kotlin example missing required marker {required}"
        );
    }

    let migration = read_workspace_text("docs/sdk/migration-to-easy.md");
    for scenario in REQUIRED_SCENARIOS {
        assert!(
            migration.contains(scenario),
            "migration guide must reference conformance scenario {scenario}"
        );
    }
}

#[test]
fn sdk_first_party_kotlin_wrapper_exposes_easy_mode_contract() {
    let required_paths = [
        "wrappers/kotlin-mobile/README.md",
        "wrappers/kotlin-mobile/conformance-manifest.json",
        "wrappers/kotlin-mobile/src/main/kotlin/org/freetakteam/lxmf/easy/LxmfEasyClient.kt",
        "wrappers/kotlin-mobile/src/test/kotlin/org/freetakteam/lxmf/easy/LxmfEasyConformanceTest.kt",
    ];
    for path in required_paths {
        assert!(workspace_root().join(path).is_file(), "missing #31 wrapper artifact: {path}");
    }

    let client = read_workspace_text(
        "wrappers/kotlin-mobile/src/main/kotlin/org/freetakteam/lxmf/easy/LxmfEasyClient.kt",
    );
    for required in [
        "class LxmfEasyClient",
        "interface LxmfEasyBackend",
        "Flow<LxmfEvent>",
        "suspend fun start",
        "suspend fun send",
        "suspend fun stop",
        "AutoCloseable",
        "sealed class LxmfEasyError",
        "mobile_default",
        "QueuePressureRaised",
        "StreamGapDetected",
    ] {
        assert!(client.contains(required), "kotlin wrapper missing required API marker {required}");
    }
    assert!(
        !client.contains("Pseudocode"),
        "first-party wrapper source must not remain a pseudocode sketch"
    );

    let manifest =
        read_json(&workspace_root().join("wrappers/kotlin-mobile/conformance-manifest.json"));
    assert_eq!(manifest["wrapper"].as_str(), Some("kotlin-mobile"));
    assert_eq!(manifest["contract_family"].as_str(), Some("sdk-app"));
    assert_eq!(manifest["contract_release"].as_str(), Some("v1"));
    let scenarios = manifest["scenarios"].as_array().expect("wrapper scenarios");
    let seen = scenarios
        .iter()
        .map(|scenario| scenario.as_str().expect("scenario id"))
        .collect::<BTreeSet<_>>();
    for scenario in REQUIRED_SCENARIOS {
        assert!(seen.contains(scenario), "kotlin wrapper manifest missing scenario {scenario}");
    }

    let tests =
        read_workspace_text("wrappers/kotlin-mobile/src/test/kotlin/org/freetakteam/lxmf/easy/LxmfEasyConformanceTest.kt");
    for scenario in REQUIRED_SCENARIOS {
        assert!(tests.contains(scenario), "kotlin wrapper test missing scenario {scenario}");
    }
    for required in [
        "conformance-manifest.json",
        "docs/fixtures/sdk-app-v1",
        "Files.readString",
        "Files.isRegularFile",
    ] {
        assert!(
            tests.contains(required),
            "kotlin wrapper test must exercise shared fixture integration marker {required}"
        );
    }
}

#[test]
fn sdk_wrapper_parity_release_gate_is_wired_for_ci_and_releases() {
    let registry = read_json(&workspace_root().join("wrappers/wrapper-conformance.json"));
    assert_eq!(registry["contract_family"].as_str(), Some("sdk-app"));
    assert_eq!(registry["contract_release"].as_str(), Some("v1"));

    let wrappers = registry["wrappers"].as_array().expect("wrapper registry entries");
    let kotlin = wrappers
        .iter()
        .find(|wrapper| wrapper["id"].as_str() == Some("kotlin-mobile"))
        .expect("kotlin-mobile wrapper registry entry");
    assert_eq!(
        kotlin["manifest"].as_str(),
        Some("wrappers/kotlin-mobile/conformance-manifest.json")
    );
    assert_eq!(
        kotlin["fixture_root"].as_str(),
        Some("docs/fixtures/sdk-app-v1"),
        "wrapper registry should point every wrapper at the shared fixture root"
    );

    let scenarios = kotlin["scenarios"].as_array().expect("kotlin wrapper scenarios");
    let seen = scenarios
        .iter()
        .map(|scenario| scenario.as_str().expect("scenario id"))
        .collect::<BTreeSet<_>>();
    for scenario in REQUIRED_SCENARIOS {
        assert!(seen.contains(scenario), "wrapper registry missing scenario {scenario}");
    }

    let ci = read_workspace_text(".github/workflows/ci.yml");
    assert!(
        ci.contains("SDK wrapper parity gate")
            && ci.contains("cargo test -p test-support sdk_wrapper_parity_release_gate"),
        "pull-request CI must run the SDK wrapper parity gate"
    );

    let release = read_workspace_text(".github/workflows/release-bundles.yml");
    assert!(
        release.contains("sdk-wrapper-parity")
            && release.contains("cargo test -p test-support sdk_wrapper_parity_release_gate")
            && release.contains("needs: [validate-release-version, sdk-wrapper-parity]"),
        "release bundles must fail before packaging when wrapper parity drifts"
    );
}

#[test]
fn sdk_kotlin_wrapper_has_executable_gradle_conformance_harness() {
    for path in [
        "wrappers/kotlin-mobile/settings.gradle.kts",
        "wrappers/kotlin-mobile/build.gradle.kts",
        "wrappers/kotlin-mobile/src/test/kotlin/org/freetakteam/lxmf/easy/LxmfEasyConformanceTest.kt",
    ] {
        assert!(workspace_root().join(path).is_file(), "missing Kotlin wrapper build artifact: {path}");
    }

    let build = read_workspace_text("wrappers/kotlin-mobile/build.gradle.kts");
    for required in [
        "kotlin(\"jvm\")",
        "kotlinx-coroutines-core",
        "kotlin(\"test-junit5\")",
        "useJUnitPlatform()",
    ] {
        assert!(build.contains(required), "Kotlin wrapper build missing {required}");
    }

    let ci = read_workspace_text(".github/workflows/ci.yml");
    assert!(
        ci.contains("kotlin-wrapper-conformance")
            && ci.contains("gradle/actions/setup-gradle@v4")
            && ci.contains("gradle -p wrappers/kotlin-mobile test"),
        "CI must execute the Kotlin wrapper conformance tests"
    );
}

#[test]
fn sdk_quickstart_links_easy_mode_golden_paths() {
    let quickstart = read_workspace_text("docs/sdk/quickstart.md");
    for required in [
        "examples/sdk-easy/rust-managed",
        "examples/sdk-easy/kotlin-mobile",
        "wrappers/kotlin-mobile",
        "docs/sdk/migration-to-easy.md",
        "docs/fixtures/sdk-app-v1/manifest.json",
    ] {
        assert!(quickstart.contains(required), "quickstart missing #33 link {required}");
    }
}

#[test]
fn sdk_conformance_app_mode_fixtures_match_contract_vocabularies() {
    let event_contract = contract_doc("sdk-app-events-v1.md");
    let error_contract = contract_doc("sdk-app-errors-v1.md");
    let profile_contract = contract_doc("sdk-app-profiles-v1.md");
    let manifest = fixture("manifest.json");

    for scenario in manifest["scenarios"].as_array().expect("manifest scenarios") {
        let path = scenario["path"].as_str().expect("fixture path");
        let body = fixture(path);
        assert_eq!(body["contract_release"].as_str(), Some("v1"), "{path}");

        if let Some(profile) = body["profile"].as_str() {
            assert!(ALLOWED_PROFILES.contains(&profile), "unexpected profile {profile} in {path}");
            assert!(profile_contract.contains(profile), "profile {profile} missing from contract");
        }

        if let Some(events) = body["expected_events"].as_array() {
            for event in events {
                let event = event.as_str().expect("event name");
                assert!(ALLOWED_EVENTS.contains(&event), "unexpected event {event} in {path}");
                assert!(event_contract.contains(event), "event {event} missing from contract");
            }
        }

        if let Some(error) = body["expected_error"].as_str() {
            assert!(
                ALLOWED_ERROR_CODES.contains(&error),
                "unexpected error code {error} in {path}"
            );
            assert!(error_contract.contains(error), "error code {error} missing from contract");
        }

        if let Some(mappings) = body["mappings"].as_array() {
            for mapping in mappings {
                let code = mapping["code"].as_str().expect("mapping code");
                let category = mapping["category"].as_str().expect("mapping category");
                assert!(ALLOWED_ERROR_CODES.contains(&code), "unexpected mapped code {code}");
                assert!(ALLOWED_CATEGORIES.contains(&category), "unexpected category {category}");
                assert!(error_contract.contains(code), "mapped code {code} missing from contract");
                assert!(
                    error_contract.contains(category),
                    "mapped category {category} missing from contract"
                );
            }
        }
    }
}

#[test]
fn sdk_conformance_app_mode_lifecycle_fixture_freezes_restart_and_waiter_wakeup() {
    let fixture = fixture("lifecycle.start_stop_restart.json");
    assert_eq!(fixture["scenario_id"].as_str(), Some("lifecycle.start_stop_restart"));
    assert_eq!(fixture["kind"].as_str(), Some("lifecycle"));

    let actions = fixture["actions"].as_array().expect("lifecycle actions");
    let actions = actions.iter().map(|value| value.as_str().expect("action")).collect::<Vec<_>>();
    assert_eq!(actions, vec!["start", "stop", "restart"]);

    let events = fixture["expected_events"]
        .as_array()
        .expect("lifecycle expected events")
        .iter()
        .map(|value| value.as_str().expect("event"))
        .collect::<Vec<_>>();
    assert_eq!(events, vec!["RuntimeStarted", "RuntimeStopped", "RuntimeStarted"]);

    let assertions = fixture["assertions"].as_object().expect("lifecycle assertions");
    assert_eq!(
        assertions.get("restart_increments_runtime_id").and_then(JsonValue::as_bool),
        Some(true)
    );
    assert_eq!(assertions.get("stop_is_idempotent").and_then(JsonValue::as_bool), Some(true));
    assert_eq!(
        assertions.get("blocked_waits_resolve_on_stop").and_then(JsonValue::as_bool),
        Some(true)
    );
    assert_eq!(
        assertions.get("blocked_waits_resolve_on_restart").and_then(JsonValue::as_bool),
        Some(true)
    );
}

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
