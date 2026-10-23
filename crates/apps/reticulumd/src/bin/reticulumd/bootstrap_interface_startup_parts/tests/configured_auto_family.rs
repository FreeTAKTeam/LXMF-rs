use super::*;
use crate::interface_hot_apply::{InterfaceHotApplyBridge, InterfaceMutationOwner};
use rns_transport::iface::{
    encode_packet_ifac, IfacState, InterfaceSharedConfig, TxMessage, TxMessageType,
};
use rns_transport::packet::{Packet, PacketDataBuffer};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::time::{sleep, timeout, Duration};

const INITIAL: &str = "auto-family-initial-credential";
const ROTATED: &str = "auto-family-rotated-credential";
const FINAL: &str = "auto-family-final-credential";

struct Fixture {
    transport: Arc<Transport>,
    daemon: Arc<rns_rpc::RpcDaemon>,
    owner: InterfaceMutationOwner,
    failures: Vec<String>,
}

impl Fixture {
    async fn start(config: &DaemonConfig, plan: AutoRuntimePlan) -> Self {
        let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let identity = rns_transport::identity_bridge::to_transport_private_identity(&identity);
        let transport = Arc::new(Transport::new(rns_transport::transport::TransportConfig::new(
            "auto-family",
            &identity,
            true,
        )));
        let manager = transport.iface_manager();
        let (batch, records) =
            start_configured_auto_for_test(plan, config, &transport, &manager).await;
        let failures = batch.startup_failures.iter().map(|failure| failure.error.clone()).collect();
        let daemon = Arc::new(rns_rpc::RpcDaemon::test_instance());
        daemon.replace_interfaces(records.clone());
        let owner = InterfaceHotApplyBridge::spawn_with_configured_roots(
            transport.clone(),
            batch.into_owned_roots(&records),
            Arc::downgrade(&daemon),
            None,
            None,
        );
        daemon.set_interface_mutation_bridge(Arc::new(owner.bridge.clone()));
        Self { transport, daemon, owner, failures }
    }

    async fn shutdown(&mut self) {
        timeout(Duration::from_secs(10), self.owner.shutdown())
            .await
            .expect("existing native shutdown deadline")
            .expect("owned Auto and native workers joined");
        assert!(self.transport.interface_hashes().await.is_empty());
    }

    fn runtime(&self) -> Value {
        let runtime = self.daemon.interface_records()[0].settings.as_ref().expect("settings")
            ["_runtime"]
            .clone();
        let text = runtime.to_string();
        for key in [INITIAL, ROTATED, FINAL] {
            assert!(!text.contains(key), "published Auto diagnostics expose credentials");
        }
        runtime
    }

    fn address(&self, index: usize) -> AddressHash {
        let record = &self.daemon.interface_records()[index];
        AddressHash::new_from_hex_string(
            record.settings.as_ref().expect("settings")["_runtime"]["iface"]
                .as_str()
                .expect("native address")
                .trim_matches('/'),
        )
        .expect("native hash")
    }
}

fn ports() -> (u16, u16) {
    let discovery = std::net::UdpSocket::bind("127.0.0.1:0").expect("reserve discovery endpoint");
    let data = std::net::UdpSocket::bind("127.0.0.1:0").expect("reserve data endpoint");
    (
        discovery.local_addr().expect("discovery endpoint").port(),
        data.local_addr().expect("data endpoint").port(),
    )
}

fn config(discovery: u16, data: u16, other: u16, credential: &str) -> DaemonConfig {
    DaemonConfig::from_toml(&format!("interfaces=[{{type='auto',enabled=true,name='auto-family',devices=['lo'],group_id='auto-family-group',discovery_scope='global',discovery_port={discovery},data_port={data},multicast_address_type='permanent',network_name='auto-family-network',passphrase='{credential}'}},{{type='tcp_client',enabled=true,name='unrelated',host='127.0.0.1',port={other}}}]\n[reticulum]\nshare_instance=false")).expect("configured Auto family")
}

fn plan(config: &DaemonConfig) -> AutoRuntimePlan {
    let mut plan = auto_plan_for_config(
        auto::auto_config(&config.interfaces[0]).expect("configured Auto parameters"),
    );
    plan.device_filter.allowed = vec!["lo".into()];
    assert!(plan.candidates.is_empty() && plan.adopted_devices.is_empty());
    assert!(plan.initial_peer_announce_datagrams().is_empty());
    assert_eq!(plan.startup_plan.initial_peering_wait, Duration::from_millis(1920));
    plan
}

async fn ready_gate(plan: &AutoRuntimePlan) {
    sleep(plan.startup_plan.initial_peering_wait).await;
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

fn packet(marker: u8) -> Packet {
    Packet {
        destination: AddressHash::new([marker; 16]),
        data: PacketDataBuffer::new_from_slice(&[marker, 0x7e, 0x7d, 0xc0, 0xdb]),
        ..Default::default()
    }
}

async fn unrelated(fixture: &Fixture, peer: &mut TcpStream, marker: u8) {
    let address = fixture.address(1);
    let mut observed = fixture.transport.iface_rx();
    let inbound = packet(marker);
    peer.write_all(
        &rns_transport::iface::hdlc::Hdlc::frame(&inbound.to_bytes().expect("plain packet"))
            .expect("HDLC"),
    )
    .await
    .expect("unrelated TCP ingress");
    let received = timeout(Duration::from_secs(2), observed.recv())
        .await
        .expect("unrelated ingress deadline")
        .expect("unrelated ingress");
    assert_eq!(received.address, address);
    assert_eq!(received.packet.destination, inbound.destination);
    assert_eq!(received.packet.data, inbound.data);
    let outbound = packet(marker + 1);
    let expected =
        rns_transport::iface::hdlc::Hdlc::frame(&outbound.to_bytes().expect("plain packet"))
            .expect("HDLC");
    assert_eq!(
        fixture
            .transport
            .iface_manager()
            .lock()
            .await
            .send(TxMessage { tx_type: TxMessageType::Direct(address), packet: outbound })
            .await
            .sent_ifaces,
        1
    );
    let mut wire = vec![0; expected.len()];
    timeout(Duration::from_secs(2), peer.read_exact(&mut wire))
        .await
        .expect("unrelated egress deadline")
        .expect("unrelated egress");
    assert_eq!(wire, expected);
}

struct Peer {
    listener: UdpSocket,
    sender: UdpSocket,
}

impl Peer {
    async fn bind(ip: &str, port: u16) -> Self {
        let listener = UdpSocket::bind((ip, port)).await.expect("owned configured-port receiver");
        let sender = UdpSocket::bind((ip, 0)).await.expect("owned ephemeral sender");
        assert_ne!(
            listener.local_addr().expect("listen endpoint").port(),
            sender.local_addr().expect("send endpoint").port()
        );
        Self { listener, sender }
    }

    async fn discover(
        &self,
        fixture: &Fixture,
        plan: &AutoRuntimePlan,
        existing: &[AddressHash],
    ) -> AddressHash {
        let source = self.sender.local_addr().expect("source endpoint");
        let token = rns_transport::iface::auto::peering_token(
            plan.config.group_id.as_bytes(),
            &source.ip().to_string(),
        );
        let target = &plan.startup_plan.discovery_listeners[0];
        assert_eq!(
            self.sender
                .send_to(&token, ("127.0.0.1", target.unicast_bind_port))
                .await
                .expect("one authenticated group-token input"),
            token.len()
        );
        let parent = fixture.address(0);
        timeout(Duration::from_secs(3), async {
            loop {
                if let Some(child) = fixture
                    .transport
                    .iface_manager()
                    .lock()
                    .await
                    .traffic_snapshots()
                    .into_iter()
                    .find(|child| {
                        child.parent == Some(parent) && !existing.contains(&child.address)
                    })
                {
                    break child.address;
                }
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("native Auto virtual child registration")
    }

    async fn authenticate(
        &self,
        fixture: &Fixture,
        plan: &AutoRuntimePlan,
        address: AddressHash,
        credential: &str,
        rejected: &str,
        marker: u8,
    ) -> Vec<u8> {
        let manager = fixture.transport.iface_manager();
        let mut observed = fixture.transport.iface_rx();
        let violations = || async {
            manager
                .lock()
                .await
                .traffic_snapshots()
                .into_iter()
                .find(|child| child.address == address)
                .expect("native child")
                .ifac_violations
        };
        let before = violations().await;
        let inbound = packet(marker);
        let target = ("127.0.0.1", plan.startup_plan.data_listeners[0].bind_port);
        for bytes in [wire(rejected, &inbound), inbound.to_bytes().expect("plaintext packet")] {
            assert_eq!(
                self.sender.send_to(&bytes, target).await.expect("negative native datagram"),
                bytes.len()
            );
        }
        timeout(Duration::from_secs(2), async {
            while violations().await != before + 2 {
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("both wrong-key/plaintext datagrams counted before admission");
        while let Ok(event) = observed.try_recv() {
            assert_ne!(event.packet.destination, inbound.destination, "negative native admission");
        }
        let bytes = wire(credential, &inbound);
        assert_eq!(bytes.len(), inbound.to_bytes().expect("plain packet").len() + 16);
        self.sender.send_to(&bytes, target).await.expect("authenticated native datagram");
        let received = timeout(Duration::from_secs(2), observed.recv())
            .await
            .expect("Auto ingress deadline")
            .expect("Auto ingress");
        assert_eq!(received.address, address);
        assert_eq!(received.packet.destination, inbound.destination);
        assert_eq!(received.packet.data, inbound.data);
        assert_eq!(received.packet.ifac.map(|ifac| ifac.length), Some(0));
        assert_eq!(
            received.source,
            rns_transport::iface::IfaceSource::Udp(
                self.sender.local_addr().expect("native source endpoint")
            )
        );
        let outbound = packet(marker + 1);
        let expected = wire(credential, &outbound);
        assert_eq!(
            manager
                .lock()
                .await
                .send(TxMessage { tx_type: TxMessageType::Direct(address), packet: outbound })
                .await
                .sent_ifaces,
            1
        );
        let mut buffer = [0; 2048];
        let (count, source) = timeout(Duration::from_secs(2), self.listener.recv_from(&mut buffer))
            .await
            .expect("Auto egress deadline")
            .expect("Auto egress at configured port");
        assert_eq!(&buffer[..count], expected);
        assert_eq!(source.ip(), std::net::Ipv4Addr::LOCALHOST);
        expected
    }

    async fn isolated(&self, forbidden: &[u8]) {
        let result = timeout(Duration::from_secs(2), async {
            let mut buffer = [0; 2048];
            loop {
                let (count, _) =
                    self.listener.recv_from(&mut buffer).await.expect("peer isolation observer");
                assert_ne!(&buffer[..count], forbidden, "direct traffic reached another peer");
            }
        })
        .await;
        assert!(result.is_err(), "observe full peer-isolation interval");
    }
}

fn wire(credential: &str, packet: &Packet) -> Vec<u8> {
    let state: IfacState = Arc::new(std::sync::RwLock::new(
        InterfaceSharedConfig {
            network_name: Some("auto-family-network".into()),
            passphrase: Some(credential.into()),
            ..Default::default()
        }
        .ifac_context_with_default_size(16)
        .expect("independent Auto IFAC peer"),
    ));
    encode_packet_ifac(&state, packet).expect("authenticated Auto datagram")
}

fn assert_released(plan: &AutoRuntimePlan) {
    let _discovery = std::net::UdpSocket::bind(("127.0.0.1", plan.config.discovery_port))
        .expect("owned discovery socket released");
    let _data = std::net::UdpSocket::bind(("127.0.0.1", plan.config.data_port))
        .expect("owned data socket released");
}

async fn published(fixture: &Fixture, delivered: u64) {
    timeout(Duration::from_secs(3), async {
        loop {
            let runtime = fixture.runtime();
            let status = &runtime["auto"]["carrier_runtime"];
            if status["peer_data_delivered_count"].as_u64().is_some_and(|count| count >= delivered)
            {
                assert_eq!(runtime["auto"]["candidate_devices"], serde_json::json!([]));
                assert_eq!(runtime["auto"]["adopted_devices"], serde_json::json!([]));
                // Native status seeds this count from the manually supplied data
                // listeners; it does not establish host enumeration/adoption.
                assert_eq!(status["adopted_device_count"], 1);
                assert_eq!(status["adopted_devices"][0]["ifname"], "lo");
                break;
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("native Auto status publication");
}

#[tokio::test]
async fn configured_auto_bind_failure_repairs_with_authenticated_restart() {
    if auto_lifecycle_runs_in_child() {
        return;
    }
    let (discovery, data) = ports();
    let blocker =
        std::net::UdpSocket::bind(("127.0.0.1", data)).expect("hold exact configured data port");
    let other = TcpListener::bind("127.0.0.1:0").await.expect("unrelated endpoint");
    let other_port = other.local_addr().expect("unrelated endpoint").port();
    let cfg = config(discovery, data, other_port, INITIAL);
    let native_plan = plan(&cfg);
    let mut failed = Fixture::start(&cfg, native_plan.clone()).await;
    assert_eq!(failed.failures.len(), 1);
    assert!(failed.failures[0].contains("AutoInterface discovery runtime startup failed"));
    let runtime = failed.runtime();
    assert_eq!(runtime["startup_status"], "failed");
    assert!(runtime["iface"].is_null());
    assert_eq!(
        failed.transport.interface_hashes().await.len(),
        1,
        "only unrelated TCP root remains"
    );
    {
        let _released = std::net::UdpSocket::bind(("127.0.0.1", discovery))
            .expect("partial activation released discovery sockets");
    }
    let mut unaffected = accept(&other).await;
    unrelated(&failed, &mut unaffected, 0x51).await;
    let old_identity = failed.transport.internal_identity().await;
    failed.shutdown().await;
    closed(&mut unaffected).await;
    drop(blocker);
    assert_released(&native_plan);

    let cfg = config(discovery, data, other_port, FINAL);
    let native_plan = plan(&cfg);
    let mut repaired = Fixture::start(&cfg, native_plan.clone()).await;
    assert!(repaired.failures.is_empty());
    assert_ne!(repaired.transport.internal_identity().await, old_identity);
    let mut unaffected = accept(&other).await;
    unrelated(&repaired, &mut unaffected, 0x54).await;
    ready_gate(&native_plan).await;
    let peer = Peer::bind("127.0.0.2", data).await;
    let child = peer.discover(&repaired, &native_plan, &[]).await;
    peer.authenticate(&repaired, &native_plan, child, FINAL, INITIAL, 0x57).await;
    published(&repaired, 1).await;
    repaired.shutdown().await;
    closed(&mut unaffected).await;
    assert_released(&native_plan);
}

#[tokio::test]
async fn configured_auto_virtual_children_rotate_policy_and_restart_on_owned_ports() {
    if auto_lifecycle_runs_in_child() {
        return;
    }
    let (discovery, data) = ports();
    let other = TcpListener::bind("127.0.0.1:0").await.expect("unrelated endpoint");
    let other_port = other.local_addr().expect("other endpoint").port();
    let cfg = config(discovery, data, other_port, INITIAL);
    let native_plan = plan(&cfg);
    let mut fixture = Fixture::start(&cfg, native_plan.clone()).await;
    assert!(fixture.failures.is_empty());
    let parent = fixture.address(0);
    let other_address = fixture.address(1);
    let mut unaffected = accept(&other).await;
    unrelated(&fixture, &mut unaffected, 0x61).await;
    ready_gate(&native_plan).await;
    let first = Peer::bind("127.0.0.2", data).await;
    let second = Peer::bind("127.0.0.3", data).await;
    let first_address = first.discover(&fixture, &native_plan, &[]).await;
    first.authenticate(&fixture, &native_plan, first_address, INITIAL, ROTATED, 0x64).await;
    let manager = fixture.transport.iface_manager();
    let unrelated_policy;
    {
        let mut manager = manager.lock().await;
        unrelated_policy = (
            manager.mode(&other_address),
            manager.outgoing(&other_address),
            manager.gravity(&other_address),
            manager.announce_pacing(&other_address),
        );
        let mut config = manager.shared_config(&parent).expect("prepared parent policy").clone();
        assert_eq!(config.ifac_size, None);
        config.passphrase = Some(ROTATED.into());
        assert!(manager.try_set_shared_config(parent, config).expect("native live key rotation"));
        assert!(manager.set_mode(parent, InterfaceMode::Gateway));
        assert!(manager.set_outgoing(parent, false));
        assert!(manager.set_gravity(parent, 7));
        assert!(manager.set_announce_pacing(parent, 9600, 23));
    }
    let second_address = second.discover(&fixture, &native_plan, &[first_address]).await;
    {
        let mut manager = manager.lock().await;
        for address in [parent, first_address, second_address] {
            assert_eq!(manager.mode(&address), Some(InterfaceMode::Gateway));
            assert_eq!(manager.outgoing(&address), Some(false));
            assert_eq!(manager.gravity(&address), Some(7));
            assert_eq!(manager.announce_pacing(&address), Some((9600, 23)));
            assert_eq!(
                manager
                    .shared_config(&address)
                    .expect("native shared policy")
                    .passphrase
                    .as_deref(),
                Some(ROTATED)
            );
        }
        assert_eq!(
            (
                manager.mode(&other_address),
                manager.outgoing(&other_address),
                manager.gravity(&other_address),
                manager.announce_pacing(&other_address)
            ),
            unrelated_policy
        );
        assert!(manager.set_outgoing(parent, true));
    }
    let expected =
        first.authenticate(&fixture, &native_plan, first_address, ROTATED, INITIAL, 0x67).await;
    second.isolated(&expected).await;
    let expected =
        second.authenticate(&fixture, &native_plan, second_address, ROTATED, INITIAL, 0x6a).await;
    first.isolated(&expected).await;
    assert_eq!(fixture.address(0), parent);
    assert_eq!(fixture.address(1), other_address);
    unrelated(&fixture, &mut unaffected, 0x6d).await;
    published(&fixture, 3).await;
    let old_identity = fixture.transport.internal_identity().await;
    fixture.shutdown().await;
    closed(&mut unaffected).await;
    assert_released(&native_plan);
    drop((first, second));

    let cfg = config(discovery, data, other_port, FINAL);
    let native_plan = plan(&cfg);
    let mut restarted = Fixture::start(&cfg, native_plan.clone()).await;
    assert!(restarted.failures.is_empty());
    assert_ne!(restarted.transport.internal_identity().await, old_identity);
    let mut unaffected = accept(&other).await;
    unrelated(&restarted, &mut unaffected, 0x70).await;
    ready_gate(&native_plan).await;
    let peer = Peer::bind("127.0.0.2", data).await;
    let address = peer.discover(&restarted, &native_plan, &[]).await;
    peer.authenticate(&restarted, &native_plan, address, FINAL, ROTATED, 0x73).await;
    published(&restarted, 1).await;
    restarted.shutdown().await;
    closed(&mut unaffected).await;
    assert_released(&native_plan);
}
