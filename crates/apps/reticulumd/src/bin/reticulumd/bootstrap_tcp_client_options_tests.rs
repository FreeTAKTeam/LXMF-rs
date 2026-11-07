use super::restore_tests::{native_address, verify_unrelated_tcp};
use super::*;
use crate::interface_hot_apply::InterfaceHotApplyBridge;
use reticulum_daemon::config::InterfaceConfig;
use rns_transport::iface::tcp_client::TcpClient;
use rns_transport::iface::{IfaceRole, RxMessage, TxMessage, TxMessageType};
use rns_transport::packet::{Packet, PacketDataBuffer};
use tokio::net::TcpListener;
use tokio::time::{timeout, Duration};

struct Peer {
    transport: Arc<Transport>,
    address: AddressHash,
    packets: tokio::sync::broadcast::Receiver<RxMessage>,
}

impl Peer {
    async fn accept(listener: &TcpListener, recipe: &InterfaceConfig, bound: Duration) -> Self {
        let (stream, _) = timeout(bound, listener.accept())
            .await
            .expect("peer connection deadline")
            .expect("native connection");
        let transport = Arc::new(Transport::new(TransportConfig::default()));
        let packets = transport.iface_rx();
        let address = crate::interfaces::common::spawn_configured(
            &mut *transport.iface_manager().lock().await,
            TcpClient::new_from_stream("test-owned-peer", stream),
            TcpClient::spawn,
            IfaceRole::Unicast,
            recipe,
        )
        .expect("prepared native IFAC peer");
        Self { transport, address, packets }
    }

    async fn tunnel(&mut self, source: &Arc<Transport>, root: AddressHash) -> Vec<u8> {
        let destination = rns_transport::destination::PlainInputDestination::new(
            rns_transport::identity::EmptyIdentity {},
            rns_transport::destination::DestinationName::new("rnstransport", "tunnel.synthesize"),
        )
        .desc
        .address_hash;
        let packet = timeout(Duration::from_secs(2), async {
            loop {
                let received = self.packets.recv().await.expect("native peer event");
                if received.packet.destination == destination {
                    assert_eq!(received.address, self.address);
                    break received.packet;
                }
            }
        })
        .await
        .expect("authenticated tunnel synthesis deadline");
        let bytes = packet.data.as_slice();
        assert_eq!(bytes.len(), 176);
        let identity =
            rns_transport::identity::Identity::try_new_from_slices(&bytes[..32], &bytes[32..64])
                .expect("tunnel identity");
        assert_eq!(identity.address_hash, source.internal_identity().await);
        assert_eq!(
            &bytes[64..96],
            source
                .iface_manager()
                .lock()
                .await
                .full_hash(&root)
                .expect("native full hash")
                .as_slice()
        );
        assert!(rns_transport::identity::verify(
            bytes[32..64].try_into().expect("signing key"),
            &bytes[..112],
            &bytes[112..]
        ));
        bytes[96..112].to_vec()
    }

    async fn stop(self) {
        crate::interface_hot_apply::stop_owned_native(
            &self.transport.iface_manager(),
            Some(&self.transport),
            crate::configured_interface_runtime::NativeInterfaceRuntime::new(self.address, None),
        )
        .await
        .expect("native peer joined");
    }
}

async fn manage(daemon: &Arc<rns_rpc::RpcDaemon>, operation: &'static str) -> rns_rpc::RpcResponse {
    let daemon = daemon.clone();
    tokio::task::spawn_blocking(move || {
        daemon.handle_rpc(rns_rpc::RpcRequest {
            id: 1,
            method: "manage_interface".into(),
            params: Some(serde_json::json!({"operation":operation,"name":"options-source"})),
        })
    })
    .await
    .expect("management worker")
    .expect("management RPC")
}

async fn roundtrip(source: &Arc<Transport>, root: AddressHash, peer: &mut Peer, marker: u8) {
    let mut inbound = source.iface_rx();
    for (sender, address, destination, value) in
        [(&peer.transport, peer.address, root, marker), (source, root, peer.address, marker + 1)]
    {
        let packet = Packet {
            destination: AddressHash::new([value; 16]),
            data: PacketDataBuffer::new_from_slice(&[value, 0x7e, 0x7d]),
            ..Default::default()
        };
        let report = sender
            .iface_manager()
            .lock()
            .await
            .send(TxMessage { tx_type: TxMessageType::Direct(address), packet: packet.clone() })
            .await;
        assert_eq!(report.sent_ifaces, 1);
        let receiver = if value == marker { &mut inbound } else { &mut peer.packets };
        let received = timeout(Duration::from_secs(2), async {
            loop {
                let event = receiver.recv().await.expect("native event");
                if event.packet.destination == packet.destination {
                    break event;
                }
            }
        })
        .await
        .expect("exact authenticated data deadline");
        assert_eq!(received.address, destination);
        assert_eq!(received.packet.data, packet.data);
    }
}

#[tokio::test]
async fn named_tcp_replacement_preserves_options_initial_tunnel_and_later_reconnect() {
    let root = tempfile::tempdir().expect("state");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("native peer endpoint");
    let other = TcpListener::bind("127.0.0.1:0").await.expect("unrelated peer");
    let port = listener.local_addr().expect("endpoint").port();
    let other_port = other.local_addr().expect("endpoint").port();
    let config_text = |mtu, connect_timeout| {
        format!("interfaces=[{{type='tcp_client',enabled=true,name='options-source',host='127.0.0.1',port={port},ifac_size=64,network_name='options-network',passphrase='options-hook-secret',mtu={mtu},connect_timeout={connect_timeout},prefer_ipv6=true,i2p_tunneled=true}},{{type='tcp_client',enabled=true,name='unrelated',host='127.0.0.1',port={other_port}}}]\n[reticulum]\nshare_instance=false")
    };
    let config = DaemonConfig::from_toml(&config_text(512, 1)).expect("config");
    let path = root.path().join("reticulum.toml");
    std::fs::write(&path, config_text(512, 1)).expect("config file");
    let startup = start_fixture(&Args::parse_from(["reticulumd"]), &config, root.path()).await;
    let source = startup.transport.expect("transport");
    let initial = native_address(&startup.configured_interfaces[0]);
    let unrelated = native_address(&startup.configured_interfaces[1]);
    let daemon = Arc::new(rns_rpc::RpcDaemon::test_instance());
    daemon.replace_interfaces(startup.configured_interfaces.clone());
    let mut owner = InterfaceHotApplyBridge::spawn_with_configured_roots(
        source.clone(),
        startup.interfaces.into_owned_roots(&startup.configured_interfaces),
        Arc::downgrade(&daemon),
        None,
        startup.configured_startup,
    )
    .with_management_config(Some(path.clone()), true);
    daemon.set_interface_mutation_bridge(Arc::new(owner.bridge.clone()));
    let mut first = Peer::accept(&listener, &config.interfaces[0], Duration::from_secs(2)).await;
    let first_nonce = first.tunnel(&source, initial).await;
    let (mut unaffected, _) = timeout(Duration::from_secs(2), other.accept())
        .await
        .expect("unrelated deadline")
        .expect("unrelated connection");
    roundtrip(&source, initial, &mut first, 0x91).await;
    verify_unrelated_tcp(&source, unrelated, &mut unaffected, 0x93).await;

    let mut invalid = daemon.interface_records();
    invalid[0].settings.as_mut().expect("options")["connect_timeout"] = serde_json::json!(0);
    let response = daemon
        .handle_rpc(rns_rpc::RpcRequest {
            id: 2,
            method: "set_interfaces".into(),
            params: Some(serde_json::json!({"interfaces":invalid})),
        })
        .expect("rejected Apply response");
    assert!(response.error.is_some());
    assert!(!serde_json::to_string(&response).expect("response").contains("options-hook-secret"));
    assert_eq!(native_address(&daemon.interface_records()[0]), initial);
    roundtrip(&source, initial, &mut first, 0x94).await;

    std::fs::write(&path, config_text(1024, 0)).expect("invalid config file");
    let response = manage(&daemon, "reload").await;
    assert!(response.error.is_some());
    assert!(
        !serde_json::to_string(&response).expect("response").contains("options-hook-secret"),
        "named configuration diagnostics redact credentials"
    );
    assert_eq!(native_address(&daemon.interface_records()[0]), initial);
    roundtrip(&source, initial, &mut first, 0x96).await;

    let occupied = TcpListener::bind("127.0.0.1:0").await.expect("occupied replacement endpoint");
    let occupied_port = occupied.local_addr().expect("occupied endpoint").port();
    std::fs::write(&path, format!("interfaces=[{{type='tcp_server',enabled=true,name='options-source',host='127.0.0.1',port={occupied_port},ifac_size=64,network_name='options-network',passphrase='options-hook-secret',mtu=512}},{{type='tcp_client',enabled=true,name='unrelated',host='127.0.0.1',port={other_port}}}]\n[reticulum]\nshare_instance=false"))
        .expect("valid replacement with unavailable native bind");
    let response = manage(&daemon, "reload").await;
    let error = response.error.as_ref().expect("replacement bind rejected");
    assert!(error.message.contains("previous interface restored"), "{response:?}");
    assert!(!serde_json::to_string(&response).expect("response").contains("options-hook-secret"));
    let restored = daemon.interface_records();
    let rollback = native_address(&restored[0]);
    assert_ne!(rollback, initial);
    assert_eq!(
        restored[0].settings.as_ref().expect("status")["_runtime"]["tcp"]["stream_status"]["mtu"],
        512
    );
    assert_eq!(native_address(&restored[1]), unrelated);
    first.stop().await;
    let mut rollback_peer =
        Peer::accept(&listener, &config.interfaces[0], Duration::from_secs(2)).await;
    let rollback_nonce = rollback_peer.tunnel(&source, rollback).await;
    assert_ne!(rollback_nonce, first_nonce);
    roundtrip(&source, rollback, &mut rollback_peer, 0x81).await;
    verify_unrelated_tcp(&source, unrelated, &mut unaffected, 0x83).await;
    rollback_peer.stop().await;
    let mut first = Peer::accept(&listener, &config.interfaces[0], Duration::from_secs(8)).await;
    let rollback_reconnect_nonce = first.tunnel(&source, rollback).await;
    assert_ne!(rollback_reconnect_nonce, rollback_nonce);
    assert_eq!(native_address(&daemon.interface_records()[0]), rollback);
    roundtrip(&source, rollback, &mut first, 0x85).await;
    verify_unrelated_tcp(&source, unrelated, &mut unaffected, 0x87).await;

    std::fs::write(&path, config_text(1024, 1)).expect("replacement config");
    let response = manage(&daemon, "reload").await;
    assert!(response.error.is_none(), "{response:?}");
    let current = daemon.interface_records();
    let replacement = native_address(&current[0]);
    assert_ne!(replacement, initial);
    assert_ne!(replacement, rollback);
    assert_eq!(
        current[0].settings.as_ref().expect("status")["_runtime"]["tcp"]["stream_status"]["mtu"],
        1024
    );
    assert_eq!(native_address(&current[1]), unrelated);
    first.stop().await;
    let mut second = Peer::accept(&listener, &config.interfaces[0], Duration::from_secs(2)).await;
    let replacement_nonce = second.tunnel(&source, replacement).await;
    assert_ne!(replacement_nonce, first_nonce);
    assert_ne!(replacement_nonce, rollback_reconnect_nonce);
    roundtrip(&source, replacement, &mut second, 0x98).await;
    verify_unrelated_tcp(&source, unrelated, &mut unaffected, 0x9a).await;

    second.stop().await;
    let mut reconnected =
        Peer::accept(&listener, &config.interfaces[0], Duration::from_secs(8)).await;
    let reconnect_nonce = reconnected.tunnel(&source, replacement).await;
    assert_ne!(reconnect_nonce, replacement_nonce);
    assert_eq!(native_address(&daemon.interface_records()[0]), replacement);
    roundtrip(&source, replacement, &mut reconnected, 0x9c).await;
    verify_unrelated_tcp(&source, unrelated, &mut unaffected, 0x9e).await;
    timeout(Duration::from_secs(2), owner.shutdown())
        .await
        .expect("owner shutdown deadline")
        .expect("dynamic sender and native workers joined");
    assert!(source.interface_hashes().await.is_empty());
    reconnected.stop().await;
}
