use super::*;
use std::time::Duration;
use zeromq::RouterSocket;

fn check_context(error: &SdkError, client: &ZmqPipelineBackendClient, stage: &str, sent: bool) {
    let detail = &error.details["sdk_zmq_exchange"];
    assert_eq!(detail["session_id"], client.session_id());
    assert_eq!(detail["request_id"], 1);
    assert_eq!(detail["method"], "sdk_poll_events_v2");
    assert_eq!(detail["stage"], stage);
    assert_eq!(detail["send_completed"], sent);
}

#[tokio::test]
async fn dealer_lock_timeout_preserves_the_current_owner() {
    let mut config = ZmqPipelineBackendConfig::local("tcp://127.0.0.1:1");
    config.request_timeout = Duration::from_millis(100);
    let client = ZmqPipelineBackendClient::new_async_only(config).expect("client");
    let owner = client.dealer_pool[1].lock().await;
    let error = tokio::time::timeout(
        Duration::from_millis(600),
        client.call_rpc_async("sdk_poll_events_v2", None),
    )
    .await
    .expect("bounded lock wait")
    .expect_err("held dealer slot");
    check_context(&error, &client, "transport lock", false);
    assert!(owner.is_none());
    assert_eq!(error.machine_code, "SDK_TRANSPORT_ZMQ_TIMEOUT");
}

#[tokio::test]
async fn ignored_pipeline_replies_are_counted_without_retaining_peer_identifiers() {
    let mut commands = PullSocket::new();
    let endpoint = commands.bind("tcp://127.0.0.1:0").await.expect("commands").to_string();
    let mut config = ZmqPipelineBackendConfig::local_tcp(endpoint, "tcp://127.0.0.1:0");
    config.request_timeout = Duration::from_millis(400);
    let client = ZmqPipelineBackendClient::new_async_only(config).expect("client");
    let server = tokio::spawn(async move {
        let request = recv_request_envelope(&mut commands).await.expect("request");
        let mut responses = PushSocket::new();
        responses.connect(&request.response_endpoint.expect("endpoint")).await.expect("connect");
        tokio::time::sleep(Duration::from_millis(50)).await;
        for (session, id) in [
            ("unrelated-peer-session".to_owned(), request.request_id),
            (request.session_id.clone(), request.request_id + 1),
        ] {
            let response = ZmqRpcEnvelope::response(session, id, Vec::new());
            responses
                .send(ZmqMessage::from(zmq::encode_envelope(&response).expect("encode")))
                .await
                .expect("unrelated response");
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    });
    let error =
        client.call_rpc_async("sdk_poll_events_v2", None).await.expect_err("no matching reply");
    check_context(&error, &client, "correlated response", true);
    assert_eq!(error.machine_code, "SDK_TRANSPORT_ZMQ_TIMEOUT");
    assert_eq!(error.details["sdk_zmq_exchange"]["ignored_replies"], 2);
    assert!(!error.message.contains("unrelated-peer-session"));
    assert!(!serde_json::to_string(&error.details)
        .expect("serialize details")
        .contains("unrelated-peer-session"));
    assert!(client.transport.lock().await.is_none());
    server.await.expect("server");
}

#[tokio::test]
async fn pipeline_decode_and_mapped_remote_errors_keep_local_stage_and_original_semantics() {
    for case in ["envelope", "rpc", "remote"] {
        let mut commands = PullSocket::new();
        let endpoint = commands.bind("tcp://127.0.0.1:0").await.expect("commands").to_string();
        let mut config = ZmqPipelineBackendConfig::local_tcp(endpoint, "tcp://127.0.0.1:0");
        config.request_timeout = Duration::from_secs(2);
        let client = ZmqPipelineBackendClient::new_async_only(config).expect("client");
        let mut remote = rns_rpc::RpcError::new("SDK_POLICY_TEST", "remote rejection");
        remote.category = Some("Policy".to_owned());
        remote.retryable = Some(true); // Existing mapper drops this; diagnostics must not change it.
        let expected = super::super::support::map_rpc_error(remote.clone());
        let server = tokio::spawn(async move {
            let request = recv_request_envelope(&mut commands).await.expect("request");
            let mut responses = PushSocket::new();
            responses
                .connect(&request.response_endpoint.expect("endpoint"))
                .await
                .expect("connect");
            tokio::time::sleep(Duration::from_millis(50)).await;
            let bytes = if case == "envelope" {
                vec![0xff]
            } else {
                let payload = if case == "rpc" {
                    vec![0xff]
                } else {
                    rns_rpc::rpc::codec::encode_frame(&RpcResponse {
                        id: request.request_id,
                        result: None,
                        error: Some(remote),
                    })
                    .expect("rpc encode")
                };
                zmq::encode_envelope(&ZmqRpcEnvelope::response(
                    request.session_id,
                    request.request_id,
                    payload,
                ))
                .expect("envelope encode")
            };
            responses.send(ZmqMessage::from(bytes)).await.expect("response");
            tokio::time::sleep(Duration::from_millis(50)).await;
        });
        let error =
            client.call_rpc_async("sdk_poll_events_v2", None).await.expect_err("invalid response");
        let stage = match case {
            "envelope" => "response decode",
            "rpc" => "rpc response decode",
            _ => "rpc response",
        };
        check_context(&error, &client, stage, true);
        if case == "remote" {
            assert_eq!(error.machine_code, expected.machine_code);
            assert_eq!(error.category, expected.category);
            assert_eq!(error.retryable, expected.retryable);
            assert_eq!(error.is_user_actionable, expected.is_user_actionable);
            assert_eq!(error.cause_code, expected.cause_code);
            assert_eq!(error.extensions, expected.extensions);
            assert!(error.message.starts_with(&expected.message));
        } else {
            assert_eq!(error.category, ErrorCategory::Transport);
            // Envelope decode fails inside the exchange and resets it; RPC
            // decode happens after a successful exchange and keeps it, as before.
            assert_eq!(client.transport.lock().await.is_none(), case == "envelope");
        }
        server.await.expect("server");
    }
}

#[tokio::test]
async fn dealer_correlation_mismatch_reports_local_context_without_ignoring_reply() {
    let mut router = RouterSocket::new();
    let endpoint = router.bind("tcp://127.0.0.1:0").await.expect("router").to_string();
    let mut config = ZmqPipelineBackendConfig::local(endpoint);
    config.request_timeout = Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new_async_only(config).expect("client");
    let server = tokio::spawn(async move {
        let request = tokio::time::timeout(Duration::from_secs(1), router.recv())
            .await
            .expect("receive deadline")
            .expect("request");
        let identity = request.get(0).expect("routing identity").clone();
        let bytes = request.get(1).expect("envelope");
        let request = zmq::decode_envelope(bytes).expect("decode");
        let response = ZmqRpcEnvelope::response(
            "unrelated-peer-session".to_owned(),
            request.request_id,
            Vec::new(),
        );
        let mut message = ZmqMessage::from(zmq::encode_envelope(&response).expect("encode"));
        message.push_front(identity);
        router.send(message).await.expect("reply");
        tokio::time::sleep(Duration::from_millis(50)).await;
    });
    let error = client.call_rpc_async("sdk_poll_events_v2", None).await.expect_err("mismatch");
    check_context(&error, &client, "response correlation", true);
    assert_eq!(error.machine_code, "SDK_TRANSPORT_ZMQ_CORRELATION_MISMATCH");
    assert_eq!(error.details["sdk_zmq_exchange"]["ignored_replies"], 0);
    assert!(!error.message.contains("unrelated-peer-session"));
    assert!(client.dealer_pool[1].lock().await.is_none());
    server.await.expect("server");
}
