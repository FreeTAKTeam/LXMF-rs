use super::*;

#[test]
fn workflow_peer_ready_uses_zmq_sdk_method_and_preserves_contact_metadata() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({
            "workflow": {
                "identity": "peer-ready",
                "contact": {
                    "identity": "peer-ready",
                    "display_name": "RCH Relay",
                    "trust_level": "trusted",
                    "bootstrap": true,
                    "updated_ts_ms": 1700000400,
                    "metadata": {
                        "callsign": "RCH-1",
                        "capabilities": ["rem.direct_chat", "rch.announce_slot"]
                    },
                    "extensions": {
                        "source": "zmq"
                    }
                },
                "was_created": true,
                "announced": true
            }
        }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let result = client
        .workflow_peer_ready(crate::WorkflowPeerReadyRequest {
            identity: crate::IdentityRef("peer-ready".to_owned()),
            display_name: Some("RCH Relay".to_owned()),
            trust_level: Some(crate::TrustLevel::Trusted),
            bootstrap: Some(true),
            announce: Some(true),
            metadata: BTreeMap::from([
                ("callsign".to_owned(), json!("RCH-1")),
                ("capabilities".to_owned(), json!(["rem.direct_chat", "rch.announce_slot"])),
            ]),
            extensions: BTreeMap::from([("source".to_owned(), json!("rem-rch"))]),
        })
        .expect("workflow peer ready");

    assert_eq!(result.identity.0, "peer-ready");
    assert_eq!(result.contact.display_name.as_deref(), Some("RCH Relay"));
    assert_eq!(result.contact.metadata["callsign"], json!("RCH-1"));
    assert_eq!(result.contact.metadata["capabilities"][1], json!("rch.announce_slot"));
    assert!(result.was_created);
    assert!(result.announced);

    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_workflow_peer_ready_v2");
    let params = request.params.as_ref().expect("params");
    assert_eq!(params["identity"], json!("peer-ready"));
    assert_eq!(params["display_name"], json!("RCH Relay"));
    assert_eq!(params["trust_level"], json!("trusted"));
    assert_eq!(params["bootstrap"], json!(true));
    assert_eq!(params["announce"], json!(true));
    assert_eq!(params["metadata"]["callsign"], json!("RCH-1"));
    assert_eq!(params["metadata"]["capabilities"][0], json!("rem.direct_chat"));
    assert_eq!(params["extensions"]["source"], json!("rem-rch"));
    server.join().expect("server joined");
}

#[test]
fn peer_directory_merges_contacts_and_presence_over_zmq_sdk_methods() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let server = spawn_response_sequence_zmq_server(
        command_endpoint.clone(),
        vec![
            json!({
                "contact_list": {
                    "contacts": [{
                        "identity": "peer-ready",
                        "display_name": "RCH Relay",
                        "trust_level": "trusted",
                        "bootstrap": true,
                        "updated_ts_ms": 1700000400,
                        "metadata": {
                            "callsign": "RCH-1",
                            "capabilities": ["rem.direct_chat", "rch.announce_slot"]
                        },
                        "extensions": {
                            "saved": true
                        }
                    }],
                    "next_cursor": null
                }
            }),
            json!({
                "presence_list": {
                    "peers": [{
                        "peer_id": "peer-ready",
                        "last_seen_ts_ms": 1700000500,
                        "first_seen_ts_ms": 1700000100,
                        "seen_count": 4,
                        "name": "Relay Announce",
                        "name_source": "announce",
                        "trust_level": "trusted",
                        "bootstrap": true,
                        "extensions": {
                            "capability_flags": ["rem.direct_chat"],
                            "announce_slots": ["rch.broadcast"]
                        }
                    }],
                    "next_cursor": null
                }
            }),
        ],
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let peers = client.peer_directory(Some(10)).expect("peer directory");

    assert_eq!(peers.len(), 1);
    let peer = &peers[0];
    assert_eq!(peer.peer_id, "peer-ready");
    assert_eq!(peer.display_name.as_deref(), Some("RCH Relay"));
    assert_eq!(peer.name_source.as_deref(), Some("contact"));
    assert_eq!(peer.trust_level, Some(crate::TrustLevel::Trusted));
    assert!(peer.bootstrap);
    assert!(peer.online);
    assert_eq!(peer.last_seen_ts_ms, Some(1700000500));
    assert_eq!(peer.first_seen_ts_ms, Some(1700000100));
    assert_eq!(peer.seen_count, 4);
    assert_eq!(peer.metadata["callsign"], json!("RCH-1"));
    assert_eq!(peer.metadata["capabilities"][1], json!("rch.announce_slot"));
    assert_eq!(peer.extensions["saved"], json!(true));
    assert_eq!(peer.extensions["announce_slots"][0], json!("rch.broadcast"));

    let captured = captured.lock().expect("captured requests");
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0].method, "sdk_identity_contact_list_v2");
    assert_eq!(captured[0].params.as_ref().expect("contact params")["limit"], json!(10));
    assert_eq!(captured[1].method, "sdk_identity_presence_list_v2");
    assert_eq!(captured[1].params.as_ref().expect("presence params")["limit"], json!(10));
    server.join().expect("server joined");
}

#[test]
fn peer_directory_limit_still_pages_presence_for_returned_contact() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let server = spawn_response_sequence_zmq_server(
        command_endpoint.clone(),
        vec![
            json!({
                "contact_list": {
                    "contacts": [{
                        "identity": "bob",
                        "display_name": "Bob",
                        "trust_level": "trusted",
                        "bootstrap": true,
                        "updated_ts_ms": 1700000400,
                        "metadata": {},
                        "extensions": {}
                    }],
                    "next_cursor": "contact:1"
                }
            }),
            json!({
                "contact_list": {
                    "contacts": [{
                        "identity": "charlie",
                        "display_name": "Charlie",
                        "trust_level": "untrusted",
                        "bootstrap": false,
                        "updated_ts_ms": 1700000401,
                        "metadata": {},
                        "extensions": {}
                    }],
                    "next_cursor": null
                }
            }),
            json!({
                "presence_list": {
                    "peers": [{
                        "peer_id": "eve",
                        "last_seen_ts_ms": 1700000490,
                        "first_seen_ts_ms": 1700000090,
                        "seen_count": 1,
                        "name": "Eve",
                        "name_source": "announce",
                        "trust_level": "unknown",
                        "bootstrap": false,
                        "extensions": {}
                    }],
                    "next_cursor": "presence:1"
                }
            }),
            json!({
                "presence_list": {
                    "peers": [{
                        "peer_id": "bob",
                        "last_seen_ts_ms": 1700000500,
                        "first_seen_ts_ms": 1700000100,
                        "seen_count": 4,
                        "name": "Bob Relay",
                        "name_source": "announce",
                        "trust_level": "trusted",
                        "bootstrap": true,
                        "extensions": {}
                    }],
                    "next_cursor": null
                }
            }),
        ],
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let peers = client.peer_directory(Some(1)).expect("peer directory");

    assert_eq!(peers.len(), 1);
    let peer = &peers[0];
    assert_eq!(peer.peer_id, "bob");
    assert!(peer.online);
    assert_eq!(peer.last_seen_ts_ms, Some(1700000500));
    assert_eq!(peer.first_seen_ts_ms, Some(1700000100));
    assert_eq!(peer.seen_count, 4);

    let captured = captured.lock().expect("captured requests");
    assert_eq!(captured.len(), 4);
    assert_eq!(captured[0].method, "sdk_identity_contact_list_v2");
    assert_eq!(captured[0].params.as_ref().expect("contact params")["limit"], json!(1));
    assert_eq!(
        captured[1].params.as_ref().expect("contact page 2 params")["cursor"],
        json!("contact:1")
    );
    assert_eq!(captured[2].method, "sdk_identity_presence_list_v2");
    assert_eq!(captured[2].params.as_ref().expect("presence params")["limit"], json!(1));
    assert_eq!(
        captured[3].params.as_ref().expect("presence page 2 params")["cursor"],
        json!("presence:1")
    );
    server.join().expect("server joined");
}

#[test]
fn peer_directory_uses_presence_name_source_when_contact_has_no_name() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let server = spawn_response_sequence_zmq_server(
        command_endpoint.clone(),
        vec![
            json!({
                "contact_list": {
                    "contacts": [{
                        "identity": "peer-ready",
                        "display_name": null,
                        "trust_level": "trusted",
                        "bootstrap": true,
                        "updated_ts_ms": 1700000400,
                        "metadata": {},
                        "extensions": {}
                    }],
                    "next_cursor": null
                }
            }),
            json!({
                "presence_list": {
                    "peers": [{
                        "peer_id": "peer-ready",
                        "last_seen_ts_ms": 1700000500,
                        "first_seen_ts_ms": 1700000100,
                        "seen_count": 4,
                        "name": "Relay Announce",
                        "name_source": "announce",
                        "trust_level": "trusted",
                        "bootstrap": true,
                        "extensions": {}
                    }],
                    "next_cursor": null
                }
            }),
        ],
        Arc::new(Mutex::new(Vec::new())),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let peers = client.peer_directory(Some(10)).expect("peer directory");

    assert_eq!(peers.len(), 1);
    let peer = &peers[0];
    assert_eq!(peer.peer_id, "peer-ready");
    assert_eq!(peer.display_name.as_deref(), Some("Relay Announce"));
    assert_eq!(peer.name_source.as_deref(), Some("announce"));
    server.join().expect("server joined");
}
#[test]
fn peer_directory_since_passes_presence_stale_cutoff_over_zmq_sdk_method() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let server = spawn_response_sequence_zmq_server(
        command_endpoint.clone(),
        vec![
            json!({
                "contact_list": {
                    "contacts": [{
                        "identity": "stale-saved",
                        "display_name": "Saved Peer",
                        "trust_level": "trusted",
                        "bootstrap": true,
                        "updated_ts_ms": 1700000400,
                        "metadata": {},
                        "extensions": {}
                    }],
                    "next_cursor": null
                }
            }),
            json!({
                "presence_list": {
                    "peers": [{
                        "peer_id": "fresh-peer",
                        "last_seen_ts_ms": 1700000800,
                        "first_seen_ts_ms": 1700000700,
                        "seen_count": 2,
                        "name": "Fresh Peer",
                        "name_source": "announce",
                        "trust_level": "unknown",
                        "bootstrap": false,
                        "extensions": {}
                    }],
                    "next_cursor": null
                }
            }),
        ],
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let peers = client.peer_directory_since(Some(10), Some(1_700_000_500)).expect("peer directory");

    assert_eq!(peers.len(), 2);
    let stale_saved = peers.iter().find(|peer| peer.peer_id == "stale-saved").expect("saved peer");
    assert!(!stale_saved.online);
    let fresh = peers.iter().find(|peer| peer.peer_id == "fresh-peer").expect("fresh peer");
    assert!(fresh.online);
    assert_eq!(fresh.last_seen_ts_ms, Some(1_700_000_800));

    let captured = captured.lock().expect("captured requests");
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[1].method, "sdk_identity_presence_list_v2");
    let params = captured[1].params.as_ref().expect("presence params");
    assert_eq!(params["limit"], json!(10));
    assert_eq!(params["min_last_seen_ts_ms"], json!(1_700_000_500));
    server.join().expect("server joined");
}
#[test]
fn peer_lifecycle_methods_use_zmq_sdk_methods_and_preserve_metadata() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let server = spawn_response_sequence_zmq_server(
        command_endpoint.clone(),
        vec![
            json!({
                "peer": {
                    "identity": "peer-ready",
                    "state": "connected",
                    "display_name": "RCH Relay",
                    "connected": true,
                    "updated_ts_ms": 1700000600,
                    "metadata": {
                        "callsign": "RCH-1",
                        "capability_flags": ["rem.direct_chat"],
                        "announce_slots": ["rch.broadcast"]
                    },
                    "extensions": {
                        "source": "connect"
                    }
                }
            }),
            json!({
                "peer": {
                    "identity": "peer-ready",
                    "state": "disconnected",
                    "display_name": "RCH Relay",
                    "connected": false,
                    "updated_ts_ms": 1700000610,
                    "metadata": {
                        "callsign": "RCH-1"
                    },
                    "extensions": {
                        "source": "disconnect"
                    }
                }
            }),
            json!({
                "peer": {
                    "identity": "peer-ready",
                    "state": "reconnected",
                    "display_name": "RCH Relay",
                    "connected": true,
                    "updated_ts_ms": 1700000620,
                    "metadata": {
                        "callsign": "RCH-1"
                    },
                    "extensions": {
                        "source": "reconnect"
                    }
                }
            }),
        ],
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let request = crate::PeerConnectionRequest {
        identity: crate::IdentityRef("peer-ready".to_owned()),
        display_name: Some("RCH Relay".to_owned()),
        correlation_id: Some("peer-life-corr".to_owned()),
        metadata: BTreeMap::from([
            ("callsign".to_owned(), json!("RCH-1")),
            ("capability_flags".to_owned(), json!(["rem.direct_chat"])),
            ("announce_slots".to_owned(), json!(["rch.broadcast"])),
        ]),
        extensions: BTreeMap::from([("source".to_owned(), json!("rem-rch"))]),
    };

    let connected = client.peer_connect(request.clone()).expect("peer connect");
    let disconnected = client.peer_disconnect(request.clone()).expect("peer disconnect");
    let reconnected = client.peer_reconnect(request).expect("peer reconnect");

    assert_eq!(connected.identity.0, "peer-ready");
    assert_eq!(connected.state, crate::PeerConnectionState::Connected);
    assert!(connected.connected);
    assert_eq!(connected.metadata["announce_slots"][0], json!("rch.broadcast"));
    assert_eq!(disconnected.state, crate::PeerConnectionState::Disconnected);
    assert!(!disconnected.connected);
    assert_eq!(reconnected.state, crate::PeerConnectionState::Reconnected);
    assert!(reconnected.connected);

    let captured = captured.lock().expect("captured requests");
    assert_eq!(captured.len(), 3);
    assert_eq!(captured[0].method, "sdk_peer_connect_v2");
    assert_eq!(captured[1].method, "sdk_peer_disconnect_v2");
    assert_eq!(captured[2].method, "sdk_peer_reconnect_v2");
    for request in captured.iter() {
        let params = request.params.as_ref().expect("params");
        assert_eq!(params["identity"], json!("peer-ready"));
        assert_eq!(params["display_name"], json!("RCH Relay"));
        assert_eq!(params["correlation_id"], json!("peer-life-corr"));
        assert_eq!(params["metadata"]["callsign"], json!("RCH-1"));
        assert_eq!(params["metadata"]["capability_flags"][0], json!("rem.direct_chat"));
        assert_eq!(params["metadata"]["announce_slots"][0], json!("rch.broadcast"));
        assert_eq!(params["extensions"]["source"], json!("rem-rch"));
    }
    server.join().expect("server joined");
}
