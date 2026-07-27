use super::*;
use rns_rpc::rpc::zmq::ZmqRpcEnvelopeKind;
use rns_rpc::rpc::{RpcRequest, RpcResponse};
use std::net::TcpListener;
use std::sync::{mpsc, Arc, Mutex};
use zeromq::{PullSocket, PushSocket, Socket, SocketRecv, SocketSend, ZmqMessage};

mod batch;
mod cancel;
mod delivery_stamp_policy;
mod delivery_trace;
mod destination;
mod domains;
mod history;
mod identity;
mod propagation;
mod propagation_payload;
mod propagation_policy;
mod registry;
mod status;
mod ticket;
mod workflow;

#[derive(Debug, Clone, PartialEq)]
struct CapturedZmqRequest {
    method: String,
    params: Option<JsonValue>,
}

#[test]
fn config_rejects_remote_endpoints_without_auth() {
    let config =
        ZmqPipelineBackendConfig::local_tcp("tcp://192.0.2.10:9000", "tcp://127.0.0.1:9001");

    let err = config.validate().expect_err("remote without auth rejected");

    assert_eq!(err.category, ErrorCategory::Security);
    assert_eq!(err.machine_code, code::SECURITY_AUTH_REQUIRED);
}

#[test]
fn config_accepts_loopback_without_auth() {
    let config =
        ZmqPipelineBackendConfig::local_tcp("tcp://127.0.0.1:9000", "tcp://localhost:9001");

    config.validate().expect("loopback accepted");
}

#[test]
fn config_normalizes_ipv4_loopback_for_windows_tcp_bind() {
    let config =
        ZmqPipelineBackendConfig::local_tcp("tcp://127.0.0.1:9000", "tcp://127.0.0.1:9001");

    assert_eq!(config.command_endpoint, "tcp://localhost:9000");
    assert_eq!(config.response_endpoint, "tcp://localhost:9001");
}

#[test]
fn response_filter_requires_session_and_request_match() {
    let session = "session-a".to_string();
    let envelope = ZmqRpcEnvelope::response(session.clone(), 4, Vec::new());

    assert_eq!(envelope.kind, ZmqRpcEnvelopeKind::Response);
    assert_eq!(envelope.session_id, session);
    assert_eq!(envelope.request_id, 4);
}

#[test]
fn token_auth_metadata_matches_daemon_bearer_claim_shape() {
    let mut config =
        ZmqPipelineBackendConfig::local_tcp("tcp://127.0.0.1:9000", "tcp://127.0.0.1:9001");
    config.token_auth = Some(ZmqPipelineTokenAuth {
        issuer: "test-issuer".to_string(),
        audience: "test-audience".to_string(),
        shared_secret: "test-secret".to_string(),
        ttl_secs: 60,
    });
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let auth = client.auth_metadata_for_request(7).expect("system time ok").expect("auth metadata");
    let claims = parse_claims(&auth.value);
    let signed_payload = format!(
        "iss={};aud={};jti={};sub={};iat={};exp={}",
        claims.get("iss").expect("issuer"),
        claims.get("aud").expect("audience"),
        claims.get("jti").expect("jti"),
        claims.get("sub").expect("subject"),
        claims.get("iat").expect("iat"),
        claims.get("exp").expect("exp")
    );
    let expected_sig = token_signature("test-secret", &signed_payload);
    let expected_jti = format!("{}-7", client.session_id());

    assert_eq!(auth.scheme, "bearer");
    assert_eq!(claims.get("iss").map(String::as_str), Some("test-issuer"));
    assert_eq!(claims.get("aud").map(String::as_str), Some("test-audience"));
    assert_eq!(claims.get("jti").map(String::as_str), Some(expected_jti.as_str()));
    assert_eq!(claims.get("sub").map(String::as_str), Some("sdk-client"));
    assert_eq!(claims.get("sig").map(String::as_str), Some(expected_sig.as_str()));
}

#[test]
fn negotiate_with_token_auth_preserves_remote_token_runtime_config() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({
            "runtime_id": "runtime-zmq-token",
            "active_contract_version": 2,
            "effective_capabilities": [],
            "effective_limits": {
                "max_poll_events": 64,
                "max_event_bytes": 32768,
                "max_batch_bytes": 1048576,
                "max_extension_keys": 32,
                "idempotency_ttl_ms": 60000
            },
            "contract_release": "v2",
            "schema_namespace": "sdk.v2",
            "sdk_version": "9.8.7-test",
            "python_reference": {
                "reticulum_conformance_ref": "conformance-test-ref",
                "python_reticulum_version": "1.2.2-test",
                "python_reticulum_ref": "reticulum-test-ref",
                "python_lxmf_version": "0.9.6-test",
                "python_lxmf_ref": "lxmf-test-ref"
            },
            "software_parity": crate::current_software_parity_orientation()
        }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    config.token_auth = Some(ZmqPipelineTokenAuth {
        issuer: "test-issuer".to_string(),
        audience: "test-audience".to_string(),
        shared_secret: "test-secret".to_string(),
        ttl_secs: 60,
    });
    let backend = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let response = backend
        .negotiate(crate::capability::NegotiationRequest {
            supported_contract_versions: vec![2],
            requested_capabilities: Vec::new(),
            profile: crate::types::Profile::DesktopLocalRuntime,
            bind_mode: crate::types::BindMode::LocalOnly,
            auth_mode: crate::types::AuthMode::LocalTrusted,
            overflow_policy: crate::types::OverflowPolicy::Reject,
            block_timeout_ms: None,
            rpc_backend: None,
            extensions: Default::default(),
        })
        .expect("negotiate");

    assert_eq!(response.runtime_id, "runtime-zmq-token");
    assert_eq!(response.sdk_version, "9.8.7-test");
    assert_eq!(response.python_reference.python_reticulum_version.as_deref(), Some("1.2.2-test"));
    assert_eq!(response.python_reference.python_lxmf_version.as_deref(), Some("0.9.6-test"));
    assert_eq!(response.python_reference.python_lxmf_ref, "lxmf-test-ref");
    assert_eq!(response.software_parity, Some(crate::current_software_parity_orientation()));
    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_negotiate_v2");
    let config = &request.params.as_ref().expect("params")["config"];
    assert_eq!(config["bind_mode"], json!("remote"));
    assert_eq!(config["auth_mode"], json!("token"));
    assert_eq!(config["rpc_backend"]["token_auth"]["issuer"], json!("test-issuer"));
    assert_eq!(config["rpc_backend"]["token_auth"]["audience"], json!("test-audience"));
    assert_eq!(config["rpc_backend"]["token_auth"]["shared_secret"], json!("test-secret"));
    server.join().expect("server joined");
}

#[test]
fn negotiate_without_reported_parity_metadata_falls_back_to_local_constants() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({
            "runtime_id": "runtime-zmq-legacy",
            "active_contract_version": 2,
            "effective_capabilities": [],
            "effective_limits": {
                "max_poll_events": 64,
                "max_event_bytes": 32768,
                "max_batch_bytes": 1048576,
                "max_extension_keys": 32,
                "idempotency_ttl_ms": 60000
            },
            "contract_release": "v2",
            "schema_namespace": "sdk.v2"
        }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let backend = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let response = backend
        .negotiate(crate::capability::NegotiationRequest {
            supported_contract_versions: vec![2],
            requested_capabilities: Vec::new(),
            profile: crate::types::Profile::DesktopLocalRuntime,
            bind_mode: crate::types::BindMode::LocalOnly,
            auth_mode: crate::types::AuthMode::LocalTrusted,
            overflow_policy: crate::types::OverflowPolicy::Reject,
            block_timeout_ms: None,
            rpc_backend: None,
            extensions: Default::default(),
        })
        .expect("negotiate");

    assert_eq!(response.sdk_version, crate::SDK_VERSION);
    assert_eq!(
        response.python_reference.python_reticulum_ref,
        crate::PYTHON_RETICULUM_REFERENCE_REF
    );
    assert_eq!(
        response.python_reference.python_reticulum_version.as_deref(),
        Some(crate::PYTHON_RETICULUM_REFERENCE_VERSION)
    );
    assert_eq!(response.software_parity, None);
    server.join().expect("server joined");
}

#[test]
fn identity_announce_now_uses_zmq_sdk_method() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({ "accepted": true, "announce_id": 1 }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let ack = client.identity_announce_now().expect("identity announce");

    assert!(ack.accepted);
    assert_eq!(
        captured.lock().expect("captured request").as_ref().expect("zmq request").method,
        "sdk_identity_announce_now_v2"
    );
    assert_eq!(
        captured.lock().expect("captured request").as_ref().expect("zmq request").params,
        Some(json!({}))
    );
    server.join().expect("server joined");
}

#[test]
fn identity_announce_preserves_display_and_capability_metadata() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({
            "announce": {
                "accepted": true,
                "announce_id": "ann-rem-rch-1",
                "identity": "local-identity",
                "display_name": "Field Team One",
                "capabilities": ["rem.direct_chat", "rem.restart_recovery"],
                "metadata": {
                    "callsign": "FT1",
                    "rem_capability_flags": ["direct_chat", "restart_recovery"],
                    "rch_announce_slots": ["broadcast", "topics"]
                },
                "extensions": {
                    "source": "zmq"
                }
            }
        }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let result = client
        .identity_announce(crate::IdentityAnnounceRequest {
            identity: Some(crate::IdentityRef("local-identity".to_owned())),
            display_name: Some("Field Team One".to_owned()),
            capabilities: vec!["rem.direct_chat".to_owned(), "rem.restart_recovery".to_owned()],
            metadata: BTreeMap::from([
                ("callsign".to_owned(), json!("FT1")),
                ("rem_capability_flags".to_owned(), json!(["direct_chat", "restart_recovery"])),
                ("rch_announce_slots".to_owned(), json!(["broadcast", "topics"])),
            ]),
            extensions: BTreeMap::from([("source".to_owned(), json!("rem-rch"))]),
        })
        .expect("identity announce");

    assert!(result.accepted);
    assert_eq!(result.announce_id.as_ref(), Some(&json!("ann-rem-rch-1")));
    assert_eq!(
        result.identity.as_ref().map(|identity| identity.0.as_str()),
        Some("local-identity")
    );
    assert_eq!(result.display_name.as_deref(), Some("Field Team One"));
    assert_eq!(result.capabilities[0], "rem.direct_chat");
    assert_eq!(result.metadata["callsign"], json!("FT1"));
    assert_eq!(result.metadata["rch_announce_slots"], json!(["broadcast", "topics"]));
    assert_eq!(result.extensions["source"], json!("zmq"));

    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_identity_announce_now_v2");
    let params = request.params.as_ref().expect("params");
    assert_eq!(params["identity"], json!("local-identity"));
    assert_eq!(params["display_name"], json!("Field Team One"));
    assert_eq!(params["capabilities"][1], json!("rem.restart_recovery"));
    assert_eq!(params["metadata"]["callsign"], json!("FT1"));
    assert_eq!(params["metadata"]["rem_capability_flags"][0], json!("direct_chat"));
    assert_eq!(params["metadata"]["rch_announce_slots"][1], json!("topics"));
    assert_eq!(params["extensions"]["source"], json!("rem-rch"));
    server.join().expect("server joined");
}

#[test]
fn identity_list_uses_zmq_sdk_method_and_decodes_identity_bundles() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({
            "identities": [{
                "identity": "local-identity",
                "public_key": "pubkey-base64",
                "display_name": "Local Operator",
                "capabilities": ["direct_chat", "identity_discovery"],
                "extensions": { "source": "zmq" }
            }]
        }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let identities = client.identity_list().expect("identity list");

    assert_eq!(identities.len(), 1);
    assert_eq!(identities[0].identity.0, "local-identity");
    assert_eq!(identities[0].display_name.as_deref(), Some("Local Operator"));
    assert_eq!(identities[0].capabilities, vec!["direct_chat", "identity_discovery"]);
    assert_eq!(identities[0].extensions["source"], json!("zmq"));
    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_identity_list_v2");
    assert_eq!(request.params.as_ref().expect("params"), &json!({}));
    server.join().expect("server joined");
}

#[test]
fn identity_activate_uses_zmq_sdk_method() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({ "accepted": true, "revision": 7 }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let ack = client
        .identity_activate(crate::domain::IdentityRef("local-identity".to_owned()))
        .expect("identity activate");

    assert!(ack.accepted);
    assert_eq!(ack.revision, Some(7));
    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_identity_activate_v2");
    assert_eq!(request.params.as_ref().expect("params")["identity"], json!("local-identity"));
    server.join().expect("server joined");
}

#[test]
fn identity_import_uses_zmq_sdk_method_and_decodes_identity_bundle() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({
            "identity": {
                "identity": "imported-identity",
                "public_key": "imported-pubkey",
                "display_name": "Imported Peer",
                "capabilities": ["direct_chat"],
                "extensions": { "source": "import" }
            }
        }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let identity = client
        .identity_import(crate::domain::IdentityImportRequest {
            bundle_base64: "aW1wb3J0ZWQ=".to_owned(),
            passphrase: Some("secret".to_owned()),
            display_name: None,
            capabilities: Vec::new(),
            metadata: BTreeMap::new(),
            extensions: BTreeMap::from([("source".to_owned(), json!("rem-recovery"))]),
        })
        .expect("identity import");

    assert_eq!(identity.identity.0, "imported-identity");
    assert_eq!(identity.public_key, "imported-pubkey");
    assert_eq!(identity.display_name.as_deref(), Some("Imported Peer"));
    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_identity_import_v2");
    let params = request.params.as_ref().expect("params");
    assert_eq!(params["bundle_base64"], json!("aW1wb3J0ZWQ="));
    assert_eq!(params["passphrase"], json!("secret"));
    assert_eq!(params["extensions"]["source"], json!("rem-recovery"));
    server.join().expect("server joined");
}

#[test]
fn identity_export_uses_zmq_sdk_method_and_decodes_bundle() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({
            "bundle": {
                "bundle_base64": "ZXhwb3J0ZWQ=",
                "passphrase": null,
                "extensions": { "format": "portable" }
            }
        }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let bundle = client
        .identity_export(crate::domain::IdentityRef("local-identity".to_owned()))
        .expect("identity export");

    assert_eq!(bundle.bundle_base64, "ZXhwb3J0ZWQ=");
    assert_eq!(bundle.passphrase, None);
    assert_eq!(bundle.extensions["format"], json!("portable"));
    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_identity_export_v2");
    assert_eq!(request.params.as_ref().expect("params")["identity"], json!("local-identity"));
    server.join().expect("server joined");
}

#[test]
fn identity_presence_list_uses_zmq_sdk_method_and_decodes_response() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({
            "presence_list": {
                "peers": [{
                    "peer_id": "peer-a",
                    "last_seen_ts_ms": 2000,
                    "first_seen_ts_ms": 1000,
                    "seen_count": 3,
                    "name": "Peer A",
                    "name_source": "announce",
                    "trust_level": "trusted",
                    "bootstrap": true,
                    "extensions": { "source": "zmq" }
                }],
                "next_cursor": "presence:1"
            }
        }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let result = client
        .identity_presence_list(crate::domain::PresenceListRequest {
            cursor: Some("presence:0".to_owned()),
            limit: Some(1),
            min_last_seen_ts_ms: None,
            extensions: BTreeMap::new(),
        })
        .expect("identity presence list");

    assert_eq!(result.next_cursor.as_deref(), Some("presence:1"));
    assert_eq!(result.peers[0].peer_id, "peer-a");
    assert_eq!(result.peers[0].trust_level, Some(crate::domain::TrustLevel::Trusted));
    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_identity_presence_list_v2");
    assert_eq!(request.params.as_ref().expect("params")["cursor"], json!("presence:0"));
    assert_eq!(request.params.as_ref().expect("params")["limit"], json!(1));
    server.join().expect("server joined");
}

#[test]
fn identity_contact_update_uses_zmq_sdk_method_and_decodes_response() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({
            "contact": {
                "identity": "peer-contact",
                "display_name": "RCH Relay",
                "trust_level": "trusted",
                "bootstrap": true,
                "updated_ts_ms": 3000,
                "metadata": {
                    "capabilities": ["rch.announce_slot"],
                    "callsign": "RCH-1"
                },
                "extensions": { "source": "zmq" }
            }
        }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let contact = client
        .identity_contact_update(crate::domain::ContactUpdateRequest {
            identity: crate::domain::IdentityRef("peer-contact".to_owned()),
            display_name: Some("RCH Relay".to_owned()),
            trust_level: Some(crate::domain::TrustLevel::Trusted),
            bootstrap: Some(true),
            metadata: BTreeMap::from([(
                "callsign".to_owned(),
                JsonValue::String("RCH-1".to_owned()),
            )]),
            extensions: BTreeMap::from([(
                "source".to_owned(),
                JsonValue::String("zmq".to_owned()),
            )]),
        })
        .expect("identity contact update");

    assert_eq!(contact.identity.0, "peer-contact");
    assert_eq!(contact.display_name.as_deref(), Some("RCH Relay"));
    assert_eq!(contact.trust_level, crate::domain::TrustLevel::Trusted);
    assert!(contact.bootstrap);
    assert_eq!(contact.metadata["callsign"], json!("RCH-1"));
    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_identity_contact_update_v2");
    let params = request.params.as_ref().expect("params");
    assert_eq!(params["identity"], json!("peer-contact"));
    assert_eq!(params["display_name"], json!("RCH Relay"));
    assert_eq!(params["trust_level"], json!("trusted"));
    assert_eq!(params["bootstrap"], json!(true));
    assert_eq!(params["metadata"]["callsign"], json!("RCH-1"));
    assert_eq!(params["extensions"]["source"], json!("zmq"));
    server.join().expect("server joined");
}

#[test]
fn identity_contact_list_uses_zmq_sdk_method_and_decodes_response() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({
            "contact_list": {
                "contacts": [{
                    "identity": "peer-contact",
                    "display_name": "REM Phone",
                    "trust_level": "trusted",
                    "bootstrap": false,
                    "updated_ts_ms": 4000,
                    "metadata": {
                        "capabilities": ["rem.peer"],
                        "callsign": "REM-1"
                    },
                    "extensions": { "source": "zmq" }
                }],
                "next_cursor": "contact:1"
            }
        }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let result = client
        .identity_contact_list(crate::domain::ContactListRequest {
            cursor: Some("contact:0".to_owned()),
            limit: Some(1),
            extensions: BTreeMap::from([(
                "source".to_owned(),
                JsonValue::String("zmq".to_owned()),
            )]),
        })
        .expect("identity contact list");

    assert_eq!(result.next_cursor.as_deref(), Some("contact:1"));
    assert_eq!(result.contacts[0].identity.0, "peer-contact");
    assert_eq!(result.contacts[0].display_name.as_deref(), Some("REM Phone"));
    assert_eq!(result.contacts[0].metadata["callsign"], json!("REM-1"));
    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_identity_contact_list_v2");
    let params = request.params.as_ref().expect("params");
    assert_eq!(params["cursor"], json!("contact:0"));
    assert_eq!(params["limit"], json!(1));
    assert_eq!(params["extensions"]["source"], json!("zmq"));
    server.join().expect("server joined");
}

#[test]
fn identity_resolve_uses_zmq_sdk_method_and_decodes_identity_ref() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({ "identity": "peer-destination-hash" }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let identity = client
        .identity_resolve(crate::domain::IdentityResolveRequest {
            hash: "peer-announced-hash".to_owned(),
            extensions: BTreeMap::from([("requested_by".to_owned(), json!("rem-peer-discovery"))]),
        })
        .expect("identity resolve")
        .expect("resolved identity");

    assert_eq!(identity.0, "peer-destination-hash");
    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_identity_resolve_v2");
    let params = request.params.as_ref().expect("params");
    assert_eq!(params["hash"], json!("peer-announced-hash"));
    assert_eq!(params["extensions"]["requested_by"], json!("rem-peer-discovery"));
    server.join().expect("server joined");
}

#[test]
fn identity_bootstrap_uses_zmq_sdk_method_and_preserves_capability_metadata() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({
            "contact": {
                "identity": "peer-destination-hash",
                "display_name": "Field Team One",
                "trust_level": "trusted",
                "bootstrap": true,
                "updated_ts_ms": 1700003000,
                "metadata": {
                    "callsign": "FT1",
                    "rem_capabilities": ["direct_chat", "restart_recovery"],
                    "rch_announce_slots": ["broadcast", "topics"]
                },
                "extensions": { "source": "bootstrap" }
            }
        }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let contact = client
        .identity_bootstrap(crate::domain::IdentityBootstrapRequest {
            identity: crate::domain::IdentityRef("peer-destination-hash".to_owned()),
            auto_sync: true,
            extensions: BTreeMap::from([(
                "metadata".to_owned(),
                json!({
                    "callsign": "FT1",
                    "rem_capabilities": ["direct_chat", "restart_recovery"],
                    "rch_announce_slots": ["broadcast", "topics"]
                }),
            )]),
        })
        .expect("identity bootstrap");

    assert_eq!(contact.identity.0, "peer-destination-hash");
    assert_eq!(contact.display_name.as_deref(), Some("Field Team One"));
    assert_eq!(contact.metadata["callsign"], json!("FT1"));
    assert_eq!(contact.metadata["rem_capabilities"], json!(["direct_chat", "restart_recovery"]));
    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_identity_bootstrap_v2");
    let params = request.params.as_ref().expect("params");
    assert_eq!(params["identity"], json!("peer-destination-hash"));
    assert_eq!(params["auto_sync"], json!(true));
    assert_eq!(params["extensions"]["metadata"]["callsign"], json!("FT1"));
    assert_eq!(
        params["extensions"]["metadata"]["rch_announce_slots"],
        json!(["broadcast", "topics"])
    );
    server.join().expect("server joined");
}

#[test]
fn send_uses_zmq_sdk_method_and_preserves_delivery_options() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({ "message_id": "sdk-zmq-message-1" }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let message_id = client
        .send(
            SendRequest::new(
                "source-destination",
                "target-destination",
                json!({
                    "title": "RCH",
                    "body": "hello https://example.invalid/incident/42",
                    "9": [{
                        "command_type": "checklist.create.online"
                    }]
                }),
            )
            .with_delivery_method("direct")
            .with_try_propagation_on_fail(true)
            .with_stamp_cost(16)
            .with_include_ticket(true)
            .with_idempotency_key("send-once-incident-42")
            .with_ttl_ms(45_000)
            .with_correlation_id("corr-1")
            .with_extension("incident_id", json!("incident-42")),
        )
        .expect("send");

    assert_eq!(message_id.0, "sdk-zmq-message-1");
    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_send_v2");
    let params = request.params.as_ref().expect("params");
    assert_eq!(params["source"], json!("source-destination"));
    assert_eq!(params["destination"], json!("target-destination"));
    assert_eq!(params["content"], json!("hello https://example.invalid/incident/42"));
    assert_eq!(params["method"], json!("direct"));
    assert_eq!(params["try_propagation_on_fail"], json!(true));
    assert_eq!(params["stamp_cost"], json!(16));
    assert_eq!(params["include_ticket"], json!(true));
    assert_eq!(params["idempotency_key"], json!("send-once-incident-42"));
    assert_eq!(params["ttl_ms"], json!(45_000));
    assert_eq!(params["correlation_id"], json!("corr-1"));
    assert_eq!(params["extensions"]["incident_id"], json!("incident-42"));
    assert_eq!(params["fields"]["9"][0]["command_type"], json!("checklist.create.online"));
    assert_eq!(params["fields"].get("body"), None);
    assert_eq!(params["fields"].get("title"), None);
    assert_eq!(params["fields"].get("_sdk"), None);
    server.join().expect("server joined");
}

#[test]
fn send_message_ids_do_not_collide_across_fresh_zmq_clients() {
    let first_command_endpoint = unused_loopback_endpoint();
    let first_response_endpoint = unused_loopback_endpoint();
    let first_captured = Arc::new(Mutex::new(None));
    let first_server = spawn_single_response_zmq_server(
        first_command_endpoint.clone(),
        json!({ "message_id": "first" }),
        Arc::clone(&first_captured),
    );
    let mut first_config =
        ZmqPipelineBackendConfig::local_tcp(first_command_endpoint, first_response_endpoint);
    first_config.request_timeout = std::time::Duration::from_secs(2);
    let first_client = ZmqPipelineBackendClient::new(first_config).expect("first zmq client");

    first_client
        .send(SendRequest::new(
            "source-destination",
            "first-target",
            json!({ "title": "RCH", "content": "first" }),
        ))
        .expect("first send");

    let second_command_endpoint = unused_loopback_endpoint();
    let second_response_endpoint = unused_loopback_endpoint();
    let second_captured = Arc::new(Mutex::new(None));
    let second_server = spawn_single_response_zmq_server(
        second_command_endpoint.clone(),
        json!({ "message_id": "second" }),
        Arc::clone(&second_captured),
    );
    let mut second_config =
        ZmqPipelineBackendConfig::local_tcp(second_command_endpoint, second_response_endpoint);
    second_config.request_timeout = std::time::Duration::from_secs(2);
    let second_client = ZmqPipelineBackendClient::new(second_config).expect("second zmq client");

    second_client
        .send(SendRequest::new(
            "source-destination",
            "second-target",
            json!({ "title": "RCH", "content": "second" }),
        ))
        .expect("second send");

    let first_id = first_captured
        .lock()
        .expect("first captured")
        .as_ref()
        .expect("first request")
        .params
        .as_ref()
        .expect("first params")["id"]
        .clone();
    let second_id = second_captured
        .lock()
        .expect("second captured")
        .as_ref()
        .expect("second request")
        .params
        .as_ref()
        .expect("second params")["id"]
        .clone();

    assert_ne!(first_id, second_id);
    first_server.join().expect("first server joined");
    second_server.join().expect("second server joined");
}

#[test]
fn send_preserves_documented_lxmf_field_keys_over_zmq_sdk_method() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({ "message_id": "sdk-zmq-message-fields" }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let message_id = client
        .send(SendRequest::new(
            "source-destination",
            "target-destination",
            json!({
                "title": "REM/RCH fields",
                "content": "field preservation",
                "2": { "lat": 1.25, "lon": 2.5 },
                "5": [["image.jpg", [1, 2, 3]]],
                "9": [{ "command_type": "status.request" }],
                "12": [170, 187],
                "14": ["ref-a"],
                "16": { "renderer": "basic" },
                "_lxmf_fields_msgpack_b64": "gqECoQk="
            }),
        ))
        .expect("send");

    assert_eq!(message_id.0, "sdk-zmq-message-fields");
    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_send_v2");
    let fields = &request.params.as_ref().expect("params")["fields"];
    assert_eq!(fields["2"]["lat"], json!(1.25));
    assert_eq!(fields["5"][0][0], json!("image.jpg"));
    assert_eq!(fields["9"][0]["command_type"], json!("status.request"));
    assert_eq!(fields["12"], json!([170, 187]));
    assert_eq!(fields["14"][0], json!("ref-a"));
    assert_eq!(fields["16"]["renderer"], json!("basic"));
    assert_eq!(fields["_lxmf_fields_msgpack_b64"], json!("gqECoQk="));
    assert_eq!(fields.get("title"), None);
    assert_eq!(fields.get("content"), None);
    server.join().expect("server joined");
}

#[test]
fn envelope_execute_uses_zmq_sdk_method_and_preserves_direct_chat_history_query() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({
            "response": {
                "operation_id": "app.message.history.list",
                "kind": "result",
                "accepted": true,
                "correlation_id": "history-corr",
                "payload": {
                    "messages": [{
                        "message_id": "msg-1",
                        "peer_id": "peer-a",
                        "body": "see https://example.invalid/status",
                        "receipt_status": "delivered"
                    }]
                },
                "extensions": {
                    "durable": true,
                    "restart_recovery": true
                }
            }
        }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let response = client
        .envelope_execute(
            crate::app::Envelope::query(
                "app.message.history.list",
                json!({
                    "peer_id": "peer-a",
                    "limit": 25,
                    "include_receipts": true
                }),
            )
            .with_correlation_id("history-corr")
            .with_timeout_ms(1500)
            .with_extension("restart_recovery", json!(true)),
        )
        .expect("history envelope");

    assert_eq!(response.operation_id.as_str(), "app.message.history.list");
    assert!(response.accepted);
    assert_eq!(response.correlation_id.as_deref(), Some("history-corr"));
    assert_eq!(response.payload["messages"][0]["receipt_status"], json!("delivered"));
    assert_eq!(response.extensions["durable"], json!(true));
    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_envelope_execute_v2");
    let params = request.params.as_ref().expect("params");
    assert_eq!(params["operation_id"], json!("app.message.history.list"));
    assert_eq!(params["kind"], json!("query"));
    assert_eq!(params["correlation_id"], json!("history-corr"));
    assert_eq!(params["timeout_ms"], json!(1500));
    assert_eq!(params["payload"]["peer_id"], json!("peer-a"));
    assert_eq!(params["payload"]["include_receipts"], json!(true));
    assert_eq!(params["extensions"]["restart_recovery"], json!(true));
    server.join().expect("server joined");
}

#[test]
fn envelope_execute_uses_zmq_sdk_method_and_preserves_batch_send_results() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({
            "response": {
                "operation_id": "app.delivery.send_batch",
                "kind": "result",
                "accepted": true,
                "correlation_id": "batch-corr",
                "payload": {
                    "batch_id": "batch-zmq-1",
                    "accepted_count": 2,
                    "rejected_count": 0,
                    "results": [
                        {
                            "id": "batch-msg-1",
                            "message_id": "batch-msg-1",
                            "accepted": true
                        },
                        {
                            "id": "batch-msg-2",
                            "message_id": "batch-msg-2",
                            "accepted": true
                        }
                    ]
                },
                "extensions": {
                    "burst_send": true
                }
            }
        }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let response = client
        .envelope_execute(
            crate::app::Envelope::command(
                "app.delivery.send_batch",
                json!({
                    "batch_id": "batch-zmq-1",
                    "source": "source-destination",
                    "messages": [
                        {
                            "id": "batch-msg-1",
                            "destination": "peer-a",
                            "title": "hello a",
                            "content": "payload a"
                        },
                        {
                            "id": "batch-msg-2",
                            "destination": "peer-b",
                            "title": "hello b",
                            "content": "payload b",
                            "method": "direct",
                            "include_ticket": false,
                            "try_propagation_on_fail": true
                        }
                    ]
                }),
            )
            .with_correlation_id("batch-corr")
            .with_extension("burst_send", json!(true)),
        )
        .expect("batch send envelope");

    assert_eq!(response.operation_id.as_str(), "app.delivery.send_batch");
    assert!(response.accepted);
    assert_eq!(response.correlation_id.as_deref(), Some("batch-corr"));
    assert_eq!(response.payload["batch_id"], json!("batch-zmq-1"));
    assert_eq!(response.payload["accepted_count"], json!(2));
    assert_eq!(response.payload["results"][1]["message_id"], json!("batch-msg-2"));
    assert_eq!(response.extensions["burst_send"], json!(true));
    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_envelope_execute_v2");
    let params = request.params.as_ref().expect("params");
    assert_eq!(params["operation_id"], json!("app.delivery.send_batch"));
    assert_eq!(params["kind"], json!("command"));
    assert_eq!(params["correlation_id"], json!("batch-corr"));
    assert_eq!(params["payload"]["batch_id"], json!("batch-zmq-1"));
    assert_eq!(params["payload"]["messages"][1]["method"], json!("direct"));
    assert_eq!(params["payload"]["messages"][1]["include_ticket"], json!(false));
    assert_eq!(params["payload"]["messages"][1]["try_propagation_on_fail"], json!(true));
    assert_eq!(params["extensions"]["burst_send"], json!(true));
    server.join().expect("server joined");
}
fn parse_claims(token: &str) -> BTreeMap<String, String> {
    token
        .split(';')
        .map(|part| {
            let (key, value) = part.split_once('=').expect("claim key/value");
            (key.to_string(), value.to_string())
        })
        .collect()
}

fn unused_loopback_endpoint() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve tcp port");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    format!("tcp://localhost:{port}")
}

fn zmq_cancel_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static GUARD: Mutex<()> = Mutex::new(());
    GUARD.lock().expect("zmq cancel test mutex poisoned")
}

async fn zmq_cancel_async_test_guard() -> tokio::sync::OwnedMutexGuard<()> {
    static GUARD: std::sync::OnceLock<Arc<tokio::sync::Mutex<()>>> = std::sync::OnceLock::new();
    Arc::clone(GUARD.get_or_init(|| Arc::new(tokio::sync::Mutex::new(())))).lock_owned().await
}

fn spawn_single_response_zmq_server(
    command_endpoint: String,
    response: JsonValue,
    captured: Arc<Mutex<Option<CapturedZmqRequest>>>,
) -> std::thread::JoinHandle<()> {
    let (ready_tx, ready_rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().expect("test runtime");
        runtime.block_on(async move {
            let mut commands = PullSocket::new();
            commands.bind(command_endpoint.as_str()).await.expect("bind command endpoint");
            ready_tx.send(()).expect("send zmq server ready");
            let Some(envelope) = recv_request_envelope(&mut commands).await else {
                return;
            };
            let request: RpcRequest =
                rns_rpc::rpc::codec::decode_frame(&envelope.payload).expect("decode rpc request");
            *captured.lock().expect("captured request") =
                Some(CapturedZmqRequest { method: request.method, params: request.params });
            let rpc_response =
                RpcResponse { id: envelope.request_id, result: Some(response), error: None };
            let response_payload =
                rns_rpc::rpc::codec::encode_frame(&rpc_response).expect("encode rpc response");
            let response_endpoint = envelope.response_endpoint.expect("response endpoint");
            let mut responses = PushSocket::new();
            responses.connect(response_endpoint.as_str()).await.expect("connect response endpoint");
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            responses
                .send(ZmqMessage::from(
                    zmq::encode_envelope(&ZmqRpcEnvelope::response(
                        envelope.session_id,
                        envelope.request_id,
                        response_payload,
                    ))
                    .expect("encode zmq response"),
                ))
                .await
                .expect("send response");
        });
    });
    ready_rx.recv().expect("zmq server ready");
    handle
}

fn spawn_response_sequence_zmq_server(
    command_endpoint: String,
    responses: Vec<JsonValue>,
    captured: Arc<Mutex<Vec<CapturedZmqRequest>>>,
) -> std::thread::JoinHandle<()> {
    let (ready_tx, ready_rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().expect("test runtime");
        runtime.block_on(async move {
            let mut commands = PullSocket::new();
            commands.bind(command_endpoint.as_str()).await.expect("bind command endpoint");
            ready_tx.send(()).expect("send zmq server ready");
            for response in responses {
                let Some(envelope) = recv_request_envelope(&mut commands).await else {
                    return;
                };
                let request: RpcRequest = rns_rpc::rpc::codec::decode_frame(&envelope.payload)
                    .expect("decode rpc request");
                captured
                    .lock()
                    .expect("captured requests")
                    .push(CapturedZmqRequest { method: request.method, params: request.params });
                let rpc_response =
                    RpcResponse { id: envelope.request_id, result: Some(response), error: None };
                let response_payload =
                    rns_rpc::rpc::codec::encode_frame(&rpc_response).expect("encode rpc response");
                let response_endpoint = envelope.response_endpoint.expect("response endpoint");
                let mut responses = PushSocket::new();
                responses
                    .connect(response_endpoint.as_str())
                    .await
                    .expect("connect response endpoint");
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                responses
                    .send(ZmqMessage::from(
                        zmq::encode_envelope(&ZmqRpcEnvelope::response(
                            envelope.session_id,
                            envelope.request_id,
                            response_payload,
                        ))
                        .expect("encode zmq response"),
                    ))
                    .await
                    .expect("send response");
            }
        });
    });
    ready_rx.recv().expect("zmq server ready");
    handle
}

async fn recv_request_envelope(commands: &mut PullSocket) -> Option<ZmqRpcEnvelope> {
    let message = tokio::time::timeout(std::time::Duration::from_secs(1), commands.recv())
        .await
        .ok()?
        .ok()?;
    let bytes = Vec::<u8>::try_from(message).ok()?;
    zmq::decode_envelope(&bytes).ok()
}
use std::collections::BTreeMap;
