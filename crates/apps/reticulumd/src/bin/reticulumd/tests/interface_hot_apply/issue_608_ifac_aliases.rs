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

const TYPES_NETWORK: &str = "sdk-ifac-types-network";
const TYPES_KEY: &str = "sdk-ifac-types-retained-key";

async fn type_guard_traffic(
    transport: &Arc<Transport>,
    address: AddressHash,
    peer: &UdpSocket,
    worker: std::net::SocketAddr,
    unrelated: AddressHash,
    tcp_peer: &mut TcpStream,
    marker: u8,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let state = Arc::new(RwLock::new(
        InterfaceSharedConfig {
            ifac_size: Some(128),
            network_name: Some(TYPES_NETWORK.into()),
            passphrase: Some(TYPES_KEY.into()),
            ..Default::default()
        }
        .ifac_context()
        .expect("independent retained IFAC"),
    ));
    let packet = Packet {
        destination: AddressHash::new([marker; 16]),
        data: PacketDataBuffer::new_from_slice(&[marker, 0x00, 0xff, 0x7d, 0x7e]),
        ..Default::default()
    };
    let mut ingress = transport.iface_rx();
    peer.send_to(&encode_packet_ifac(&state, &packet).expect("authenticated ingress"), worker)
        .await
        .expect("native UDP ingress");
    let received = timeout(Duration::from_secs(2), ingress.recv())
        .await
        .expect("retained UDP ingress deadline")
        .expect("native UDP event");
    assert_eq!(received.address, address);
    assert_eq!(received.packet.destination, packet.destination);
    assert_eq!(received.packet.data, packet.data);
    assert!(received.packet.ifac.is_some());
    assert_eq!(
        transport
            .iface_manager()
            .lock()
            .await
            .send(TxMessage { tx_type: TxMessageType::Direct(address), packet: packet.clone() })
            .await
            .sent_ifaces,
        1
    );
    let mut wire = [0; 2048];
    let (length, source) = timeout(Duration::from_secs(2), peer.recv_from(&mut wire))
        .await
        .expect("retained UDP egress deadline")
        .expect("native UDP egress");
    assert_eq!(source, worker);
    assert_eq!(&wire[..length], encode_packet_ifac(&state, &packet).expect("retained wire"));
    assert_eq!(
        decode_packet_ifac(&state, &wire[..length]).expect("authenticated egress").data,
        packet.data
    );

    let packet = Packet { destination: AddressHash::new([marker + 1; 16]), ..packet };
    let mut ingress = transport.iface_rx();
    let frame =
        rns_transport::iface::hdlc::Hdlc::frame(&packet.to_bytes().expect("unrelated packet"))
            .expect("unrelated HDLC");
    tcp_peer.write_all(&frame).await.expect("unrelated native ingress");
    let received = timeout(Duration::from_secs(2), ingress.recv())
        .await
        .expect("unrelated ingress deadline")
        .expect("unrelated ingress");
    assert_eq!(received.address, unrelated);
    assert_eq!(received.packet.data, packet.data);
    assert_eq!(
        transport
            .iface_manager()
            .lock()
            .await
            .send(TxMessage { tx_type: TxMessageType::Direct(unrelated), packet })
            .await
            .sent_ifaces,
        1
    );
    timeout(Duration::from_secs(2), async {
        let mut received = Vec::new();
        loop {
            let count = tcp_peer.read(&mut wire).await.expect("unrelated egress");
            assert_ne!(count, 0);
            received.extend_from_slice(&wire[..count]);
            assert!(received.len() <= 16_384);
            if received.windows(frame.len()).any(|candidate| candidate == frame) {
                break;
            }
        }
    })
    .await
    .expect("exact unrelated egress deadline");
}

fn type_guard_addresses(daemon: &Arc<RpcDaemon>) -> (AddressHash, AddressHash) {
    let records = daemon.interface_records();
    assert_eq!(records.len(), 2);
    let address = |name: &str| {
        let record = records
            .iter()
            .find(|record| record.name.as_deref() == Some(name))
            .expect("native record");
        AddressHash::new_from_hex_string(
            record.settings.as_ref().expect("settings")["_runtime"]["iface"]
                .as_str()
                .expect("native address")
                .trim_matches('/'),
        )
        .expect("native hash")
    };
    (address("sdk-ifac-types"), address("sdk-ifac-unrelated"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sdk_ifac_type_rejection_preserves_authenticated_udp_and_unrelated_native_tcp() {
    use tokio::io::AsyncReadExt;
    let transport = Arc::new(Transport::new(TransportConfig::default()));
    let daemon = Arc::new(RpcDaemon::test_instance());
    let mut owner = InterfaceHotApplyBridge::spawn_with_configured_roots(
        transport.clone(),
        Vec::new(),
        Arc::downgrade(&daemon),
        None,
        None,
    );
    daemon.set_interface_mutation_bridge(Arc::new(owner.bridge.clone()));
    let peer = UdpSocket::bind("127.0.0.1:0").await.expect("UDP peer");
    let reserved = UdpSocket::bind("127.0.0.1:0").await.expect("native endpoint reservation");
    let worker = reserved.local_addr().expect("native endpoint");
    let other = TcpListener::bind("127.0.0.1:0").await.expect("unrelated peer");
    let mut record = udp_peer_record(
        "sdk-ifac-types",
        "127.0.0.1",
        worker.port(),
        "127.0.0.1",
        peer.local_addr().expect("peer address").port(),
    );
    record.settings.as_mut().expect("settings")["network_name"] = json!(TYPES_NETWORK);
    record.settings.as_mut().expect("settings")["passphrase"] = json!(TYPES_KEY);
    record.settings.as_mut().expect("settings")["ifac_size"] = json!(128);
    let unrelated = tcp_record(
        "sdk-ifac-unrelated",
        "127.0.0.1",
        other.local_addr().expect("unrelated endpoint").port(),
    );
    let valid = vec![record.clone(), unrelated.clone()];
    drop(reserved);

    let rpc_socket =
        TcpListener::bind("127.0.0.1:0").await.expect("canonical endpoint reservation");
    let rpc_address = rpc_socket.local_addr().expect("canonical endpoint");
    let endpoint = format!("tcp://{rpc_address}");
    drop(rpc_socket);
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let server = tokio::spawn(crate::zmq_rpc_loop::run_zmq_router_loop_until(
        endpoint.clone(),
        true,
        daemon.clone(),
        stopped,
    ));
    drop(wait_for_tcp_server_connect("127.0.0.1", rpc_address.port()).await);
    let configured = valid.clone();
    let mut events = daemon.subscribe_events();
    let client = tokio::task::spawn_blocking(move || {
        let client = Client::new(
            ZmqPipelineBackendClient::new(ZmqPipelineBackendConfig::local(endpoint))
                .expect("canonical backend"),
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
                json!({"interfaces":configured}),
            ))
            .expect("initial SDK admission");
        assert!(result.accepted);
        assert_eq!(result.value["queued"], true);
        client
    })
    .await
    .expect("SDK setup worker");
    timeout(Duration::from_secs(2), async {
        loop {
            let event = events.recv().await.expect("initial native commit");
            if event.event_type == "interfaces_updated" && event.payload["complete"] == true {
                break;
            }
        }
    })
    .await
    .expect("actual initial mutation completion");
    timeout(Duration::from_secs(3), async {
        loop {
            if daemon.interface_records().iter().any(|record| {
                record.name.as_deref() == Some("sdk-ifac-types")
                    && record.settings.as_ref().expect("settings")["_runtime"]["udp"]["status"]
                        ["link_state"]
                        == "bound"
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("native UDP bound status publication");
    let (address, unrelated_address) = type_guard_addresses(&daemon);
    let (mut unaffected, _) = timeout(Duration::from_secs(2), other.accept())
        .await
        .expect("unrelated native deadline")
        .expect("unrelated native connection");
    type_guard_traffic(
        &transport,
        address,
        &peer,
        worker,
        unrelated_address,
        &mut unaffected,
        0x71,
    )
    .await;

    let mut invalid_alias = record.clone();
    invalid_alias.settings.as_mut().expect("settings")["network_name"] =
        json!({"must-not-log-ifac-value":true});
    invalid_alias.settings.as_mut().expect("settings")["networkname"] = json!(TYPES_NETWORK);
    let mut invalid_size = record.clone();
    invalid_size.settings.as_mut().expect("settings")["ifac_size"] = json!(128.0);
    let mut missing_credentials = record;
    let settings =
        missing_credentials.settings.as_mut().expect("settings").as_object_mut().expect("object");
    settings.remove("network_name");
    settings.remove("passphrase");
    settings.insert("ifac_size".into(), json!(-1));
    let client = tokio::task::spawn_blocking(move || {
        for invalid in [invalid_alias, invalid_size, missing_credentials] {
            let error = client
                .rns_interfaces(SdkControlRequest::new(
                    RnsInterfacesOperation::Set,
                    json!({"interfaces":[invalid,unrelated.clone()]}),
                ))
                .expect_err("malformed SDK replacement must reject before native mutation");
            let text = error.to_string();
            assert!(!text.contains("must-not-log-ifac-value"));
            assert!(!text.contains(TYPES_KEY));
            assert!(text.contains("IFAC interface configuration was rejected"), "{text}");
        }
        client
    })
    .await
    .expect("SDK rejected replacement worker");
    assert_eq!(type_guard_addresses(&daemon), (address, unrelated_address));
    assert_eq!(
        transport
            .iface_manager()
            .lock()
            .await
            .shared_config(&address)
            .expect("retained policy")
            .passphrase
            .as_deref(),
        Some(TYPES_KEY)
    );
    type_guard_traffic(
        &transport,
        address,
        &peer,
        worker,
        unrelated_address,
        &mut unaffected,
        0x74,
    )
    .await;

    let mut events = daemon.subscribe_events();
    let client = tokio::task::spawn_blocking(move || {
        assert!(
            client
                .rns_interfaces(SdkControlRequest::new(
                    RnsInterfacesOperation::Set,
                    json!({"interfaces":valid})
                ))
                .expect("valid SDK command after rejections")
                .accepted
        );
        client
    })
    .await
    .expect("valid SDK recovery worker");
    timeout(Duration::from_secs(2), async {
        loop {
            let event = events.recv().await.expect("valid native commit");
            if event.event_type == "interfaces_updated" && event.payload["complete"] == true {
                break;
            }
        }
    })
    .await
    .expect("actual valid mutation completion");
    assert_eq!(type_guard_addresses(&daemon), (address, unrelated_address));
    type_guard_traffic(
        &transport,
        address,
        &peer,
        worker,
        unrelated_address,
        &mut unaffected,
        0x77,
    )
    .await;
    tokio::task::spawn_blocking(move || {
        client.shutdown(ShutdownMode::Graceful).expect("SDK shutdown")
    })
    .await
    .expect("SDK shutdown worker");
    timeout(Duration::from_secs(10), owner.shutdown())
        .await
        .expect("native cleanup bound")
        .expect("native workers joined");
    assert!(transport.interface_hashes().await.is_empty());
    timeout(Duration::from_secs(2), unaffected.read_to_end(&mut Vec::new()))
        .await
        .expect("unrelated EOF deadline")
        .expect("unrelated EOF");
    stop.send_replace(true);
    timeout(Duration::from_secs(2), server)
        .await
        .expect("canonical router stop")
        .expect("router join")
        .expect("router result");
}
