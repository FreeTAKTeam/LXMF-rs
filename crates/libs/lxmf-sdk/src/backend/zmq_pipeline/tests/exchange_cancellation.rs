use super::*;
use std::time::Duration;
use zeromq::RouterSocket;

#[tokio::test]
async fn externally_cancelled_pipeline_drops_socket_before_next_exchange() {
    let mut commands = PullSocket::new();
    let endpoint = commands.bind("tcp://127.0.0.1:0").await.expect("bind").to_string();
    let client = ZmqPipelineBackendClient::new_async_only(ZmqPipelineBackendConfig::local_tcp(
        endpoint,
        "tcp://127.0.0.1:0",
    ))
    .expect("client");
    let first = {
        let call = client.call_rpc_async("sdk_poll_events_v2", None);
        tokio::pin!(call);
        tokio::select! {
            request = recv_request_envelope(&mut commands) => request.expect("first request"),
            result = &mut call => panic!("unexpected reply: {result:?}"),
            () = tokio::time::sleep(Duration::from_secs(2)) => panic!("request deadline"),
        }
        // Dropping the externally owned future simulates an outer timeout.
    };
    assert!(client.transport.lock().await.is_none(), "cancelled socket must not be pooled");
    assert_eq!(first.session_id, client.session_id());
    let responder = async {
        let request = recv_request_envelope(&mut commands).await.expect("second request");
        let mut responses = PushSocket::new();
        responses
            .connect(request.response_endpoint.as_deref().expect("endpoint"))
            .await
            .expect("connect response");
        tokio::time::sleep(Duration::from_millis(50)).await;
        let payload = rns_rpc::rpc::codec::encode_frame(&rns_rpc::RpcResponse {
            id: request.request_id,
            result: Some(json!({"recovered":true})),
            error: None,
        })
        .expect("encode RPC");
        let response =
            ZmqRpcEnvelope::response(request.session_id.clone(), request.request_id, payload);
        responses
            .send(ZmqMessage::from(zmq::encode_envelope(&response).expect("encode envelope")))
            .await
            .expect("send response");
        request
    };
    let (reply, second) =
        tokio::join!(client.call_rpc_async("sdk_poll_events_v2", None), responder);
    assert_eq!(reply.expect("recovered reply"), json!({"recovered":true}));
    assert_eq!(first.session_id, second.session_id);
    assert_ne!(first.response_endpoint, second.response_endpoint);
    assert_ne!(first.request_id, second.request_id);
    assert!(client.transport.lock().await.is_some());
    // The cancelled listener must relinquish its port, not merely disappear from the pool.
    let mut replacement = PullSocket::new();
    tokio::time::timeout(
        Duration::from_secs(1),
        replacement.bind(first.response_endpoint.as_deref().expect("first endpoint")),
    )
    .await
    .expect("listener release bounded")
    .expect("rebind cancelled listener");
    replacement.backend().shutdown();
}

#[tokio::test]
async fn externally_cancelled_dealer_drops_route_but_preserves_session() {
    let mut router = RouterSocket::new();
    let endpoint = router.bind("tcp://127.0.0.1:0").await.expect("bind").to_string();
    let client =
        ZmqPipelineBackendClient::new_async_only(ZmqPipelineBackendConfig::local(endpoint))
            .expect("client");
    let first = {
        let call = client.call_rpc_async("sdk_poll_events_v2", None);
        tokio::pin!(call);
        tokio::select! {
            request = router.recv() => request.expect("first request").into_vec(),
            result = &mut call => panic!("unexpected reply: {result:?}"),
            () = tokio::time::sleep(Duration::from_secs(2)) => panic!("request deadline"),
        }
    };
    assert!(client.dealer_pool[1].lock().await.is_none(), "cancelled route must not be pooled");
    // Exercise exactly the cancelled slot again, rather than another pool member.
    client.next_request_id.store(9, std::sync::atomic::Ordering::Relaxed);
    let responder = async {
        let frames = router.recv().await.expect("second request").into_vec();
        let request = zmq::decode_envelope(&frames[1]).expect("request envelope");
        let payload = rns_rpc::rpc::codec::encode_frame(&rns_rpc::RpcResponse {
            id: request.request_id,
            result: Some(json!({"recovered":true})),
            error: None,
        })
        .expect("encode RPC");
        let response =
            ZmqRpcEnvelope::response(request.session_id.clone(), request.request_id, payload);
        let mut message =
            ZmqMessage::from(zmq::encode_envelope(&response).expect("encode envelope"));
        message.push_front(frames[0].clone());
        router.send(message).await.expect("reply");
        frames
    };
    let (reply, second) =
        tokio::join!(client.call_rpc_async("sdk_poll_events_v2", None), responder);
    assert_eq!(reply.expect("recovered reply"), json!({"recovered":true}));
    assert_ne!(first[0], second[0]);
    let first = zmq::decode_envelope(&first[1]).expect("first envelope");
    let second = zmq::decode_envelope(&second[1]).expect("second envelope");
    assert_eq!(first.session_id, second.session_id);
    assert_eq!(second.request_id, 9);
    assert!(client.dealer_pool[1].lock().await.is_some());
}

#[tokio::test]
async fn cancelling_connection_handshake_releases_the_peer_stream() {
    use tokio::io::AsyncReadExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind raw peer");
    let endpoint = format!("tcp://{}", listener.local_addr().expect("address"));
    let client =
        ZmqPipelineBackendClient::new_async_only(ZmqPipelineBackendConfig::local(endpoint))
            .expect("client");
    let mut stream = {
        let call = client.call_rpc_async("sdk_status_v2", None);
        tokio::pin!(call);
        tokio::select! {
            peer=listener.accept()=>peer.expect("accept").0,
            result=&mut call=>panic!("unexpected completion: {result:?}"),
            ()=tokio::time::sleep(Duration::from_secs(1))=>panic!("connection not started"),
        }
    };
    let mut greeting = Vec::new();
    tokio::time::timeout(Duration::from_secs(1), stream.read_to_end(&mut greeting))
        .await
        .expect("cancelled peer reaches EOF")
        .expect("read peer");
    assert!(client.dealer_pool[1].lock().await.is_none());
}

#[tokio::test]
async fn replay_safe_retry_uses_fresh_ids_and_one_budget() {
    let mut router = RouterSocket::new();
    let endpoint = router.bind("tcp://127.0.0.1:0").await.expect("bind").to_string();
    let mut config = ZmqPipelineBackendConfig::local(endpoint);
    config.request_timeout = Duration::from_millis(400);
    let client = ZmqPipelineBackendClient::new_async_only(config).expect("client");
    let peer = async {
        let first = router.recv().await.expect("first").into_vec();
        let first = zmq::decode_envelope(&first[1]).expect("first envelope");
        let second = router.recv().await.expect("retry").into_vec();
        let request = zmq::decode_envelope(&second[1]).expect("retry envelope");
        assert_eq!(first.session_id, request.session_id);
        assert_ne!(first.request_id, request.request_id);
        let payload = rns_rpc::rpc::codec::encode_frame(&rns_rpc::RpcResponse {
            id: request.request_id,
            result: Some(json!({"ok":true})),
            error: None,
        })
        .expect("RPC");
        let response = ZmqRpcEnvelope::response(request.session_id, request.request_id, payload);
        let mut message = ZmqMessage::from(zmq::encode_envelope(&response).expect("envelope"));
        message.push_front(second[0].clone());
        router.send(message).await.expect("send");
    };
    let started = tokio::time::Instant::now();
    let (result, ()) = tokio::join!(client.call_rpc_replay_safe("sdk_broker_fetch_v1", None), peer);
    assert_eq!(result.expect("retry"), json!({"ok":true}));
    assert!(started.elapsed() < Duration::from_millis(500));
}
