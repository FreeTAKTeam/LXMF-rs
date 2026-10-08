use super::*;
use std::time::Duration;
use zeromq::{PushSocket, SocketSend};

#[tokio::test]
async fn completed_blocking_output_remains_accounted_until_join_handle_drop() {
    let daemon = Arc::new(RpcDaemon::test_instance());
    let metrics = daemon.zmq_pipeline_metrics();
    let envelope = ZmqRpcEnvelope::request(
        "unawaited",
        7,
        "tcp://127.0.0.1:1",
        rns_rpc::e2e_harness::build_rpc_frame(7, "status", None).expect("frame"),
        None,
    );
    let bytes = zmq::encode_envelope(&envelope).expect("envelope");
    let input_bytes = bytes.len();
    let wait = metrics.enter(ZmqStage::DispatchWait, input_bytes);
    let task = tokio::task::spawn_blocking(move || {
        pipeline::dispatch_with_metrics(&daemon, ZmqMessage::from(bytes), false, input_bytes, wait)
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        while !task.is_finished() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("blocking output complete");
    let snapshot = metrics.snapshot();
    assert_eq!(snapshot["handler"]["active"], 0);
    assert_eq!(snapshot["response_queue"]["active"], 1);
    assert!(snapshot["response_queue"]["owned_wire_bytes"].as_u64().expect("bytes") > 0);
    drop(task);
    assert_eq!(metrics.snapshot()["response_queue"]["active"], 0);
    assert_eq!(metrics.snapshot()["response_queue"]["owned_wire_bytes"], 0);
    assert_eq!(metrics.snapshot()["response_queue"]["cancelled"], 1);
}

#[tokio::test]
async fn real_pipeline_accounts_all_stages_and_rejected_ingress() {
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve port");
    let command_endpoint = format!("tcp://{}", reservation.local_addr().expect("address"));
    drop(reservation);
    let daemon = Arc::new(RpcDaemon::test_instance());
    let metrics = daemon.zmq_pipeline_metrics();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let server = tokio::spawn(run_zmq_rpc_loop_until(
        ZmqRpcLoopConfig {
            command_endpoint: command_endpoint.clone(),
            require_auth_for_remote: true,
        },
        Arc::clone(&daemon),
        shutdown_rx,
    ));
    let mut replies = PullSocket::new();
    let reply_endpoint = replies.bind("tcp://127.0.0.1:0").await.expect("reply bind").to_string();
    let mut commands = PushSocket::new();
    tokio::time::timeout(Duration::from_secs(2), commands.connect(&command_endpoint))
        .await
        .expect("command connect deadline")
        .expect("command connect");
    let request = ZmqRpcEnvelope::request(
        "stage-probe",
        41,
        reply_endpoint,
        rns_rpc::e2e_harness::build_rpc_frame(41, "status", None).expect("frame"),
        None,
    );
    commands
        .send(ZmqMessage::from(zmq::encode_envelope(&request).expect("envelope")))
        .await
        .expect("send request");
    let response = tokio::time::timeout(Duration::from_secs(2), replies.recv())
        .await
        .expect("reply deadline")
        .expect("reply");
    let response = zmq::decode_envelope(&Vec::<u8>::try_from(response).expect("reply bytes"))
        .expect("response envelope");
    assert_eq!(response.session_id, "stage-probe");
    assert_eq!(response.request_id, 41);
    commands.send(ZmqMessage::from(vec![0])).await.expect("send malformed ingress");
    tokio::time::timeout(Duration::from_secs(1), async {
        while metrics.snapshot()["handler"]["failed"] != 1
            || metrics.snapshot()["delivery"]["succeeded"] != 1
        {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("both requests accounted");
    shutdown_tx.send(true).expect("shutdown");
    tokio::time::timeout(Duration::from_secs(1), server)
        .await
        .expect("shutdown deadline")
        .expect("server task")
        .expect("server result");
    let snapshot = metrics.snapshot();
    for stage in ["dispatch_wait", "handler", "response_queue", "delivery"] {
        assert_eq!(snapshot[stage]["active"], 0, "{stage}");
        assert_eq!(snapshot[stage]["owned_wire_bytes"], 0, "{stage}");
        assert!(snapshot[stage]["peak_owned_wire_bytes"].as_u64().expect("bytes") > 0, "{stage}");
    }
    assert_eq!(snapshot["dispatch_wait"]["succeeded"], 2);
    assert_eq!(snapshot["handler"]["succeeded"], 1);
    assert_eq!(snapshot["handler"]["failed"], 1);
    assert_eq!(snapshot["response_queue"]["succeeded"], 1);
    assert_eq!(daemon.resource_usage_snapshot().expect("resources")["zmq_pipeline"], snapshot);
}
