use super::restore_tests::{native_address, verify_unrelated_tcp};
use super::*;
use crate::interface_hot_apply::{InterfaceHotApplyBridge, InterfaceMutationOwner};
use rns_transport::iface::{
    encode_packet_ifac, IfacState, InterfaceSharedConfig, TxMessage, TxMessageType,
};
use rns_transport::packet::{Packet, PacketDataBuffer};
use serde_json::Value;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{sleep, timeout, Duration};

const INITIAL: &str = "mesh-family-initial-credential";
const ROTATED: &str = "mesh-family-rotated-credential";
const FINAL: &str = "mesh-family-final-credential";

struct Fixture {
    transport: Arc<Transport>,
    daemon: Arc<rns_rpc::RpcDaemon>,
    owner: InterfaceMutationOwner,
    failures: Vec<String>,
}

impl Fixture {
    async fn start(config: &DaemonConfig, root: &std::path::Path) -> Self {
        let startup = start_fixture(&Args::parse_from(["reticulumd"]), config, root).await;
        let failures = startup
            .interfaces
            .startup_failures
            .iter()
            .map(|failure| failure.error.clone())
            .collect();
        let transport = startup.transport.expect("native transport");
        let daemon = Arc::new(rns_rpc::RpcDaemon::test_instance());
        daemon.replace_interfaces(startup.configured_interfaces.clone());
        let roots = startup.interfaces.into_owned_roots(&startup.configured_interfaces);
        assert_eq!(
            roots[0].native.as_ref().map(|root| root.auxiliary.len()),
            config.interfaces[0].simulation_loopback.filter(|enabled| *enabled).map(|_| 1),
            "configured simulation task transfers to the unique native owner"
        );
        let owner = InterfaceHotApplyBridge::spawn_with_configured_roots(
            transport.clone(),
            roots,
            Arc::downgrade(&daemon),
            None,
            startup.configured_startup,
        );
        daemon.set_interface_mutation_bridge(Arc::new(owner.bridge.clone()));
        Self { transport, daemon, owner, failures }
    }

    fn runtime(&self) -> Value {
        let runtime = self.daemon.interface_records()[0].settings.as_ref().expect("settings")
            ["_runtime"]
            .clone();
        let text = runtime.to_string();
        for credential in [INITIAL, ROTATED, FINAL] {
            assert!(!text.contains(credential), "runtime diagnostics expose credentials");
        }
        runtime
    }

    fn address(&self, index: usize) -> AddressHash {
        native_address(&self.daemon.interface_records()[index])
    }

    async fn shutdown(&mut self) {
        timeout(Duration::from_secs(10), self.owner.shutdown())
            .await
            .expect("existing owned cleanup deadline")
            .expect("native and simulation workers joined");
        assert!(self.transport.interface_hashes().await.is_empty());
    }
}

fn config(other: u16, credential: &str, loopback: bool) -> String {
    format!("interfaces=[{{type='meshtastic',enabled=true,name='mesh-family',simulation_loopback={loopback},simulation_node_id=41,max_payload_bytes=64,send_delay_ms=5,network_name='mesh-family-network',passphrase='{credential}'}},{{type='tcp_client',enabled=true,name='unrelated',host='127.0.0.1',port={other}}}]\n[reticulum]\nshare_instance=false")
}

async fn accept(listener: &TcpListener) -> TcpStream {
    timeout(Duration::from_secs(3), listener.accept())
        .await
        .expect("native TCP deadline")
        .expect("native TCP peer")
        .0
}

async fn closed(peer: &mut TcpStream) {
    timeout(Duration::from_secs(2), peer.read_to_end(&mut Vec::new()))
        .await
        .expect("retired TCP EOF deadline")
        .expect("retired TCP EOF");
}

async fn looped_packet(fixture: &Fixture, credential: &str, marker: u8) {
    let address = fixture.address(0);
    let manager = fixture.transport.iface_manager();
    let mut observed = fixture.transport.iface_rx();
    let state: IfacState = {
        let manager = manager.lock().await;
        let config = manager.shared_config(&address).expect("prepared native shared policy");
        assert_eq!(config.network_name.as_deref(), Some("mesh-family-network"));
        assert_eq!(config.passphrase.as_deref(), Some(credential));
        assert_eq!(config.ifac_size, None);
        assert_eq!(manager.mtu(&address), Some(564));
        Arc::new(std::sync::RwLock::new(
            config.ifac_context_with_default_size(16).expect("native default IFAC context"),
        ))
    };
    let packet = Packet {
        destination: AddressHash::new([marker; 16]),
        data: PacketDataBuffer::new_from_slice(&[marker; 140]),
        ..Default::default()
    };
    assert_eq!(
        encode_packet_ifac(&state, &packet).expect("independent policy encoding").len(),
        packet.to_bytes().expect("plain packet").len() + 16
    );
    assert_eq!(
        manager
            .lock()
            .await
            .send(TxMessage { tx_type: TxMessageType::Direct(address), packet: packet.clone() })
            .await
            .sent_ifaces,
        1
    );
    let received = timeout(Duration::from_secs(2), observed.recv())
        .await
        .expect("actual multi-chunk simulated admission deadline")
        .expect("native simulated admission");
    assert_eq!(received.address, address);
    assert_eq!(received.packet.destination, packet.destination);
    assert_eq!(received.packet.data, packet.data);
    assert_eq!(received.packet.ifac.map(|ifac| ifac.length), Some(0));
    assert_eq!(received.source, rns_transport::iface::IfaceSource::None);
}

async fn published(fixture: &Fixture, packets: u64) {
    timeout(Duration::from_secs(3), async {
        loop {
            let runtime = fixture.runtime();
            let mesh = &runtime["meshtastic"];
            let status = &mesh["status"];
            if status["packets_rx"] == packets && status["packets_tx"] == packets {
                assert!(status["chunks_rx"].as_u64().expect("native chunk count") >= 3 * packets);
                assert!(status["chunks_tx"].as_u64().expect("native chunk count") >= 3 * packets);
                assert_eq!(status["decode_errors"], 0);
                assert_eq!(mesh["evidence"], "simulated");
                assert_eq!(mesh["hardware_unverified"], true);
                assert_eq!(mesh["simulation_node_id"], 41);
                break;
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("native simulated status publication deadline");
}

#[tokio::test]
async fn configured_meshtastic_failure_rotation_and_restart_preserve_simulated_delivery() {
    let root = tempfile::tempdir().expect("owned state");
    let other = TcpListener::bind("127.0.0.1:0").await.expect("unrelated endpoint");
    let port = other.local_addr().expect("unrelated endpoint").port();
    let unavailable = DaemonConfig::from_toml(&config(port, INITIAL, false))
        .expect_err("unavailable native bearer rejected at the schema boundary");
    assert!(unavailable.to_string().contains("simulation_loopback must be true"));
    assert!(!unavailable.to_string().contains(INITIAL));
    let mut cfg = DaemonConfig::from_toml(&config(port, INITIAL, true)).expect("simulation config");
    // Programmatic callers still traverse the existing startup guard after parsing.
    cfg.interfaces[0].simulation_loopback = Some(false);
    let mut failed = Fixture::start(&cfg, root.path()).await;
    assert_eq!(failed.failures.len(), 1);
    assert_eq!(
        failed.failures[0],
        "Meshtastic daemon startup currently requires simulation_loopback = true"
    );
    assert_eq!(failed.runtime()["startup_status"], "failed");
    assert!(failed.runtime()["iface"].is_null());
    assert_eq!(failed.transport.interface_hashes().await.len(), 1);
    let mut unaffected = accept(&other).await;
    verify_unrelated_tcp(&failed.transport, failed.address(1), &mut unaffected, 0x81).await;
    failed.shutdown().await;
    closed(&mut unaffected).await;

    cfg.interfaces[0].simulation_loopback = Some(true);
    let mut fixture = Fixture::start(&cfg, root.path()).await;
    assert!(fixture.failures.is_empty());
    let address = fixture.address(0);
    let unrelated_address = fixture.address(1);
    let mut unaffected = accept(&other).await;
    verify_unrelated_tcp(&fixture.transport, unrelated_address, &mut unaffected, 0x84).await;
    looped_packet(&fixture, INITIAL, 0x87).await;
    published(&fixture, 1).await;
    {
        let manager = fixture.transport.iface_manager();
        let mut manager = manager.lock().await;
        let mut shared: InterfaceSharedConfig =
            manager.shared_config(&address).expect("active configured policy").clone();
        shared.passphrase = Some(ROTATED.into());
        assert!(manager.try_set_shared_config(address, shared).expect("live native key rotation"));
    }
    looped_packet(&fixture, ROTATED, 0x8a).await;
    published(&fixture, 2).await;
    assert_eq!(fixture.address(0), address);
    assert_eq!(fixture.address(1), unrelated_address);
    verify_unrelated_tcp(&fixture.transport, unrelated_address, &mut unaffected, 0x8d).await;
    let old_identity = fixture.transport.internal_identity().await;
    fixture.shutdown().await;
    closed(&mut unaffected).await;

    let cfg = DaemonConfig::from_toml(&config(port, FINAL, true)).expect("changed-key restart");
    let mut restarted = Fixture::start(&cfg, root.path()).await;
    assert!(restarted.failures.is_empty());
    assert_ne!(restarted.transport.internal_identity().await, old_identity);
    assert_eq!(restarted.runtime()["meshtastic"]["status"]["packets_rx"], 0);
    assert_eq!(restarted.runtime()["meshtastic"]["status"]["packets_tx"], 0);
    let mut unaffected = accept(&other).await;
    verify_unrelated_tcp(&restarted.transport, restarted.address(1), &mut unaffected, 0x90).await;
    looped_packet(&restarted, FINAL, 0x93).await;
    published(&restarted, 1).await;
    restarted.shutdown().await;
    closed(&mut unaffected).await;
}
