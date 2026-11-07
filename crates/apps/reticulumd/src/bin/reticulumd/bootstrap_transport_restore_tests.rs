use super::*;
use crate::interface_hot_apply::{InterfaceHotApplyBridge, NativeDiscoveryConfig};
use rns_transport::iface::{
    encode_packet_ifac, IfacState, InterfaceSharedConfig, TxMessage, TxMessageType,
};
use rns_transport::packet::{Packet, PacketDataBuffer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{sleep, timeout, Duration};

fn packet(marker: u8) -> Packet {
    Packet {
        destination: AddressHash::new([marker; 16]),
        data: PacketDataBuffer::new_from_slice(&[marker, 0xc0, 0xdb, 0x7e, 0x7d]),
        ..Packet::default()
    }
}

fn auth_state(secret: &str) -> IfacState {
    Arc::new(std::sync::RwLock::new(
        InterfaceSharedConfig {
            network_name: Some("restored-kiss-network".into()),
            passphrase: Some(secret.into()),
            ..Default::default()
        }
        .ifac_context_with_default_size(8)
        .expect("independent peer IFAC context"),
    ))
}

pub(super) fn native_address(record: &InterfaceRecord) -> AddressHash {
    AddressHash::new_from_hex_string(
        record.settings.as_ref().expect("settings")["_runtime"]["iface"]
            .as_str()
            .expect("native address")
            .trim_matches('/'),
    )
    .expect("native hash")
}

async fn expect_wire(peer: &mut TcpStream, expected: &[u8]) {
    timeout(Duration::from_secs(2), async {
        let mut received = Vec::new();
        let mut chunk = [0; 2048];
        loop {
            let length = peer.read(&mut chunk).await.expect("native wire read");
            assert_ne!(length, 0, "native peer closed before exact framed packet");
            received.extend_from_slice(&chunk[..length]);
            assert!(received.len() <= 16_384, "unexpected native output before target frame");
            if received.windows(expected.len()).any(|window| window == expected) {
                return;
            }
        }
    })
    .await
    .expect("exact native egress deadline");
}

async fn verify_kiss_authentication(
    transport: &Arc<Transport>,
    address: AddressHash,
    peer: &mut TcpStream,
    marker: u8,
) {
    let manager = transport.iface_manager();
    let mut observed = transport.iface_rx();
    let violations = manager
        .lock()
        .await
        .traffic_snapshots()
        .into_iter()
        .find(|entry| entry.address == address)
        .expect("native traffic")
        .ifac_violations;
    let rejected = packet(marker);
    let wrong = encode_packet_ifac(&auth_state("wrong-key"), &rejected).expect("wrong-key packet");
    let mut rejected_frames = rns_transport::kiss::encode_data_frame(&wrong);
    rejected_frames.extend(rns_transport::kiss::encode_data_frame(
        &rejected.to_bytes().expect("plaintext packet"),
    ));
    peer.write_all(&rejected_frames).await.expect("native negative frames");
    timeout(Duration::from_secs(2), async {
        loop {
            let count = manager
                .lock()
                .await
                .traffic_snapshots()
                .into_iter()
                .find(|entry| entry.address == address)
                .expect("native root remains")
                .ifac_violations;
            if count == violations + 2 {
                break;
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("both malformed-authentication frames rejected natively");
    assert!(observed.try_recv().is_err(), "negative frames cannot enter native admission");

    let state = auth_state("retained-startup-key");
    let inbound = packet(marker + 1);
    let wire = rns_transport::kiss::encode_data_frame(
        &encode_packet_ifac(&state, &inbound).expect("authenticated inbound"),
    );
    peer.write_all(&wire).await.expect("native authenticated ingress");
    let received = timeout(Duration::from_secs(2), observed.recv())
        .await
        .expect("native ingress deadline")
        .expect("native ingress event");
    assert_eq!(received.address, address);
    assert_eq!(received.packet.destination, inbound.destination);
    assert_eq!(received.packet.data, inbound.data);

    let outbound = packet(marker + 2);
    let expected = rns_transport::kiss::encode_data_frame(
        &encode_packet_ifac(&state, &outbound).expect("authenticated outbound"),
    );
    let sent = manager
        .lock()
        .await
        .send(TxMessage { tx_type: TxMessageType::Direct(address), packet: outbound })
        .await;
    assert_eq!(sent.sent_ifaces, 1);
    expect_wire(peer, &expected).await;
}

pub(super) async fn verify_unrelated_tcp(
    transport: &Arc<Transport>,
    address: AddressHash,
    peer: &mut TcpStream,
    marker: u8,
) {
    let mut observed = transport.iface_rx();
    let inbound = packet(marker);
    let wire = rns_transport::iface::hdlc::Hdlc::frame(&inbound.to_bytes().expect("packet"))
        .expect("HDLC");
    peer.write_all(&wire).await.expect("unrelated native ingress");
    let received = timeout(Duration::from_secs(2), observed.recv())
        .await
        .expect("unrelated ingress deadline")
        .expect("unrelated ingress");
    assert_eq!(received.address, address);
    assert_eq!(received.packet.data, inbound.data);
    let outbound = packet(marker + 1);
    let expected = rns_transport::iface::hdlc::Hdlc::frame(&outbound.to_bytes().expect("packet"))
        .expect("HDLC");
    assert_eq!(
        transport
            .iface_manager()
            .lock()
            .await
            .send(TxMessage { tx_type: TxMessageType::Direct(address), packet: outbound })
            .await
            .sent_ifaces,
        1
    );
    expect_wire(peer, &expected).await;
}

#[tokio::test]
async fn discovery_restores_original_index_kiss_authentication_and_preserves_unrelated_traffic() {
    let root = tempfile::tempdir().expect("state");
    let bootstrap_peer = TcpListener::bind("127.0.0.1:0").await.expect("KISS peer");
    let unrelated_peer = TcpListener::bind("127.0.0.1:0").await.expect("unrelated peer");
    let candidate_peer = TcpListener::bind("127.0.0.2:0").await.expect("discovered peer");
    let config = DaemonConfig::from_toml(&format!(
        "interfaces=[{{type='udp',enabled=false,name='sentinel-a'}},{{type='udp',enabled=false,name='sentinel-b'}},{{type='kiss_tcp_client',enabled=true,host='127.0.0.1',port={},bootstrap_only=true,network_name='restored-kiss-network',passphrase='retained-startup-key'}},{{type='tcp_client',enabled=true,name='unrelated',host='127.0.0.1',port={}}}]\n[reticulum]\nshare_instance=false\ndiscover_interfaces=true\nautoconnect_discovered_interfaces=1\n",
        bootstrap_peer.local_addr().expect("KISS port").port(),
        unrelated_peer.local_addr().expect("unrelated port").port(),
    )).expect("config");
    let startup = start_fixture(&Args::parse_from(["reticulumd"]), &config, root.path()).await;
    let transport = startup.transport.as_ref().expect("transport").clone();
    let daemon = Arc::new(rns_rpc::RpcDaemon::test_instance());
    daemon.replace_interfaces(startup.configured_interfaces.clone());
    let initial = native_address(&startup.configured_interfaces[2]);
    let unrelated = native_address(&startup.configured_interfaces[3]);
    let discovery = NativeDiscoveryConfig::new(
        root.path().to_owned(),
        &reticulum_daemon::config::ReticulumRuntimePolicy {
            discover_interfaces: true,
            max_autoconnected_interfaces: 1,
            ..Default::default()
        },
        false,
        &config.interfaces,
        (3600, 5, 0),
    );
    let mut owner = InterfaceHotApplyBridge::spawn_with_configured_roots(
        transport.clone(),
        startup.interfaces.into_owned_roots(&startup.configured_interfaces),
        Arc::downgrade(&daemon),
        discovery,
        startup.configured_startup,
    );
    daemon.set_interface_mutation_bridge(Arc::new(owner.bridge.clone()));
    let (mut first, _) = timeout(Duration::from_secs(2), bootstrap_peer.accept())
        .await
        .expect("initial KISS deadline")
        .expect("initial KISS");
    let (mut unrelated_stream, _) = timeout(Duration::from_secs(2), unrelated_peer.accept())
        .await
        .expect("unrelated deadline")
        .expect("unrelated root");
    verify_kiss_authentication(&transport, initial, &mut first, 0x31).await;
    verify_unrelated_tcp(&transport, unrelated, &mut unrelated_stream, 0x41).await;

    // Keep immutable recipe index 2 while moving its public occurrence to slot 1.
    let retained_tcp = daemon.interface_records()[3].clone();
    let mut events = daemon.subscribe_events();
    let response = daemon
        .handle_rpc(rns_rpc::RpcRequest {
            id: 1,
            method: "set_interfaces".into(),
            params: Some(serde_json::json!({"interfaces":[retained_tcp]})),
        })
        .expect("public Apply");
    assert!(response.error.is_none(), "{response:?}");
    timeout(Duration::from_secs(2), async {
        loop {
            let event = events.recv().await.expect("Apply completion event");
            if event.event_type == "interfaces_updated" && event.payload["complete"] == true {
                break;
            }
        }
    })
    .await
    .expect("native Apply completed");
    assert_eq!(daemon.interface_records().len(), 2);
    assert_eq!(native_address(&daemon.interface_records()[0]), unrelated);
    assert_eq!(native_address(&daemon.interface_records()[1]), initial);

    // This fixture begins at the already-validated observation boundary.
    let info = serde_json::from_value(serde_json::json!({
        "discovery_hash": vec![7u8; 32], "type":"BackboneInterface", "transport":true,
        "impl_name":"RNS", "version":"1.5.5", "name":"replacement",
        "received":rns_transport::time::now_epoch_secs_u64() as f64,"value":1,
        "transport_id":"11111111111111111111111111111111",
        "network_id":"22222222222222222222222222222222", "hops":1,
        "latitude":null,"longitude":null,"height":null,"reachable_on":"127.0.0.2",
        "port":candidate_peer.local_addr().expect("candidate port").port(),
    }))
    .expect("qualified observation");
    owner.discovery_observations().observed(info).await.expect("native admission queue");
    let (candidate, _) = timeout(Duration::from_secs(2), candidate_peer.accept())
        .await
        .expect("candidate deadline")
        .expect("candidate native connection");
    timeout(Duration::from_secs(8), async {
        while daemon.interface_records()[1].enabled {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("discovery disables configured KISS root");
    let mut drained = Vec::new();
    timeout(Duration::from_secs(2), first.read_to_end(&mut drained))
        .await
        .expect("old KISS EOF deadline")
        .expect("old KISS closed");
    assert_eq!(transport.iface_manager().lock().await.online(&initial), None);
    verify_unrelated_tcp(&transport, unrelated, &mut unrelated_stream, 0x51).await;
    drop(candidate);
    drop(candidate_peer);

    let (mut restored, _) = timeout(Duration::from_secs(8), bootstrap_peer.accept())
        .await
        .expect("trusted recipe restores the real KISS TCP connection")
        .expect("restored KISS");
    let restored_address = timeout(Duration::from_secs(2), async {
        loop {
            let records = daemon.interface_records();
            if records[1].enabled {
                let address = native_address(&records[1]);
                assert_eq!(native_address(&records[0]), unrelated);
                assert_eq!(records[1].kind, "kiss_tcp_client");
                let status = &records[1].settings.as_ref().expect("settings")["_runtime"];
                assert_eq!(status["managed_by"], "daemon_transport");
                assert_eq!(status["runtime_status"], "running");
                assert!(!status.to_string().contains("retained-startup-key"));
                assert_eq!(records.len(), 3, "one restored occurrence plus the discovered record");
                break address;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("restored original publication slot");
    assert_ne!(restored_address, initial);
    verify_kiss_authentication(&transport, restored_address, &mut restored, 0x61).await;
    verify_unrelated_tcp(&transport, unrelated, &mut unrelated_stream, 0x71).await;
    owner.shutdown().await.expect("joined native owner");
    assert!(transport.iface_manager().lock().await.interface_hashes().is_empty());
}
