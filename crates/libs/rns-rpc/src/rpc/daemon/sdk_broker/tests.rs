use super::*;
#[derive(Default)]
struct IdentityBridge;
impl ServiceIdentityBridge for IdentityBridge {
    fn list_service_identities(&self) -> std::io::Result<Vec<ServiceIdentityRecord>> {
        Ok(vec![])
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
