use super::*;
use std::time::Duration;

#[tokio::test]
async fn lost_announce_reply_rotates_response_generation_without_identity_reimport_or_retry() {
    let mut commands = PullSocket::new();
    let endpoint = commands.bind("tcp://127.0.0.1:0").await.expect("commands").to_string();
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve");
    let response_endpoint = format!("tcp://{}", reservation.local_addr().expect("address"));
    drop(reservation);
    let mut config = ZmqPipelineBackendConfig::local_tcp(endpoint, response_endpoint);
    config.request_timeout = Duration::from_millis(500);
    let client = ZmqPipelineBackendClient::new_async_only(config).expect("client");
    let session = client.session_id().to_owned();
    let server = tokio::spawn(async move {
        let mut generations = Vec::new();
        let mut sockets = Vec::new();
        let mut methods = Vec::new();
        let mut response_endpoints = Vec::new();
        for index in 0..4 {
            let request =
                tokio::time::timeout(Duration::from_secs(2), recv_request_envelope(&mut commands))
                    .await
                    .expect("request deadline")
                    .expect("request");
            assert_eq!(request.session_id, session);
            let rpc: RpcRequest = rns_rpc::rpc::codec::decode_frame(&request.payload).expect("rpc");
            methods.push(rpc.method);
            generations.push(request.response_connection_id.clone().expect("generation"));
            response_endpoints.push(request.response_endpoint.clone().expect("endpoint"));
            if index == 1 {
                continue;
            } // Announce executes once, response is lost.
            if index == 0 || index == 2 {
                let mut socket = PushSocket::new();
                socket.connect(&response_endpoints[index]).await.expect("connect");
                sockets.push(socket);
            }
            let response = ZmqRpcEnvelope::response(
                request.session_id,
                request.request_id,
                rns_rpc::rpc::codec::encode_frame(&RpcResponse {
                    id: request.request_id,
                    result: Some(json!({"ok":true})),
                    error: None,
                })
                .expect("frame"),
            );
            sockets
                .last_mut()
                .expect("socket")
                .send(ZmqMessage::from(zmq::encode_envelope(&response).expect("envelope")))
                .await
                .expect("reply");
        }
        commands.backend().shutdown();
        (generations, methods, response_endpoints)
    });
    client.call_rpc_async("sdk_poll_events_v2", None).await.expect("first poll");
    let error =
        client.call_rpc_async("sdk_identity_announce_now_v2", None).await.expect_err("lost reply");
    assert_eq!(error.details["sdk_zmq_exchange"]["execution_certainty"], "Unknown");
    assert!(client.transport.lock().await.is_none());
    client.call_rpc_async("sdk_poll_events_v2", None).await.expect("recovered poll");
    client.call_rpc_async("sdk_poll_events_v2", None).await.expect("reuse recovered connection");
    let (generations, methods, endpoints) = server.await.expect("server");
    assert_eq!(generations[0], generations[1]);
    assert_ne!(generations[1], generations[2]);
    assert_eq!(generations[2], generations[3]);
    assert!(endpoints.iter().all(|endpoint| endpoint == &endpoints[0]));
    assert_eq!(
        methods,
        [
            "sdk_poll_events_v2",
            "sdk_identity_announce_now_v2",
            "sdk_poll_events_v2",
            "sdk_poll_events_v2"
        ]
    );
}
