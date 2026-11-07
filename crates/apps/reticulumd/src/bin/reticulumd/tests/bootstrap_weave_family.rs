use super::pty_peer::PtyPeer;
use super::restore_tests::{native_address, verify_unrelated_tcp};
use super::*;
use crate::interface_hot_apply::{InterfaceHotApplyBridge, InterfaceMutationOwner};
use rns_transport::iface::hdlc::Hdlc;
use rns_transport::iface::weave::WeaveManagementHandle;
use rns_transport::iface::{
    encode_packet_ifac, IfacState, InterfaceMode, InterfaceSharedConfig, TxMessage, TxMessageType,
};
use rns_transport::packet::{Packet, PacketDataBuffer};
use serde_json::{json, Value};
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{sleep, timeout, Duration};

const INITIAL: &str = "weave-family-initial-credential";
const ROTATED: &str = "weave-family-rotated-credential";
const FINAL: &str = "weave-family-final-credential";
const ENDPOINT_A: [u8; 8] = [0xa1; 8];
const ENDPOINT_B: [u8; 8] = [0xb2; 8];

fn assert_redacted(value: &impl serde::Serialize) {
    let text = serde_json::to_string(value).expect("diagnostic JSON");
    for credential in [INITIAL, ROTATED, FINAL] {
        assert!(!text.contains(credential), "runtime diagnostics cannot expose credentials");
    }
}

fn config(alias: &Path, credential: &str, mtu: Option<usize>, other: Option<u16>) -> DaemonConfig {
    let mtu = mtu.map(|value| format!(",mtu={value}")).unwrap_or_default();
    let other = other
        .map(|port| {
            format!(
                ",{{type='tcp_client',enabled=true,name='unrelated',host='127.0.0.1',port={port}}}"
            )
        })
        .unwrap_or_default();
    DaemonConfig::from_toml(&format!(
        "interfaces=[{{type='weave',enabled=true,name='weave-family',device={},network_name='weave-family-network',passphrase='{credential}'{mtu}}}{other}]\n[reticulum]\nshare_instance=false",
        json!(alias.to_str().expect("owned alias"))
    )).expect("configured Weave with native baud/default IFAC")
}

struct Fixture {
    transport: Arc<Transport>,
    daemon: Arc<rns_rpc::RpcDaemon>,
    owner: InterfaceMutationOwner,
    control: WeaveManagementHandle,
}

impl Fixture {
    async fn start(config: &DaemonConfig, root: &Path) -> Self {
        let startup = start_fixture(&Args::parse_from(["reticulumd"]), config, root).await;
        assert!(startup.interfaces.startup_failures.is_empty());
        assert_eq!(startup.interfaces.weave_control_bindings.len(), 1);
        let control = startup.interfaces.weave_control_bindings[0].handle.clone();
        let transport = startup.transport.expect("native transport");
        let daemon = Arc::new(rns_rpc::RpcDaemon::test_instance());
        daemon.replace_interfaces(startup.configured_interfaces.clone());
        let owner = InterfaceHotApplyBridge::spawn_with_configured_roots(
            transport.clone(),
            startup.interfaces.into_owned_roots(&startup.configured_interfaces),
            Arc::downgrade(&daemon),
            None,
            startup.configured_startup,
        );
        daemon.set_interface_mutation_bridge(Arc::new(owner.bridge.clone()));
        Self { transport, daemon, owner, control }
    }

    async fn status(&self, ready: impl Fn(&Value) -> bool) -> Value {
        timeout(Duration::from_secs(3), async {
            loop {
                let record = &self.daemon.interface_records()[0];
                let runtime = &record.settings.as_ref().expect("settings")["_runtime"];
                let status = &runtime["weave"]["status"];
                if ready(status) {
                    assert_redacted(runtime);
                    break status.clone();
                }
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("published native Weave state deadline")
    }

    async fn shutdown(&mut self) {
        timeout(Duration::from_secs(10), self.owner.shutdown())
            .await
            .expect("existing native shutdown bound")
            .expect("native and reconnect workers joined");
        assert!(self.transport.interface_hashes().await.is_empty());
        assert!(self.control.try_set_remote_display(Some([0x44; 4]), true).is_err());
        assert!(display_rpc(&self.daemon, true).error.is_some(), "control binding retired");
    }
}

fn display_rpc(daemon: &rns_rpc::RpcDaemon, enable: bool) -> rns_rpc::RpcResponse {
    daemon
        .handle_rpc(rns_rpc::RpcRequest {
            id: 1,
            method: "weave_remote_display_control".into(),
            params: Some(json!({"iface":"weave-family","enable":enable})),
        })
        .expect("remote-display RPC response")
}

async fn accept(listener: &TcpListener) -> TcpStream {
    timeout(Duration::from_secs(3), listener.accept())
        .await
        .expect("unrelated connection deadline")
        .expect("unrelated connection")
        .0
}

async fn closed(peer: &mut TcpStream) {
    timeout(Duration::from_secs(2), peer.read_to_end(&mut Vec::new()))
        .await
        .expect("retired unrelated peer EOF deadline")
        .expect("retired unrelated peer EOF");
}

fn wdcl(target: [u8; 4], kind: u8, payload: &[u8]) -> Vec<u8> {
    let mut frame = target.to_vec();
    frame.push(kind);
    frame.extend_from_slice(payload);
    frame
}

async fn write_frame(peer: &mut PtyPeer, payload: &[u8]) {
    timeout(Duration::from_secs(2), peer.input.write_all(&Hdlc::frame(payload).expect("HDLC")))
        .await
        .expect("owned PTY write deadline")
        .expect("owned PTY write");
}

// Independent byte-level HDLC observer: bounded, with no reads beyond one frame.
async fn read_frame(peer: &mut PtyPeer) -> Vec<u8> {
    let mut frame = Vec::new();
    let mut escaped = false;
    loop {
        let byte = peer.output.read_u8().await.expect("owned PTY frame before EOF");
        match byte {
            0x7e if !frame.is_empty() => return frame,
            0x7e => escaped = false,
            0x7d if !escaped => escaped = true,
            value => {
                frame.push(if escaped { value ^ 0x20 } else { value });
                escaped = false;
                assert!(frame.len() <= 8192, "bounded WDCL frame");
            }
        }
    }
}

async fn next_frame(peer: &mut PtyPeer, seconds: u64) -> Vec<u8> {
    timeout(Duration::from_secs(seconds), read_frame(peer)).await.expect("WDCL frame deadline")
}

async fn log_event(peer: &mut PtyPeer, local: [u8; 4], event: u16, data: &[u8]) {
    let mut payload = vec![0; 6];
    payload.extend_from_slice(&event.to_be_bytes());
    payload.extend_from_slice(data);
    write_frame(peer, &wdcl(local, 3, &payload)).await;
}

async fn handshake(
    fixture: &Fixture,
    peer: &mut PtyPeer,
    discovery_seconds: u64,
) -> ([u8; 4], [u8; 4]) {
    let discover = next_frame(peer, discovery_seconds).await;
    assert_eq!(discover.len(), 9);
    assert_eq!(&discover[..5], &[0xff, 0xff, 0xff, 0xff, 0]);
    let local: [u8; 4] = discover[5..].try_into().expect("local switch ID");
    let remote = PrivateIdentity::new_from_name("owned-weave-software-peer");
    let remote_public = remote.as_identity().verifying_key_bytes();
    let remote_id: [u8; 4] = remote_public[28..].try_into().expect("last four public-key bytes");
    let mut response = remote_public.to_vec();
    response.extend_from_slice(&remote.sign(&local).to_bytes());
    write_frame(peer, &wdcl(local, 0, &response)).await;
    let connect = next_frame(peer, 2).await;
    assert_eq!(connect.len(), 101);
    assert_eq!(&connect[..5], &wdcl(remote_id, 1, &[]));
    let local_public: [u8; 32] = connect[5..37].try_into().expect("local verifying key");
    assert_eq!(&local_public[28..], &local);
    assert!(rns_transport::identity::verify(local_public, &remote_id, &connect[37..]));
    log_event(peer, local, 0x3002, &[]).await;
    let connected =
        fixture.status(|s| s["link_state"] == "connected" && s["wdcl_connected"] == true).await;
    assert_eq!(connected["remote_switch_id"], hex::encode(remote_id));
    assert_eq!(connected["baud_rate"], 3_000_000);
    assert!(connected["last_error"].is_null());
    (local, remote_id)
}

async fn endpoint(
    fixture: &Fixture,
    peer: &mut PtyPeer,
    local: [u8; 4],
    id: [u8; 8],
) -> AddressHash {
    log_event(peer, local, 0x3102, &id).await;
    let key = hex::encode(id);
    let status = fixture
        .status(|s| {
            s["endpoints"]
                .as_array()
                .is_some_and(|entries| entries.iter().any(|e| e["endpoint_id"] == key))
        })
        .await;
    let entry = status["endpoints"]
        .as_array()
        .expect("endpoints")
        .iter()
        .find(|e| e["endpoint_id"] == key)
        .expect("announced endpoint");
    AddressHash::new_from_hex_string(
        entry["iface"].as_str().expect("native endpoint hash").trim_matches('/'),
    )
    .expect("endpoint hash")
}

fn auth(credential: &str) -> IfacState {
    Arc::new(std::sync::RwLock::new(
        InterfaceSharedConfig {
            network_name: Some("weave-family-network".into()),
            passphrase: Some(credential.into()),
            ..Default::default()
        }
        .ifac_context_with_default_size(16)
        .expect("independent native sixteen-byte default"),
    ))
}

async fn policy(
    fixture: &Fixture,
    addresses: &[AddressHash],
    key: &str,
    mtu: usize,
    rotated: bool,
) {
    let manager = fixture.transport.iface_manager();
    let manager = manager.lock().await;
    for address in addresses {
        assert_eq!(manager.mtu(address), Some(mtu));
        let shared = manager.shared_config(address).expect("native inherited policy");
        assert_eq!(shared.passphrase.as_deref(), Some(key));
        assert_eq!(shared.ifac_size, None, "native IFAC default remains authoritative");
        if rotated {
            assert_eq!(manager.mode(address), Some(InterfaceMode::Gateway));
            assert_eq!(manager.outgoing(address), Some(false));
            assert_eq!(manager.gravity(address), Some(7));
            assert_eq!(manager.announce_pacing(address), Some((9600, 23)));
        }
    }
}

async fn rotate(fixture: &Fixture, root: AddressHash) {
    let manager = fixture.transport.iface_manager();
    let mut manager = manager.lock().await;
    let mut shared = manager.shared_config(&root).expect("native policy").clone();
    shared.passphrase = Some(ROTATED.into());
    assert!(manager.try_set_shared_config(root, shared).expect("live IFAC rotation"));
    assert!(manager.set_mode(root, InterfaceMode::Gateway));
    assert!(manager.set_outgoing(root, false));
    assert!(manager.set_gravity(root, 7));
    assert!(manager.set_announce_pacing(root, 9600, 23));
}

async fn authenticate(
    fixture: &Fixture,
    peer: &mut PtyPeer,
    switches: ([u8; 4], [u8; 4]),
    endpoint: ([u8; 8], AddressHash),
    keys: (&str, &str),
    marker: u8,
) {
    let root = native_address(&fixture.daemon.interface_records()[0]);
    let manager = fixture.transport.iface_manager();
    let mut ingress = fixture.transport.iface_rx();
    let packet = Packet {
        destination: AddressHash::new([marker; 16]),
        data: PacketDataBuffer::new_from_slice(&[marker, 0x7e, 0x7d, 0xc0, 0xdb]),
        ..Default::default()
    };
    let violations = || async {
        manager
            .lock()
            .await
            .traffic_snapshots()
            .into_iter()
            .find(|e| e.address == root)
            .expect("native root")
            .ifac_violations
    };
    let before = violations().await;
    for mut payload in [
        encode_packet_ifac(&auth(keys.1), &packet).expect("wrong-key packet"),
        packet.to_bytes().expect("plaintext packet"),
    ] {
        payload.extend_from_slice(&endpoint.0);
        write_frame(peer, &wdcl(switches.0, 5, &payload)).await;
    }
    timeout(Duration::from_secs(2), async {
        while violations().await != before + 2 {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("both negative native frames counted before good ingress");
    while let Ok(event) = ingress.try_recv() {
        assert_ne!(event.packet.destination, packet.destination, "negative native admission");
    }
    let state = auth(keys.0);
    let mut payload = encode_packet_ifac(&state, &packet).expect("authenticated ingress");
    assert_eq!(payload.len(), packet.to_bytes().expect("plain bytes").len() + 16);
    payload.extend_from_slice(&endpoint.0);
    write_frame(peer, &wdcl(switches.0, 5, &payload)).await;
    let received = timeout(Duration::from_secs(2), ingress.recv())
        .await
        .expect("native ingress deadline")
        .expect("native ingress");
    assert_eq!(received.address, endpoint.1);
    assert_eq!(received.packet.destination, packet.destination);
    assert_eq!(received.packet.data, packet.data);
    assert_eq!(received.packet.ifac.map(|tag| tag.length), Some(0));
    let outbound = Packet { destination: AddressHash::new([marker + 1; 16]), ..packet };
    let mut expected = vec![0, 1];
    expected.extend_from_slice(&endpoint.0);
    expected
        .extend_from_slice(&encode_packet_ifac(&state, &outbound).expect("authenticated egress"));
    assert_eq!(
        manager
            .lock()
            .await
            .send(TxMessage { tx_type: TxMessageType::Direct(endpoint.1), packet: outbound })
            .await
            .sent_ifaces,
        1
    );
    assert_eq!(
        next_frame(peer, 2).await,
        wdcl(switches.1, 2, &expected),
        "exact target endpoint and IFAC output"
    );
    assert!(
        timeout(Duration::from_secs(2), peer.output.read_u8()).await.is_err(),
        "direct reply cannot emit any extra bytes or close the peer"
    );
}

async fn display_command(fixture: &Fixture, peer: &mut PtyPeer, remote: [u8; 4], enable: bool) {
    let response = display_rpc(&fixture.daemon, enable);
    assert!(response.error.is_none(), "{response:?}");
    let result = response.result.expect("queued control");
    assert_redacted(&result);
    assert_eq!(result["queued"], true);
    assert_eq!(result["remote_switch_id_source"], "learned");
    assert_eq!(
        next_frame(peer, 2).await,
        wdcl(remote, 2, &[0x0a, 0, u8::from(enable)]),
        "remote display command observed on actual owned PTY"
    );
}

#[tokio::test]
async fn configured_weave_strict_missing_owned_device_rejects_root_and_control() {
    let root = tempfile::tempdir().expect("owned state");
    let alias = root.path().join("missing-owned-weave");
    let config = config(&alias, INITIAL, None, None);
    let args = Args::parse_from(["reticulumd", "--strict-interface-startup"]);
    let startup = start_fixture(&args, &config, root.path()).await;
    assert_eq!(startup.interfaces.startup_successes, 0);
    assert_eq!(startup.interfaces.startup_failures.len(), 1);
    let failure = &startup.interfaces.startup_failures[0];
    assert_eq!(failure.label, "weave-family");
    assert!(failure.error.contains("weave preflight open failed"));
    assert!(failure.error.contains(alias.to_str().expect("alias")));
    assert_redacted(&failure.error);
    assert!(startup.interfaces.configured_roots.is_empty());
    assert!(startup.interfaces.weave_runtime_refreshes.is_empty());
    assert!(startup.interfaces.weave_control_bindings.is_empty());
    let runtime =
        &startup.configured_interfaces[0].settings.as_ref().expect("settings")["_runtime"];
    assert_eq!(runtime["startup_status"], "failed");
    assert_redacted(runtime);
    retire_fixture(startup).await;
    assert!(!alias.exists());
}

#[tokio::test]
async fn configured_weave_pty_policy_reconnect_control_and_restart() {
    let root = tempfile::tempdir().expect("owned state");
    let alias = root.path().join("owned-weave");
    let other = TcpListener::bind("127.0.0.1:0").await.expect("unrelated peer");
    let port = other.local_addr().expect("unrelated endpoint").port();
    let mut fixture = Fixture::start(&config(&alias, INITIAL, None, Some(port)), root.path()).await;
    let initial_identity = fixture.transport.internal_identity().await;
    let parent = native_address(&fixture.daemon.interface_records()[0]);
    let unrelated = native_address(&fixture.daemon.interface_records()[1]);
    let mut unaffected = accept(&other).await;
    let failed =
        fixture.status(|s| s["link_state"] == "reconnecting" && s["last_error"].is_string()).await;
    assert_eq!(failed["wdcl_connected"], false);
    assert_eq!(failed["endpoint_count"], 0);
    assert_eq!(failed["frames_rx"], 0);
    assert_eq!(failed["frames_tx"], 0);
    assert_eq!(failed["mtu"], 1024);
    assert_eq!(fixture.transport.iface_manager().lock().await.online(&parent), Some(false));
    policy(&fixture, &[parent], INITIAL, 1024, false).await;
    verify_unrelated_tcp(&fixture.transport, unrelated, &mut unaffected, 0x11).await;

    let mut peer = PtyPeer::start().await;
    peer.publish_alias(&alias);
    // Eight seconds includes the unchanged five-second native open retry.
    let switches = handshake(&fixture, &mut peer, 8).await;
    let first = endpoint(&fixture, &mut peer, switches.0, ENDPOINT_A).await;
    policy(&fixture, &[parent, first], INITIAL, 1024, false).await;
    authenticate(&fixture, &mut peer, switches, (ENDPOINT_A, first), (INITIAL, ROTATED), 0x21)
        .await;
    display_command(&fixture, &mut peer, switches.1, true).await;

    rotate(&fixture, parent).await;
    let second = endpoint(&fixture, &mut peer, switches.0, ENDPOINT_B).await;
    assert_ne!(first, second);
    policy(&fixture, &[parent, first, second], ROTATED, 1024, true).await;
    assert!(fixture.transport.iface_manager().lock().await.set_outgoing(parent, true));
    authenticate(&fixture, &mut peer, switches, (ENDPOINT_A, first), (ROTATED, INITIAL), 0x31)
        .await;
    authenticate(&fixture, &mut peer, switches, (ENDPOINT_B, second), (ROTATED, INITIAL), 0x41)
        .await;
    assert!(fixture.transport.iface_manager().lock().await.set_outgoing(parent, false));
    verify_unrelated_tcp(&fixture.transport, unrelated, &mut unaffected, 0x51).await;

    // Remove the alias before closing the master: never reopen a recycled /dev/pts path.
    std::fs::remove_file(&alias).expect("retire only owned alias");
    peer.stop().await;
    let failed =
        fixture.status(|s| s["link_state"] == "reconnecting" && s["endpoint_count"] == 0).await;
    assert_eq!(failed["wdcl_connected"], false);
    assert!(failed["last_error"].is_string());
    assert!(failed["frames_tx"].as_u64().expect("retained counter") > 2);
    {
        let hashes = fixture.transport.interface_hashes().await;
        assert!(hashes.contains(&parent));
        assert!(hashes.contains(&unrelated));
        assert!(!hashes.contains(&first));
        assert!(!hashes.contains(&second));
    }
    verify_unrelated_tcp(&fixture.transport, unrelated, &mut unaffected, 0x61).await;
    let mut repaired = PtyPeer::start().await;
    repaired.publish_alias(&alias);
    let switches = handshake(&fixture, &mut repaired, 8).await;
    assert_eq!(native_address(&fixture.daemon.interface_records()[0]), parent);
    let replacement = endpoint(&fixture, &mut repaired, switches.0, ENDPOINT_A).await;
    assert_ne!(replacement, first);
    policy(&fixture, &[parent, replacement], ROTATED, 1024, true).await;
    assert!(fixture.transport.iface_manager().lock().await.set_outgoing(parent, true));
    authenticate(
        &fixture,
        &mut repaired,
        switches,
        (ENDPOINT_A, replacement),
        (ROTATED, INITIAL),
        0x71,
    )
    .await;
    display_command(&fixture, &mut repaired, switches.1, false).await;
    verify_unrelated_tcp(&fixture.transport, unrelated, &mut unaffected, 0x81).await;
    std::fs::remove_file(&alias).expect("retire repaired alias");
    fixture.shutdown().await;
    repaired.stop().await;
    closed(&mut unaffected).await;

    let mut restarted_peer = PtyPeer::start().await;
    restarted_peer.publish_alias(&alias);
    let mut restarted =
        Fixture::start(&config(&alias, FINAL, Some(512), Some(port)), root.path()).await;
    assert_ne!(restarted.transport.internal_identity().await, initial_identity);
    let restarted_parent = native_address(&restarted.daemon.interface_records()[0]);
    let unrelated = native_address(&restarted.daemon.interface_records()[1]);
    let mut unaffected = accept(&other).await;
    // A newly opened native worker has emitted only its initial discovery.
    let fresh = restarted.status(|s| s["link_state"] == "discovering").await;
    assert_eq!(fresh["mtu"], 512);
    assert_eq!(fresh["frames_tx"], 1);
    assert_eq!(fresh["frames_rx"], 0);
    assert_eq!(fresh["endpoint_count"], 0);
    assert!(fresh["remote_switch_id"].is_null());
    let switches = handshake(&restarted, &mut restarted_peer, 2).await;
    let endpoint = endpoint(&restarted, &mut restarted_peer, switches.0, ENDPOINT_B).await;
    policy(&restarted, &[restarted_parent, endpoint], FINAL, 512, false).await;
    authenticate(
        &restarted,
        &mut restarted_peer,
        switches,
        (ENDPOINT_B, endpoint),
        (FINAL, ROTATED),
        0x91,
    )
    .await;
    display_command(&restarted, &mut restarted_peer, switches.1, true).await;
    verify_unrelated_tcp(&restarted.transport, unrelated, &mut unaffected, 0xa1).await;
    std::fs::remove_file(&alias).expect("retire restarted alias");
    restarted.shutdown().await;
    restarted_peer.stop().await;
    closed(&mut unaffected).await;
}
