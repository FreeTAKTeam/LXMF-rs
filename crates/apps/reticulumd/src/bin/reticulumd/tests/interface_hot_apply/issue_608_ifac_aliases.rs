//! SDK credentials follow startup precedence and preserve legacy RPC aliases.
use super::*;
use lxmf_sdk::{
    Client, LxmfSdk, RnsInterfacesOperation, RnsSdkInterfaces, SdkConfig, SdkControlRequest,
    ShutdownMode, StartRequest, ZmqPipelineBackendClient, ZmqPipelineBackendConfig,
};
use rns_transport::hash::AddressHash;
use rns_transport::iface::{decode_packet_ifac, encode_packet_ifac, TxMessage, TxMessageType};
use rns_transport::packet::{Packet, PacketDataBuffer};
use std::sync::RwLock;
use tokio::net::UdpSocket;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sdk_empty_network_name_uses_nonempty_startup_alias_on_udp_wire() {
    check_sdk_ifac_aliases(
        json!({ "network_name": "", "networkname": "field-net" }),
        "field-net",
        None,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sdk_pass_phrase_precedes_passphrase_as_at_startup_on_udp_wire() {
    check_sdk_ifac_aliases(
        json!({
            "network_name": "field-net", "networkname": "ignored-net",
            "pass_phrase": "preferred-key", "passphrase": "ignored-key"
        }),
        "field-net",
        Some("preferred-key"),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sdk_ifac_legacy_aliases_preserve_whitespace_credentials_on_udp_wire() {
    check_sdk_ifac_aliases(
        json!({
            "network_name": "", "networkname": "", "ifac_netname": " field-net ",
            "pass_phrase": "", "passphrase": "", "ifac_netkey": " preferred-key "
        }),
        " field-net ",
        Some(" preferred-key "),
    )
    .await;
}

async fn check_sdk_ifac_aliases(mut settings: serde_json::Value, network: &str, key: Option<&str>) {
    let daemon = Arc::new(RpcDaemon::test_instance());
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let incoming = manager.lock().await.receiver();
    let bridge = InterfaceHotApplyBridge::spawn_with_daemon(
        manager.clone(),
        Vec::new(),
        Arc::downgrade(&daemon),
    );
    daemon.set_interface_mutation_bridge(Arc::new(bridge.clone()));

    let probe = UdpSocket::bind("127.0.0.1:0").await.expect("UDP wire probe");
    let reservation = UdpSocket::bind("127.0.0.1:0").await.expect("worker port reservation");
    let worker_addr = reservation.local_addr().expect("worker address");
    drop(reservation);
    settings["target_host"] = json!("127.0.0.1");
    settings["target_port"] = json!(probe.local_addr().expect("probe address").port());
    let mut record = udp_record("sdk-ifac-aliases", "127.0.0.1", worker_addr.port());
    record.settings = Some(settings);

    let reserved = TcpListener::bind("127.0.0.1:0").await.expect("canonical endpoint reservation");
    let rpc_addr = reserved.local_addr().expect("canonical endpoint");
    let endpoint = format!("tcp://{rpc_addr}");
    drop(reserved);
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let server = tokio::spawn(crate::zmq_rpc_loop::run_zmq_router_loop_until(
        endpoint.clone(),
        true,
        daemon,
        stopped,
    ));
    drop(wait_for_tcp_server_connect("127.0.0.1", rpc_addr.port()).await);
    let client = tokio::task::spawn_blocking(move || {
        let client = Client::new(
            ZmqPipelineBackendClient::new(ZmqPipelineBackendConfig::local(endpoint))
                .expect("real canonical backend"),
        );
        client
            .start(
                StartRequest::new(SdkConfig::desktop_full_default())
                    .with_requested_capabilities(["sdk.capability.rns_interfaces"]),
            )
            .expect("SDK start");
        let result = client
            .rns_interfaces(SdkControlRequest::new(
                RnsInterfacesOperation::Set,
                json!({ "interfaces": [record] }),
            ))
            .expect("canonical interface command");
        assert!(result.accepted, "interface command: {result:?}");
        assert_eq!(result.value["queued"], true, "SDK reports admission, not native commit");
        assert_eq!(result.value["complete"], false);
        client
    })
    .await
    .expect("SDK interface command task");
    let (iface, refresh) = wait_for_hot_apply_udp_refresh(&bridge).await;
    timeout(Duration::from_secs(2), async {
        while refresh.status.snapshot().link_state != "bound" {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("configured UDP worker bound");

    let packet = Packet {
        destination: AddressHash::new_from_slice(&[0x42; 16]),
        data: PacketDataBuffer::new_from_slice(b"SDK IFAC aliases wire payload"),
        ..Packet::default()
    };
    let expected = InterfaceSharedConfig {
        network_name: Some(network.to_string()),
        passphrase: key.map(ToOwned::to_owned),
        ..Default::default()
    };
    let expected_state = Arc::new(RwLock::new(expected.ifac_context().expect("expected IFAC")));
    let wrong = InterfaceSharedConfig {
        network_name: Some(network.to_string()),
        passphrase: Some("wrong-key".to_string()),
        ..Default::default()
    };
    let wrong_state = Arc::new(RwLock::new(wrong.ifac_context().expect("wrong-key IFAC")));

    probe.send_to(&packet.to_bytes().expect("plaintext packet"), worker_addr).await.expect("probe");
    wait_processed(&refresh, 1).await;
    assert_eq!(refresh.status.snapshot().packets_rx, 0, "plaintext must never be admitted");
    probe
        .send_to(&encode_packet_ifac(&wrong_state, &packet).expect("wrong-key frame"), worker_addr)
        .await
        .expect("wrong-key probe");
    wait_processed(&refresh, 2).await;
    assert_eq!(refresh.status.snapshot().decode_errors, 2, "both negative controls rejected");
    assert_eq!(refresh.status.snapshot().packets_rx, 0);

    probe
        .send_to(
            &encode_packet_ifac(&expected_state, &packet).expect("matching frame"),
            worker_addr,
        )
        .await
        .expect("matching probe");
    let received = timeout(Duration::from_secs(2), incoming.lock().await.recv())
        .await
        .expect("matching expected credentials must deliver")
        .expect("worker ingress queue");
    assert_eq!(received.address, iface);
    assert_eq!(received.packet.data, packet.data);
    assert!(received.packet.ifac.is_some(), "ingress retains authentication provenance");
    let mut manager_guard = manager.lock().await;
    let violations = manager_guard
        .traffic_snapshots()
        .into_iter()
        .find(|entry| entry.address == iface)
        .expect("IFAC counters")
        .ifac_violations;
    assert_eq!(violations, 2);
    let trace = manager_guard
        .send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet: packet.clone() })
        .await;
    assert_eq!(trace.sent_ifaces, 1);
    drop(manager_guard);
    let mut wire = [0_u8; 1024];
    let (length, source) = timeout(Duration::from_secs(2), probe.recv_from(&mut wire))
        .await
        .expect("first egress frame")
        .expect("wire receive");
    assert_eq!(source, worker_addr);
    assert_ne!(wire[0] & 0x80, 0, "first egress uses IFAC");
    let decoded =
        decode_packet_ifac(&expected_state, &wire[..length]).expect("startup key on wire");
    assert_eq!(decoded.data, packet.data);

    tokio::task::spawn_blocking(move || {
        assert!(
            client
                .rns_interfaces(SdkControlRequest::new(
                    RnsInterfacesOperation::Set,
                    json!({ "interfaces": [] }),
                ))
                .expect("SDK detach")
                .accepted
        );
        client.shutdown(ShutdownMode::Graceful).expect("SDK shutdown");
    })
    .await
    .expect("SDK cleanup task");
    timeout(Duration::from_secs(2), async {
        while !manager.lock().await.interface_hashes().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("owned worker removal");
    stop.send_replace(true);
    timeout(Duration::from_secs(2), server)
        .await
        .expect("router stop")
        .expect("router join")
        .expect("router result");
}

async fn wait_processed(refresh: &HotApplyUdpRefresh, count: u64) {
    timeout(Duration::from_secs(2), async {
        loop {
            let status = refresh.status.snapshot();
            if status.packets_rx + status.decode_errors >= count {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("real UDP decode processing barrier");
}
