use super::*;
use std::time::Duration;

#[tokio::test]
async fn stalled_response_peer_does_not_block_healthy_event_poll() {
    let stalled = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("stalled peer");
    let stalled_endpoint = format!("tcp://{}", stalled.local_addr().expect("stalled address"));
    let mut healthy = PullSocket::new();
    let endpoint = healthy.bind("tcp://127.0.0.1:0").await.expect("healthy peer").to_string();
    let daemon = RpcDaemon::test_instance();
    let (tx, rx) = mpsc::channel(8);
    let (_shutdown_tx, shutdown_rx) = watch::channel(false);
    let metrics = Arc::new(ZmqPipelineMetrics::default());
    let writer = tokio::spawn(run_zmq_response_writer(rx, shutdown_rx, Arc::clone(&metrics)));
    tx.send(ZmqOutboundResponse {
        queue_stage: None,
        endpoint: stalled_endpoint,
        envelope: ZmqRpcEnvelope::response("departed-client".to_string(), 1, vec![]),
    })
    .await
    .expect("stalled response");
    let (_connection, _) = tokio::time::timeout(Duration::from_secs(1), stalled.accept())
        .await
        .expect("writer connected")
        .expect("stalled connection");
    let request = ZmqRpcEnvelope::request(
        "rch-event-client",
        2,
        endpoint,
        rns_rpc::e2e_harness::build_rpc_frame(
            2,
            "sdk_poll_events_v2",
            Some(serde_json::json!({"cursor": null, "max": 8})),
        )
        .expect("poll frame"),
        None,
    );
    let response = handle_zmq_command_message(
        &daemon,
        ZmqMessage::from(zmq::encode_envelope(&request).expect("request envelope")),
        false,
    )
    .expect("event poll");
    tx.send(response).await.expect("healthy response");
    let message = tokio::time::timeout(Duration::from_millis(600), healthy.recv())
        .await
        .expect("healthy poll must bypass stalled peer")
        .expect("poll response");
    let envelope =
        zmq::decode_envelope(&Vec::<u8>::try_from(message).expect("bytes")).expect("envelope");
    assert_eq!(envelope.session_id, "rch-event-client");
    assert_eq!(envelope.request_id, 2);
    let rpc = rns_rpc::e2e_harness::parse_rpc_frame(&envelope.payload).expect("rpc");
    assert!(rpc.error.is_none(), "event poll succeeds: {:?}", rpc.error);
    assert!(rpc.result.expect("poll result")["events"].is_array());
    drop(tx);
    tokio::time::timeout(Duration::from_secs(2), writer)
        .await
        .expect("writer drains within deadline")
        .expect("writer task");
    let snapshot = metrics.snapshot();
    assert_eq!(snapshot["delivery"]["succeeded"], 1);
    assert_eq!(snapshot["delivery"]["timed_out"], 1);
    assert_eq!(snapshot["delivery"]["active"], 0);
    assert_eq!(snapshot["delivery"]["owned_wire_bytes"], 0);
}

#[tokio::test]
async fn response_writer_cancels_active_stalled_peers_on_shutdown() {
    let stalled = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("stalled peer");
    let endpoint = format!("tcp://{}", stalled.local_addr().expect("address"));
    let (tx, rx) = mpsc::channel(8);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let metrics = Arc::new(ZmqPipelineMetrics::default());
    let writer = tokio::spawn(run_zmq_response_writer(rx, shutdown_rx, Arc::clone(&metrics)));
    tx.send(ZmqOutboundResponse {
        queue_stage: None,
        endpoint,
        envelope: ZmqRpcEnvelope::response("shutdown-client".to_string(), 1, vec![]),
    })
    .await
    .expect("response");
    let (_connection, _) = tokio::time::timeout(Duration::from_secs(1), stalled.accept())
        .await
        .expect("active writer")
        .expect("connection");
    shutdown_tx.send(true).expect("shutdown");
    tokio::time::timeout(Duration::from_millis(300), writer)
        .await
        .expect("shutdown cancels stalled delivery")
        .expect("writer stopped");
    assert!(tx.is_closed());
    let snapshot = metrics.snapshot();
    assert_eq!(snapshot["delivery"]["cancelled"], 1);
    assert_eq!(snapshot["delivery"]["active"], 0);
    assert_eq!(snapshot["delivery"]["owned_wire_bytes"], 0);
}
