use super::*;
use std::time::Duration;

fn stalled_config(single_endpoint: bool) -> (std::net::TcpListener, ZmqPipelineBackendConfig) {
    // TCP accepts the connection but never completes the ZMTP handshake.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("stalled peer");
    let endpoint = format!("tcp://{}", listener.local_addr().expect("peer address"));
    let mut config = if single_endpoint {
        ZmqPipelineBackendConfig::local(endpoint)
    } else {
        ZmqPipelineBackendConfig::local_tcp(endpoint, "tcp://127.0.0.1:0")
    };
    config.request_timeout = Duration::from_millis(150);
    (listener, config)
}

#[tokio::test]
async fn async_connection_and_handshake_obey_request_deadline() {
    for single_endpoint in [false, true] {
        let (_peer, config) = stalled_config(single_endpoint);
        let client = ZmqPipelineBackendClient::new_async_only(config).expect("client");
        let error = tokio::time::timeout(
            Duration::from_millis(600),
            client.call_rpc_async("sdk_poll_events_v2", Some(json!({"cursor": null, "max": 1}))),
        )
        .await
        .expect("connection must obey SDK deadline")
        .expect_err("stalled peer times out");
        assert_eq!(error.machine_code, "SDK_TRANSPORT_ZMQ_TIMEOUT");
    }
}

#[test]
fn sync_connection_and_handshake_obey_request_deadline() {
    for single_endpoint in [false, true] {
        let (peer, config) = stalled_config(single_endpoint);
        let (tx, rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _peer = peer;
            let client = ZmqPipelineBackendClient::new(config).expect("client");
            let result =
                client.call_rpc("sdk_poll_events_v2", Some(json!({"cursor": null, "max": 1})));
            tx.send(result).expect("deadline result");
        });
        let error = rx
            .recv_timeout(Duration::from_millis(600))
            .expect("connection must obey SDK deadline")
            .expect_err("stalled peer times out");
        assert_eq!(error.machine_code, "SDK_TRANSPORT_ZMQ_TIMEOUT");
        worker.join().expect("bounded worker");
    }
}

#[tokio::test]
async fn pipeline_lock_wait_is_bounded_without_resetting_another_owner() {
    let (_peer, config) = stalled_config(false);
    let client = ZmqPipelineBackendClient::new_async_only(config).expect("client");
    let owner = client.transport.lock().await;
    let error = tokio::time::timeout(
        Duration::from_millis(600),
        client.call_rpc_async("sdk_poll_events_v2", Some(json!({"max": 1}))),
    )
    .await
    .expect("waiting caller has its own deadline")
    .expect_err("busy owner");
    assert_eq!(error.machine_code, "SDK_TRANSPORT_ZMQ_TIMEOUT");
    assert!(owner.is_none());
}

#[tokio::test]
async fn pipeline_timeout_rebinds_and_filters_wrong_session_and_request_responses() {
    let mut commands = PullSocket::new();
    let endpoint = commands.bind("tcp://127.0.0.1:0").await.expect("commands").to_string();
    let mut config = ZmqPipelineBackendConfig::local_tcp(endpoint, "tcp://127.0.0.1:0");
    config.request_timeout = Duration::from_millis(500);
    let client = ZmqPipelineBackendClient::new_async_only(config).expect("client");
    let server = tokio::spawn(async move {
        let first = recv_request_envelope(&mut commands).await.expect("first request");
        // No reply: the client must release this transport and bind a fresh endpoint.
        let second = recv_request_envelope(&mut commands).await.expect("retry request");
        assert_ne!(first.response_endpoint, second.response_endpoint);
        let response_endpoint = second.response_endpoint.expect("response endpoint");
        let mut responses = PushSocket::new();
        responses.connect(&response_endpoint).await.expect("retry socket");
        tokio::time::sleep(Duration::from_millis(50)).await;
        let payload = rns_rpc::rpc::codec::encode_frame(&RpcResponse {
            id: second.request_id,
            result: Some(json!({"events": [], "recovered": true})),
            error: None,
        })
        .expect("rpc frame");
        for (session, request_id) in [
            ("wrong-session".to_string(), second.request_id),
            (second.session_id.clone(), first.request_id),
            (second.session_id, second.request_id),
        ] {
            let envelope = ZmqRpcEnvelope::response(session, request_id, payload.clone());
            responses
                .send(ZmqMessage::from(zmq::encode_envelope(&envelope).expect("encode")))
                .await
                .expect("response");
        }
    });
    let first = client
        .call_rpc_async("sdk_poll_events_v2", Some(json!({"max": 1})))
        .await
        .expect_err("first response absent");
    assert_eq!(first.machine_code, "SDK_TRANSPORT_ZMQ_TIMEOUT");
    assert!(client.transport.lock().await.is_none());
    let recovered = client
        .call_rpc_async("sdk_poll_events_v2", Some(json!({"max": 1})))
        .await
        .expect("retry ignores unrelated responses");
    assert_eq!(recovered["recovered"], true);
    server.await.expect("server");
}
