use super::*;
use rns_rpc::e2e_harness::{build_rpc_frame, parse_rpc_frame};
use rns_rpc::rpc::zmq::ZmqRpcEnvelope;
use rns_rpc::{InterfaceMutationBridge, InterfaceRecord};
use zeromq::{DealerSocket, PullSocket, PushSocket};

struct AsyncManagedBridge {
    requests: tokio::sync::mpsc::UnboundedSender<
        std::sync::mpsc::SyncSender<Result<serde_json::Value, io::Error>>,
    >,
}

impl InterfaceMutationBridge for AsyncManagedBridge {
    fn apply_interfaces(
        &self,
        interfaces: Vec<InterfaceRecord>,
    ) -> Result<Vec<InterfaceRecord>, io::Error> {
        Ok(interfaces)
    }

    fn manage_named_interface(
        &self,
        _operation: &str,
        _name: &str,
    ) -> Result<serde_json::Value, io::Error> {
        let (reply, receiver) = std::sync::mpsc::sync_channel(1);
        self.requests.send(reply).map_err(io::Error::other)?;
        receiver.recv_timeout(std::time::Duration::from_secs(2)).map_err(io::Error::other)?
    }
}

fn daemon_with_async_management() -> (Arc<RpcDaemon>, tokio::task::JoinHandle<()>) {
    let daemon = Arc::new(RpcDaemon::test_instance());
    let (requests, mut worker) = tokio::sync::mpsc::unbounded_channel();
    daemon.set_interface_mutation_bridge(Arc::new(AsyncManagedBridge { requests }));
    let worker = tokio::spawn(async move {
        let reply = worker.recv().await.expect("management request");
        reply
            .send(Ok(serde_json::json!({ "complete": true, "state": "active" })))
            .expect("management reply");
    });
    (daemon, worker)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn canonical_router_serves_concurrent_sdk_requests() {
    let reserved = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve zmq port");
    let port = reserved.local_addr().expect("reserved address").port();
    drop(reserved);
    let endpoint = format!("tcp://localhost:{port}");
    let daemon = Arc::new(RpcDaemon::test_instance());
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let server = tokio::spawn(run_zmq_router_loop_until(
        endpoint.clone(),
        true,
        Arc::clone(&daemon),
        shutdown_rx,
    ));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let (first, second) = tokio::join!(snapshot(&endpoint, 1), snapshot(&endpoint, 2));

    assert!(first.is_ok(), "first concurrent request failed: {first:?}");
    assert!(second.is_ok(), "second concurrent request failed: {second:?}");
    shutdown_tx.send(true).expect("request router shutdown");
    server.await.expect("router task join").expect("router shutdown");
}

#[tokio::test(flavor = "current_thread")]
async fn named_management_does_not_block_router_reactor() {
    let reserved = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve zmq port");
    let port = reserved.local_addr().expect("reserved address").port();
    drop(reserved);
    let endpoint = format!("tcp://127.0.0.1:{port}");
    let (daemon, worker) = daemon_with_async_management();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let server = tokio::spawn(run_zmq_router_loop_until(
        endpoint.clone(),
        true,
        Arc::clone(&daemon),
        shutdown_rx,
    ));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mut client = DealerSocket::new();
    client.connect(&endpoint).await.expect("connect dealer");
    let payload = build_rpc_frame(
        77,
        "manage_interface",
        Some(serde_json::json!({ "operation": "attach", "name": "uplink" })),
    )
    .expect("management frame");
    let mut envelope = ZmqRpcEnvelope::request("management-test", 77, "", payload, None);
    envelope.response_endpoint = None;
    client
        .send(ZmqMessage::from(zmq::encode_envelope(&envelope).expect("encode envelope")))
        .await
        .expect("send management request");
    let message = client.recv().await.expect("management response");
    let bytes = Vec::<u8>::try_from(message).expect("response bytes");
    let response = zmq::decode_envelope(&bytes).expect("decode response");
    let rpc = parse_rpc_frame(&response.payload).expect("rpc response");
    assert!(rpc.error.is_none(), "management failed: {:?}", rpc.error);
    assert_eq!(rpc.result.unwrap_or_default()["complete"], true);

    worker.await.expect("worker join");
    shutdown_tx.send(true).expect("request router shutdown");
    server.await.expect("router task join").expect("router shutdown");
}

#[tokio::test(flavor = "current_thread")]
async fn named_management_does_not_block_command_reactor() {
    let reserved_command =
        std::net::TcpListener::bind("127.0.0.1:0").expect("reserve command port");
    let command_port = reserved_command.local_addr().expect("command address").port();
    drop(reserved_command);
    let reserved_response =
        std::net::TcpListener::bind("127.0.0.1:0").expect("reserve response port");
    let response_port = reserved_response.local_addr().expect("response address").port();
    drop(reserved_response);
    let command_endpoint = format!("tcp://127.0.0.1:{command_port}");
    let response_endpoint = format!("tcp://127.0.0.1:{response_port}");
    let mut responses = PullSocket::new();
    responses.bind(&response_endpoint).await.expect("bind response socket");
    let (daemon, worker) = daemon_with_async_management();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let server = tokio::spawn(super::super::run_zmq_rpc_loop_until(
        super::super::ZmqRpcLoopConfig {
            command_endpoint: command_endpoint.clone(),
            require_auth_for_remote: true,
        },
        daemon,
        shutdown_rx,
    ));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mut commands = PushSocket::new();
    commands.connect(&command_endpoint).await.expect("connect command socket");
    let payload = build_rpc_frame(
        78,
        "manage_interface",
        Some(serde_json::json!({ "operation": "attach", "name": "uplink" })),
    )
    .expect("management frame");
    let envelope =
        ZmqRpcEnvelope::request("management-command-test", 78, response_endpoint, payload, None);
    commands
        .send(ZmqMessage::from(zmq::encode_envelope(&envelope).expect("encode envelope")))
        .await
        .expect("send management request");
    let message = tokio::time::timeout(std::time::Duration::from_secs(3), responses.recv())
        .await
        .expect("management response deadline")
        .expect("management response");
    let bytes = Vec::<u8>::try_from(message).expect("response bytes");
    let response = zmq::decode_envelope(&bytes).expect("decode response");
    let rpc = parse_rpc_frame(&response.payload).expect("rpc response");
    assert!(rpc.error.is_none(), "management failed: {:?}", rpc.error);
    assert_eq!(rpc.result.unwrap_or_default()["complete"], true);

    worker.await.expect("worker join");
    shutdown_tx.send(true).expect("request command shutdown");
    server.await.expect("command task join").expect("command shutdown");
}

#[tokio::test(flavor = "current_thread")]
async fn command_shutdown_joins_active_blocking_management() {
    let reserved = std::net::TcpListener::bind("127.0.0.1:0").expect("port");
    let endpoint = format!("tcp://{}", reserved.local_addr().expect("address"));
    drop(reserved);
    let daemon = Arc::new(RpcDaemon::test_instance());
    let (requests, mut mutations) = tokio::sync::mpsc::unbounded_channel();
    daemon.set_interface_mutation_bridge(Arc::new(AsyncManagedBridge { requests }));
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let mut server = tokio::spawn(super::super::run_zmq_rpc_loop_until(
        super::super::ZmqRpcLoopConfig {
            command_endpoint: endpoint.clone(),
            require_auth_for_remote: true,
        },
        daemon,
        shutdown_rx,
    ));
    let mut commands = PushSocket::new();
    commands.connect(&endpoint).await.expect("commands");
    let payload = build_rpc_frame(
        99,
        "manage_interface",
        Some(serde_json::json!({"operation": "attach", "name": "uplink"})),
    )
    .expect("management");
    let envelope =
        ZmqRpcEnvelope::request("shutdown-mutation", 99, "tcp://127.0.0.1:1", payload, None);
    commands
        .send(ZmqMessage::from(zmq::encode_envelope(&envelope).expect("encode")))
        .await
        .expect("request");
    let reply = tokio::time::timeout(std::time::Duration::from_secs(1), mutations.recv())
        .await
        .expect("mutation starts")
        .expect("mutation owner");
    shutdown_tx.send(true).expect("shutdown");
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut server).await.is_err(),
        "loop must keep ownership of the active mutation"
    );
    reply.send(Ok(serde_json::json!({"complete": true}))).expect("release mutation");
    tokio::time::timeout(std::time::Duration::from_secs(1), server)
        .await
        .expect("joined mutation shutdown")
        .expect("server task")
        .expect("clean shutdown");
    assert!(mutations.try_recv().is_err());
}

async fn snapshot(endpoint: &str, request_id: u64) -> Result<serde_json::Value, String> {
    let mut client = DealerSocket::new();
    client.connect(endpoint).await.map_err(|error| error.to_string())?;
    let payload = build_rpc_frame(request_id, "sdk_snapshot_v2", Some(serde_json::json!({})))
        .map_err(|error| error.to_string())?;
    let mut envelope = rns_rpc::rpc::zmq::ZmqRpcEnvelope::request(
        format!("test-session-{request_id}"),
        request_id,
        "",
        payload,
        None,
    );
    envelope.response_endpoint = None;
    let encoded =
        rns_rpc::rpc::zmq::encode_envelope(&envelope).map_err(|error| error.to_string())?;
    client.send(ZmqMessage::from(encoded)).await.map_err(|error| error.to_string())?;
    let message = client.recv().await.map_err(|error| error.to_string())?;
    let bytes = Vec::<u8>::try_from(message).map_err(str::to_owned)?;
    let response = rns_rpc::rpc::zmq::decode_envelope(&bytes).map_err(|error| error.to_string())?;
    let rpc = parse_rpc_frame(&response.payload).map_err(|error| error.to_string())?;
    rpc.result.ok_or_else(|| format!("snapshot error: {:?}", rpc.error))
}
