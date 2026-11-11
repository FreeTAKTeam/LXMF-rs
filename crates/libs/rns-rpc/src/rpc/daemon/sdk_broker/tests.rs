use super::*;
#[derive(Default)]
struct IdentityBridge;
impl ServiceIdentityBridge for IdentityBridge {
    fn list_service_identities(&self) -> std::io::Result<Vec<ServiceIdentityRecord>> {
        self.import_service_identity(&[], ServiceIdentitySpec::default())
            .map(|identity| vec![identity])
    }
    fn create_service_identity(
        &self,
        spec: ServiceIdentitySpec,
    ) -> std::io::Result<ServiceIdentityRecord> {
        self.import_service_identity(&[], spec)
    }
    fn import_service_identity(
        &self,
        _: &[u8],
        spec: ServiceIdentitySpec,
    ) -> std::io::Result<ServiceIdentityRecord> {
        Ok(ServiceIdentityRecord {
            identity: "service-identity".into(),
            delivery_destination: "00112233445566778899aabbccddeeff".into(),
            public_key: "00".repeat(64),
            display_name: spec.display_name,
            capabilities: spec.capabilities,
            metadata: spec.metadata,
        })
    }
    fn export_service_identity(&self, _: &str) -> std::io::Result<Vec<u8>> {
        Ok(vec![7; 64])
    }
    fn announce_service_identity(
        &self,
        _: &str,
        spec: ServiceIdentitySpec,
    ) -> std::io::Result<ServiceIdentityRecord> {
        self.import_service_identity(&[], spec)
    }
}
struct Bridge;
impl OutboundBridge for Bridge {
    fn deliver(&self, _: &MessageRecord, _: &OutboundDeliveryOptions) -> std::io::Result<()> {
        Ok(())
    }
}
fn request(
    daemon: &RpcDaemon,
    subject: &str,
    sequence: u64,
    method: &str,
    params: JsonValue,
) -> RpcResponse {
    let now = now_seconds_u64();
    let claims = format!(
        "iss=issuer;aud=audience;jti={subject}-{sequence};sub={subject};iat={now};exp={}",
        now + 60
    );
    let signature = RpcDaemon::token_signature("fixture-secret", &claims);
    let headers = vec![("authorization".into(), format!("Bearer {claims};sig={signature}"))];
    let principal = daemon.authorize_http_principal(&headers, Some("10.0.0.1"), None).unwrap();
    assert_eq!(principal, subject);
    // Reuse the untrusted route/session string across two principals deliberately.
    let frame = codec::encode_frame(&RpcRequest {
        id: sequence,
        method: method.into(),
        params: Some(params),
    })
    .unwrap();
    let response =
        daemon.handle_framed_request_for_zmq_session("same-route", &principal, &frame).unwrap();
    codec::decode_frame(&response).unwrap()
}
#[test]
fn authenticated_principals_cannot_share_consumer_operation_or_mutation_authority() {
    let dir = tempfile::tempdir().unwrap();
    let store = MessagesStore::open(&dir.path().join("auth.db")).unwrap();
    store.enable_durable_broker(32 * 1024 * 1024).unwrap();
    let daemon = RpcDaemon::with_store_and_bridge(store, "auth-fixture".into(), Arc::new(Bridge));
    daemon.set_service_identity_bridge(Arc::new(IdentityBridge));
    let config=daemon.handle_rpc(RpcRequest{id:0,method:"sdk_negotiate_v2".into(),params:Some(json!({"supported_contract_versions":[2],"requested_capabilities":[],"config":{"profile":"desktop-full","bind_mode":"remote","auth_mode":"token","rpc_backend":{"token_auth":{"issuer":"issuer","audience":"audience","shared_secret":"fixture-secret","jti_cache_ttl_ms":30000,"clock_skew_ms":0}}}}))}).unwrap();
    assert!(config.error.is_none(), "{:?}", config.error);
    for subject in ["alice", "bob"] {
        assert!(request(
            &daemon,
            subject,
            1,
            "sdk_broker_negotiate_v1",
            json!({"ack_meaning":"stored"})
        )
        .error
        .is_none());
        assert!(request(
            &daemon,
            subject,
            2,
            "sdk_identity_import_v2",
            json!({"bundle_base64":base64::engine::general_purpose::STANDARD.encode(vec![7u8;64])})
        )
        .error
        .is_none());
        assert!(request(
            &daemon,
            subject,
            3,
            "sdk_identity_activate_v2",
            json!({"identity":"service-identity"})
        )
        .error
        .is_none());
    }
    let identity = "service-identity";
    let resume = json!({"identity":identity,"consumer_id":"rch","stored":0,"journal_id":null});
    assert!(request(&daemon, "alice", 4, "sdk_broker_resume_v1", resume.clone()).error.is_none());
    assert_eq!(
        request(&daemon, "bob", 4, "sdk_broker_resume_v1", resume).error.unwrap().code,
        "SDK_SECURITY_CONSUMER_FORBIDDEN"
    );
    let admission = json!({"identity":identity,"operation_id":"same-operation","destination":"11223344556677889900aabbccddeeff","title":"","content":"Test1234","fields":null,"options":{"method":"direct"}});
    let alice =
        request(&daemon, "alice", 5, "sdk_broker_admit_v1", admission.clone()).result.unwrap();
    let bob = request(&daemon, "bob", 5, "sdk_broker_admit_v1", admission).result.unwrap();
    assert_ne!(alice["message_id"], bob["message_id"]);
    let denied = request(
        &daemon,
        "bob",
        6,
        "record_receipt",
        json!({"message_id":alice["message_id"],"status":"delivered"}),
    );
    assert_eq!(denied.error.unwrap().code, "SDK_SECURITY_OPERATION_FORBIDDEN");
    let forged = request(
        &daemon,
        "alice",
        7,
        "record_receipt",
        json!({"message_id":alice["message_id"],"status":"sent: propagated resource",
            "dispatch_complete":"propagation_custody","propagation_custody":"Resource"}),
    );
    assert!(forged.error.is_none(), "{:?}", forged.error);
    daemon.store.broker_recover_dispatch().unwrap();
    let alice_id = alice["message_id"].as_str().unwrap();
    let connection = rusqlite::Connection::open(dir.path().join("auth.db")).unwrap();
    let state: String = connection
        .query_row("SELECT state FROM broker_operations WHERE message_id=?1", [alice_id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_ne!(state, "custody", "RPC text cannot grant native custody authority");
    let committed = daemon
        .record_propagation_custody(
            alice_id,
            crate::broker::PropagationCustody::Resource,
            JsonMap::new(),
        )
        .unwrap();
    assert_eq!(committed["updated"], true, "identical sent text must still retire dispatch intent");
    let state: String = connection
        .query_row("SELECT state FROM broker_operations WHERE message_id=?1", [alice_id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(state, "custody");
    let invalid = request(
        &daemon,
        "alice",
        6,
        "sdk_broker_admit_v1",
        json!({"identity":identity,"operation_id":"paper","destination":"11223344556677889900aabbccddeeff","title":"","content":"Test1234","fields":null,"options":{"method":" Paper "}}),
    );
    assert_eq!(invalid.error.unwrap().code, "SDK_CAPABILITY_UNSUPPORTED");
    daemon.shutdown_outbound_workers();
}

#[test]
fn encoded_projection_limit_counts_json_escapes_and_leaves_envelope_headroom() {
    let value = json!({"announces":["\u{0}".repeat(3*1024*1024)]});
    assert!(validate_projection_size(&value).is_err());
    assert!(validate_projection_size(&json!({"announces":[]})).is_ok());
}

#[test]
fn broker_session_capacity_recovers_across_more_than_4096_abandoned_routes() {
    use super::sessions::{BROKER_SESSION_IDLE_TTL as TTL, MAX_BROKER_SESSIONS as MAX};
    use std::time::Instant;

    let daemon = RpcDaemon::with_store(
        MessagesStore::in_memory().expect("messages"),
        "legacy-identity".into(),
    );
    let start = Instant::now();
    let active = r#"["alice","active-route"]"#;
    let negotiate = |route: &str, now| {
        daemon.admit_broker_session(route.to_owned(), now).map_err(|error| error.code)
    };
    negotiate(active, start).expect("active negotiation");
    // The pre-existing legacy entry consumes a slot, just as it did before expiry.
    let rounds = [(start, start + TTL / 2), (start + TTL, start + TTL + TTL / 4)];
    for (round, (now, touch)) in rounds.into_iter().enumerate() {
        for route in 0..MAX - 2 {
            negotiate(&format!(r#"["alice","round-{round}-route-{route}"]"#), now)
                .expect("admit fresh route");
        }
        assert_eq!(
            negotiate(r#"["alice","overflow"]"#, now).unwrap_err(),
            "SDK_STORAGE_WRITE_BUSY",
            "all registry entries are live"
        );
        let mut request = daemon.begin_broker_session_request(active, touch);
        assert!(matches!(request, BrokerSessionUse::InFlight(_)), "active route survives");
        request.accepted_at(touch);
    }
    let now = start + TTL * 2;
    let pin = daemon.begin_broker_session_request(active, now);
    assert!(matches!(pin, BrokerSessionUse::InFlight(_)), "accepted activity kept the route live");
    negotiate(r#"["alice","new-after-two-full-rounds"]"#, now).expect("retired routes free slots");
    // An in-flight request may still access the grants beyond the idle deadline.
    negotiate(r#"["alice","after-in-flight-deadline"]"#, now + TTL)
        .expect("sweep while active request is still in flight");
    assert!(daemon.sdk_identity_sessions.lock().unwrap().entries.contains_key(active));
    negotiate(active, now + TTL).expect("renegotiation does not replace an in-flight route");
    drop(pin); // rejected requests do not extend the idle deadline
    assert!(matches!(
        daemon.begin_broker_session_request(active, now + TTL),
        BrokerSessionUse::Expired
    ));
    let counts = daemon.broker_session_counts().expect("aggregate lifecycle counts");
    assert_eq!(counts["current"], 1);
    assert_eq!(counts["expired_total"], 2 * (MAX - 2) + 2);
    assert!(daemon
        .sdk_identity_sessions
        .lock()
        .unwrap()
        .entries
        .contains_key(crate::rpc::LEGACY_RPC_SESSION_ID));
}

#[test]
fn session_expiry_recovers_durable_checkpoints_and_operation_idempotency() {
    use super::sessions::BROKER_SESSION_IDLE_TTL as TTL;

    let dir = tempfile::tempdir().expect("persistent test store");
    let store = MessagesStore::open(&dir.path().join("lifecycle.db")).expect("messages");
    store.enable_durable_broker(32 * 1024 * 1024).expect("durable broker");
    let daemon = RpcDaemon::with_store_and_bridge(store, "lifecycle".into(), Arc::new(Bridge));
    daemon.set_service_identity_bridge(Arc::new(IdentityBridge));
    let configuration = daemon.handle_rpc(RpcRequest {
        id: 0,
        method: "sdk_negotiate_v2".into(),
        params: Some(json!({
            "supported_contract_versions": [2], "requested_capabilities": [],
            "config": {
                "profile": "desktop-full", "bind_mode": "remote", "auth_mode": "token",
                "rpc_backend": {"token_auth": {"issuer": "issuer", "audience": "audience",
                    "shared_secret": "fixture-secret", "jti_cache_ttl_ms": 30000, "clock_skew_ms": 0}}
            }
        })),
    }).expect("configure authenticated fixture");
    assert!(configuration.error.is_none(), "{:?}", configuration.error);

    let call =
        |subject, sequence, method, params| request(&daemon, subject, sequence, method, params);
    let alice = |sequence, method, params| {
        let response = call("alice", sequence, method, params);
        assert!(response.error.is_none(), "{method}: {:?}", response.error);
        response.result.expect("durable RPC result")
    };
    let negotiate = json!({"ack_meaning":"stored"});
    let import =
        json!({"bundle_base64":base64::engine::general_purpose::STANDARD.encode(vec![7u8;64])});
    let identity = "service-identity";
    let destination = "00112233445566778899aabbccddeeff";
    let resume =
        json!({"identity":identity, "consumer_id":"lifecycle", "stored":0, "journal_id":null});
    let fetch =
        json!({"identity":identity, "consumer_id":"lifecycle", "max_events":8, "max_bytes":16384});

    alice(1, "sdk_broker_negotiate_v1", negotiate.clone());
    alice(2, "sdk_identity_import_v2", import.clone());
    alice(3, "sdk_identity_activate_v2", json!({"identity":identity}));
    let initial = alice(4, "sdk_broker_resume_v1", resume);
    daemon
        .accept_inbound(MessageRecord {
            id: "restart-message".into(),
            source: "11".repeat(16),
            destination: destination.into(),
            title: String::new(),
            content: "before expiry".into(),
            timestamp: 1,
            direction: "in".into(),
            fields: None,
            receipt_status: None,
        })
        .expect("commit inbound custody");
    let issued = alice(5, "sdk_broker_fetch_v1", fetch.clone());
    assert_eq!(issued["events"][0]["payload"]["message"]["id"], "restart-message");
    let admission = json!({
        "identity":identity, "operation_id":"out-before-expiry", "destination":"22".repeat(16),
        "title":"", "content":"before expiry", "fields":null, "options":{"method":"direct"}
    });
    let admitted = alice(6, "sdk_broker_admit_v1", admission.clone());

    let scope = r#"["alice","same-route"]"#;
    let last_accepted = || {
        daemon.sdk_identity_sessions.lock().unwrap().entries[scope]
            .broker_activity
            .expect("active negotiated route")
            .last_accepted
    };
    let before_invalid = last_accepted();
    let invalid = call("alice", 7, "sdk_broker_fetch_v1", json!({"identity":"forged"}));
    assert_eq!(invalid.error.unwrap().code, "SDK_SECURITY_IDENTITY_FORBIDDEN");
    let wrong_principal = call("bob", 7, "sdk_broker_fetch_v1", fetch.clone());
    assert_eq!(wrong_principal.error.unwrap().code, "SDK_BROKER_NEGOTIATION_REQUIRED");
    assert_eq!(last_accepted(), before_invalid, "rejected requests do not renew");
    assert!(matches!(
        daemon.begin_broker_session_request(scope, before_invalid + TTL),
        BrokerSessionUse::Expired
    ));
    assert!(daemon.sdk_identities.lock().unwrap().contains_key(identity));
    let expired = call("alice", 8, "sdk_broker_fetch_v1", fetch.clone());
    assert_eq!(expired.error.unwrap().code, "SDK_BROKER_NEGOTIATION_REQUIRED");

    alice(9, "sdk_broker_negotiate_v1", negotiate);
    let no_grants = call("alice", 10, "sdk_broker_fetch_v1", fetch.clone());
    assert_eq!(no_grants.error.unwrap().code, "SDK_SECURITY_IDENTITY_FORBIDDEN");
    alice(11, "sdk_identity_import_v2", import);
    alice(12, "sdk_identity_activate_v2", json!({"identity":identity}));
    assert_eq!(alice(13, "sdk_broker_fetch_v1", fetch.clone()), issued);
    let stored = alice(
        14,
        "sdk_broker_ack_stored_v1",
        json!({
            "identity":identity, "consumer_id":issued["consumer_id"], "journal_id":issued["journal_id"],
            "end":issued["end"], "receipt":issued["receipt"]
        }),
    );
    assert_eq!(stored["stored"], issued["end"]);
    let resumed = alice(
        15,
        "sdk_broker_resume_v1",
        json!({
            "identity":identity, "consumer_id":issued["consumer_id"], "journal_id":initial["journal_id"],
            "stored":stored["stored"]
        }),
    );
    assert_eq!(resumed["stored"], stored["stored"]);
    let readmitted = alice(16, "sdk_broker_admit_v1", admission);
    assert_eq!(readmitted["message_id"], admitted["message_id"]);
    assert!(daemon.owns_durable_message(admitted["message_id"].as_str().unwrap()).unwrap());
    // A sweep triggered by another route leaves no tombstone. Ordinary calls
    // then follow unnegotiated behavior, and durable calls still request restoration.
    let expire_at = last_accepted() + TTL;
    daemon.sdk_identity_sessions.lock().unwrap().reap_idle(expire_at);
    let counts = daemon.broker_session_counts().expect("counts after sweep");
    assert_eq!(counts["current"], 0);
    assert_eq!(counts["expired_total"], 2);
    let activate = json!({"identity":identity});
    let ordinary = call("alice", 17, "sdk_identity_activate_v2", activate.clone());
    let unnegotiated = call("bob", 17, "sdk_identity_activate_v2", activate);
    assert_eq!(ordinary.error.unwrap().code, unnegotiated.error.unwrap().code);
    let expired = call("alice", 18, "sdk_broker_fetch_v1", fetch);
    assert_eq!(expired.error.unwrap().code, "SDK_BROKER_NEGOTIATION_REQUIRED");
    daemon.shutdown_outbound_workers();
}
