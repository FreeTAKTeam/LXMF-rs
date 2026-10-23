use super::pty_peer::PtyPeer;
use super::restore_tests::{native_address, verify_unrelated_tcp};
use super::*;
use crate::bridge_rnode_management::DaemonRNodeManagementHandle;
use crate::interface_hot_apply::{InterfaceHotApplyBridge, InterfaceMutationOwner};
use rns_transport::iface::lora::LoraRNodeManagementHandle;
use rns_transport::iface::{
    encode_packet_ifac, IfacState, InterfaceSharedConfig, TxMessage, TxMessageType,
};
use rns_transport::packet::{Packet, PacketDataBuffer};
use serde_json::{json, Value};
use std::path::Path;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpSocket, TcpStream};
use tokio::time::{sleep, timeout, Duration};

const INITIAL: &str = "lora-family-initial-credential";
const ROTATED: &str = "lora-family-rotated-credential";
const FINAL: &str = "lora-family-final-credential";

fn assert_redacted(value: &impl serde::Serialize) {
    let text = serde_json::to_string(value).expect("diagnostic JSON");
    for credential in [INITIAL, ROTATED, FINAL] {
        assert!(!text.contains(credential), "runtime diagnostics cannot expose credentials");
    }
}

fn config(
    device: &str,
    state: Option<&Path>,
    other: u16,
    key: &str,
    mtu: Option<u16>,
) -> DaemonConfig {
    let kind = if state.is_some() { "lora" } else { "RNodeInterface" };
    let state = state
        .map(|path| format!(",state_path={}", json!(path.to_str().expect("owned state path"))))
        .unwrap_or_default();
    let mtu = mtu.map(|value| format!(",max_payload_bytes={value}")).unwrap_or_default();
    let (retry, max_retry) = if kind == "lora" { (50, 100) } else { (2500, 2500) };
    DaemonConfig::from_toml(&format!(
        "interfaces=[{{type='{kind}',enabled=true,name='lora-family',device={},baud_rate=115200,region='US915',frequency_hz=915000000,bandwidth_hz=125000,spreading_factor=9,coding_rate='4/5',tx_power_dbm=17,reconnect_backoff_ms={retry},max_reconnect_backoff_ms={max_retry},network_name='lora-family-network',passphrase='{key}'{state}{mtu}}},{{type='tcp_client',enabled=true,name='unrelated',host='127.0.0.1',port={other}}}]\n[reticulum]\nshare_instance=false",
        json!(device)
    )).expect("configured ordinary LoRa or Python RNode profile")
}

struct Fixture {
    transport: Arc<Transport>,
    daemon: Arc<rns_rpc::RpcDaemon>,
    owner: InterfaceMutationOwner,
    native: LoraRuntimeStatusSource,
    control: LoraRNodeManagementHandle,
}

impl Fixture {
    async fn start(config: &DaemonConfig, root: &Path) -> Self {
        let startup = start_fixture(&Args::parse_from(["reticulumd"]), config, root).await;
        assert!(startup.interfaces.startup_failures.is_empty());
        assert_eq!(startup.interfaces.lora_runtime_refreshes.len(), 1);
        assert_eq!(startup.interfaces.rnode_management_bindings.len(), 1);
        let native = startup.interfaces.lora_runtime_refreshes[0].status.clone();
        let control = match &startup.interfaces.rnode_management_bindings[0].handle {
            DaemonRNodeManagementHandle::Lora(handle) => handle.clone(),
            _ => panic!("configured LoRa must retain its native control owner"),
        };
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
        Self { transport, daemon, owner, native, control }
    }

    fn address(&self) -> AddressHash {
        native_address(&self.daemon.interface_records()[0])
    }

    async fn status(&self, published: bool, ready: impl Fn(&Value) -> bool) -> Value {
        timeout(Duration::from_secs(3), async {
            loop {
                let status = if published {
                    let record = &self.daemon.interface_records()[0];
                    let runtime = &record.settings.as_ref().expect("settings")["_runtime"];
                    assert_redacted(runtime);
                    runtime["lora"]["rnode_status"].clone()
                } else {
                    self.native.to_json()
                };
                if ready(&status) {
                    return status;
                }
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("native or published LoRa status deadline")
    }

    async fn failed(&self, error: &str) {
        self.status(false, |s| {
            s["online"] == false
                && s["last_command_error"].as_str().is_some_and(|e| e.contains(error))
        })
        .await;
        self.status(true, |s| {
            s["online"] == false
                && s["last_command_error"].as_str().is_some_and(|e| e.contains(error))
        })
        .await;
        assert_eq!(
            self.transport.iface_manager().lock().await.online(&self.address()),
            Some(false)
        );
    }

    async fn radio_ready(&self, bearer: &str, mtu: u16) {
        let ready = |s: &Value| {
            s["online"] == true
                && s["probe_status"]["detected"] == true
                && s["probe_status"]["firmware_version"]["label"] == "1.83"
                && s["probe_status"]["platform"] == 0x80
                && s["probe_status"]["mcu"] == 1
                && s["radio_status"]["frequency_hz"] == 915000000_u32
                && s["radio_status"]["bandwidth_hz"] == 125000_u32
                && s["radio_status"]["tx_power_dbm"] == 17
                && s["radio_status"]["spreading_factor"] == 9
                && s["radio_status"]["coding_rate"] == 5
                && s["radio_status"]["radio_state"] == 1
                && s["last_command_error"].is_null()
        };
        self.status(false, ready).await;
        let status = self.status(true, ready).await;
        assert_eq!(status["bearer"], bearer);
        assert_eq!(status["configured"]["max_payload_bytes"], mtu);
        assert_eq!(status["hardware_errors"], json!([]));
        assert_eq!(self.transport.iface_manager().lock().await.online(&self.address()), Some(true));
    }

    async fn shutdown(&mut self) {
        timeout(Duration::from_secs(10), self.owner.shutdown())
            .await
            .expect("native shutdown bound")
            .expect("all native and reconnect workers joined");
        assert!(self.transport.interface_hashes().await.is_empty());
        assert!(self.control.try_query_radio_state().is_err(), "retired native receiver closed");
        assert!(query(&self.daemon).error.is_some(), "retired RPC control binding removed");
        assert_eq!(self.native.to_json()["online"], false);
    }
}

fn query(daemon: &rns_rpc::RpcDaemon) -> rns_rpc::RpcResponse {
    daemon
        .handle_rpc(rns_rpc::RpcRequest {
            id: 1,
            method: "rnode_management".into(),
            params: Some(json!({"iface":"lora-family","command":"radio_state_query"})),
        })
        .expect("RNode RPC response")
}

async fn accept(listener: &TcpListener) -> TcpStream {
    timeout(Duration::from_secs(3), listener.accept())
        .await
        .expect("connection deadline")
        .expect("owned connection")
        .0
}

async fn closed<R: AsyncRead + Unpin>(reader: &mut R) {
    timeout(Duration::from_secs(2), reader.read_to_end(&mut Vec::new()))
        .await
        .expect("retired peer EOF deadline")
        .expect("retired peer EOF");
}

// Byte framing/command values are independently observed against both pinned Python files.
fn kiss(command: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0xc0];
    for byte in std::iter::once(command).chain(payload.iter().copied()) {
        match byte {
            0xc0 => bytes.extend([0xdb, 0xdc]),
            0xdb => bytes.extend([0xdb, 0xdd]),
            other => bytes.push(other),
        }
    }
    bytes.push(0xc0);
    bytes
}

async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> Vec<u8> {
    let mut frame = Vec::new();
    loop {
        match reader.read_u8().await.expect("KISS byte before EOF") {
            0xc0 if !frame.is_empty() => return frame,
            0xc0 => {}
            0xdb => frame.push(match reader.read_u8().await.expect("KISS escape byte") {
                0xdc => 0xc0,
                0xdd => 0xdb,
                other => panic!("invalid KISS escape {other:#04x}"),
            }),
            byte => frame.push(byte),
        }
        assert!(frame.len() <= 4096, "bounded software KISS frame");
    }
}

async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, command: u8, payload: &[u8]) {
    timeout(Duration::from_secs(2), writer.write_all(&kiss(command, payload)))
        .await
        .expect("owned KISS input deadline")
        .expect("owned KISS input");
}

async fn startup<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    reader: &mut R,
    writer: &mut W,
    valid: bool,
) {
    let commands = [
        vec![0x08, 0x73],
        vec![0x50, 0],
        vec![0x48, 0],
        vec![0x49, 0],
        [&[0x01][..], &915000000_u32.to_be_bytes()].concat(),
        [&[0x02][..], &125000_u32.to_be_bytes()].concat(),
        vec![0x03, 17],
        vec![0x04, 9],
        vec![0x05, 5],
        vec![0x06, 1],
    ];
    timeout(Duration::from_secs(2), async {
        for expected in commands {
            assert_eq!(read_frame(reader).await, expected, "exact probe/radio command");
        }
        let replies = [
            (0x08, vec![0x46]),
            (0x50, if valid { vec![1, 83] } else { vec![1, 0] }),
            (0x48, vec![0x80]),
            (0x49, vec![1]),
            (0x01, 915000000_u32.to_be_bytes().to_vec()),
            (0x02, 125000_u32.to_be_bytes().to_vec()),
            (0x03, vec![17]),
            (0x04, vec![9]),
            (0x05, vec![5]),
            (0x06, vec![1]),
        ];
        for (command, payload) in replies {
            writer.write_all(&kiss(command, &payload)).await.expect("owned radio replies");
        }
        writer.flush().await.expect("owned radio replies flushed");
    })
    .await
    .expect("complete probe/radio handshake deadline");
}

fn auth(key: &str) -> IfacState {
    Arc::new(std::sync::RwLock::new(
        InterfaceSharedConfig {
            network_name: Some("lora-family-network".into()),
            passphrase: Some(key.into()),
            ..Default::default()
        }
        .ifac_context_with_default_size(8)
        .expect("peer eight-byte IFAC context"),
    ))
}

async fn policy(fixture: &Fixture, key: &str, mtu: usize) {
    let manager = fixture.transport.iface_manager();
    let manager = manager.lock().await;
    assert_eq!(manager.mtu(&fixture.address()), Some(mtu));
    assert_eq!(
        manager.shared_config(&fixture.address()).expect("prepared policy").passphrase.as_deref(),
        Some(key)
    );
}

async fn rotate(fixture: &Fixture) {
    let manager = fixture.transport.iface_manager();
    let mut manager = manager.lock().await;
    let mut shared = manager.shared_config(&fixture.address()).expect("live policy").clone();
    shared.passphrase = Some(ROTATED.into());
    assert!(manager.try_set_shared_config(fixture.address(), shared).expect("live IFAC rotation"));
}

async fn expect_frame<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    reader: &mut R,
    writer: &mut W,
    expected: &[u8],
) {
    timeout(Duration::from_secs(2), async {
        loop {
            let frame = read_frame(reader).await;
            if frame == expected {
                return;
            }
            assert_eq!(
                frame,
                [0x08, 0x73],
                "only an existing activity probe may precede target output"
            );
            writer.write_all(&kiss(0x08, &[0x46])).await.expect("owned activity reply");
        }
    })
    .await
    .expect("exact software KISS output deadline");
}

async fn authenticate<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    fixture: &Fixture,
    reader: &mut R,
    writer: &mut W,
    key: &str,
    rejected: &str,
    marker: u8,
) {
    let packet = Packet {
        destination: AddressHash::new([marker; 16]),
        data: PacketDataBuffer::new_from_slice(&[marker, 0xc0, 0xdb, 0x7e, 0x7d]),
        ..Default::default()
    };
    let manager = fixture.transport.iface_manager();
    let mut ingress = fixture.transport.iface_rx();
    let violations = || async {
        manager
            .lock()
            .await
            .traffic_snapshots()
            .into_iter()
            .find(|entry| entry.address == fixture.address())
            .expect("native root")
            .ifac_violations
    };
    let before = violations().await;
    write_frame(
        writer,
        0,
        &encode_packet_ifac(&auth(rejected), &packet).expect("wrong-key packet"),
    )
    .await;
    write_frame(writer, 0, &packet.to_bytes().expect("plaintext packet")).await;
    timeout(Duration::from_secs(2), async {
        while violations().await != before + 2 {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("counted rejection before native admission");
    assert!(ingress.try_recv().is_err(), "wrong-key and plaintext cannot enter Transport");
    let raw = encode_packet_ifac(&auth(key), &packet).expect("authenticated ingress");
    assert_eq!(raw.len(), packet.to_bytes().expect("packet bytes").len() + 8);
    write_frame(writer, 0, &raw).await;
    let event = timeout(Duration::from_secs(2), ingress.recv())
        .await
        .expect("ingress deadline")
        .expect("native Transport admission");
    assert_eq!(event.address, fixture.address());
    assert_eq!(event.packet.destination, packet.destination);
    assert_eq!(event.packet.data, packet.data);
    assert_eq!(event.packet.ifac.map(|ifac| ifac.length), Some(0));
    let outbound = Packet { destination: AddressHash::new([marker + 1; 16]), ..packet };
    let mut expected = vec![0];
    expected.extend(encode_packet_ifac(&auth(key), &outbound).expect("authenticated egress"));
    assert_eq!(
        manager
            .lock()
            .await
            .send(TxMessage { tx_type: TxMessageType::Direct(fixture.address()), packet: outbound })
            .await
            .sent_ifaces,
        1
    );
    expect_frame(reader, writer, &expected).await;
}

async fn management<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    fixture: &Fixture,
    reader: &mut R,
    writer: &mut W,
) {
    let response = query(&fixture.daemon);
    assert!(response.error.is_none(), "{response:?}");
    let result = response.result.expect("queued native control");
    assert_redacted(&result);
    assert_eq!(result["queued"], true);
    assert_eq!(result["command"], "radio_state_query");
    expect_frame(reader, writer, &[0x06, 0xff]).await;
    write_frame(writer, 0x06, &[1]).await;
}

fn fresh(fixture: &Fixture) {
    let status = fixture.native.to_json();
    assert_eq!(status["online"], false);
    assert_eq!(status["probe_status"]["detected"], false);
    assert!(status["probe_status"]["firmware_version"].is_null());
    assert!(status["radio_status"]["frequency_hz"].is_null());
    assert!(status["last_command_error"].is_null());
}

#[tokio::test]
async fn configured_lora_serial_state_repair_reopen_control_and_restart() {
    let root = tempfile::tempdir().expect("owned state");
    let alias = root.path().join("owned-lora-device");
    let state = root.path().join("owned-lora-state.json");
    let other = TcpListener::bind("127.0.0.1:0").await.expect("unrelated listener");
    let port = other.local_addr().expect("unrelated address").port();
    let config = config(alias.to_str().expect("alias"), Some(&state), port, INITIAL, None);
    for (payload, expected) in [(b"{invalid".to_vec(), "invalid lora state"), (serde_json::to_vec(&json!({"version":1,"duty_cycle_debt_ms":0,"last_updated_unix_ms":1,"uncertain":true,"uncertainty_reason":"owned fixture"})).expect("uncertain state"), "marked uncertain")] {
        std::fs::write(&state, payload).expect("owned invalid/uncertain state");
        let startup = start_fixture(&Args::parse_from(["reticulumd"]), &config, root.path()).await;
        assert_eq!(startup.interfaces.startup_failures.len(), 1);
        assert!(startup.interfaces.startup_failures[0].error.contains(expected));
        assert_redacted(&startup.interfaces.startup_failures[0].error);
        assert!(startup.interfaces.lora_runtime_refreshes.is_empty());
        assert!(startup.interfaces.rnode_management_bindings.is_empty());
        assert_eq!(startup.interfaces.configured_roots.len(), 1, "only unrelated TCP owns a root");
        let runtime = &startup.configured_interfaces[0].settings.as_ref().expect("settings")["_runtime"];
        assert_eq!(runtime["startup_status"], "failed");
        assert!(runtime["iface"].is_null());
        let unrelated = native_address(&startup.configured_interfaces[1]);
        let mut peer = accept(&other).await;
        verify_unrelated_tcp(startup.transport.as_ref().expect("transport"), unrelated, &mut peer, 0x11).await;
        retire_fixture(startup).await;
        closed(&mut peer).await;
    }
    std::fs::remove_file(&state).expect("repair only the owned invalid state");
    let mut fixture = Fixture::start(&config, root.path()).await;
    let identity = fixture.transport.internal_identity().await;
    let address = fixture.address();
    let unrelated = native_address(&fixture.daemon.interface_records()[1]);
    let mut unaffected = accept(&other).await;
    fixture.failed("failed to open LoRa serial").await;
    policy(&fixture, INITIAL, 220).await;
    let persisted: Value =
        serde_json::from_slice(&std::fs::read(&state).expect("repaired persisted state"))
            .expect("valid state");
    assert_eq!(persisted["uncertain"], false);
    let mut first = PtyPeer::start().await;
    first.publish_alias(&alias);
    startup(&mut first.output, &mut first.input, true).await;
    fixture.radio_ready("serial", 220).await;
    authenticate(&fixture, &mut first.output, &mut first.input, INITIAL, ROTATED, 0x21).await;
    rotate(&fixture).await;
    policy(&fixture, ROTATED, 220).await;
    authenticate(&fixture, &mut first.output, &mut first.input, ROTATED, INITIAL, 0x31).await;
    management(&fixture, &mut first.output, &mut first.input).await;
    verify_unrelated_tcp(&fixture.transport, unrelated, &mut unaffected, 0x41).await;
    std::fs::remove_file(&alias).expect("retire owned alias before master close");
    first.stop().await;
    fixture.failed("failed to open LoRa serial").await;
    assert_eq!(fixture.address(), address);
    policy(&fixture, ROTATED, 220).await;
    verify_unrelated_tcp(&fixture.transport, unrelated, &mut unaffected, 0x51).await;
    let mut repaired = PtyPeer::start().await;
    repaired.publish_alias(&alias);
    startup(&mut repaired.output, &mut repaired.input, true).await;
    fixture.radio_ready("serial", 220).await;
    authenticate(&fixture, &mut repaired.output, &mut repaired.input, ROTATED, INITIAL, 0x61).await;
    management(&fixture, &mut repaired.output, &mut repaired.input).await;
    std::fs::remove_file(&alias).expect("retire repaired alias");
    fixture.shutdown().await;
    repaired.stop().await;
    closed(&mut unaffected).await;
    let retired = fixture.native.to_json();
    let mut final_peer = PtyPeer::start().await;
    final_peer.publish_alias(&alias);
    let config = self::config(alias.to_str().expect("alias"), Some(&state), port, FINAL, Some(200));
    let mut restarted = Fixture::start(&config, root.path()).await;
    assert_ne!(restarted.transport.internal_identity().await, identity);
    fresh(&restarted);
    let mut unaffected = accept(&other).await;
    startup(&mut final_peer.output, &mut final_peer.input, true).await;
    restarted.radio_ready("serial", 200).await;
    policy(&restarted, FINAL, 200).await;
    authenticate(&restarted, &mut final_peer.output, &mut final_peer.input, FINAL, ROTATED, 0x71)
        .await;
    management(&restarted, &mut final_peer.output, &mut final_peer.input).await;
    let unrelated = native_address(&restarted.daemon.interface_records()[1]);
    verify_unrelated_tcp(&restarted.transport, unrelated, &mut unaffected, 0x81).await;
    assert_eq!(
        fixture.native.to_json(),
        retired,
        "new transport cannot rewrite retired native status"
    );
    std::fs::remove_file(&alias).expect("retire final owned alias");
    restarted.shutdown().await;
    final_peer.stop().await;
    closed(&mut unaffected).await;
}

#[tokio::test]
async fn configured_rnode_tcp_refusal_invalid_firmware_reconnect_control_and_restart() {
    let root = tempfile::tempdir().expect("owned state");
    let reserved = TcpSocket::new_v4().expect("owned unlistened socket");
    reserved.bind("127.0.0.1:0".parse().expect("loopback")).expect("reserve refused port");
    let endpoint = reserved.local_addr().expect("owned endpoint");
    let device = format!("tcp://{endpoint}");
    let other = TcpListener::bind("127.0.0.1:0").await.expect("unrelated listener");
    let port = other.local_addr().expect("unrelated address").port();
    let mut fixture =
        Fixture::start(&config(&device, None, port, INITIAL, None), root.path()).await;
    let identity = fixture.transport.internal_identity().await;
    let address = fixture.address();
    let unrelated = native_address(&fixture.daemon.interface_records()[1]);
    let mut unaffected = accept(&other).await;
    fixture.failed("failed to connect LoRa tcp").await;
    policy(&fixture, INITIAL, 508).await;
    let listener = reserved.listen(8).expect("repair the same owned refused endpoint");
    let invalid = accept(&listener).await;
    let (mut read, mut write) = invalid.into_split();
    startup(&mut read, &mut write, false).await;
    closed(&mut read).await;
    fixture.failed("firmware version 1.0 is below required").await;
    assert_eq!(fixture.address(), address);
    verify_unrelated_tcp(&fixture.transport, unrelated, &mut unaffected, 0x91).await;
    drop((read, write));
    let connected = accept(&listener).await;
    let (mut read, mut write) = connected.into_split();
    startup(&mut read, &mut write, true).await;
    fixture.radio_ready("tcp", 508).await;
    authenticate(&fixture, &mut read, &mut write, INITIAL, ROTATED, 0xa1).await;
    rotate(&fixture).await;
    policy(&fixture, ROTATED, 508).await;
    authenticate(&fixture, &mut read, &mut write, ROTATED, INITIAL, 0xb1).await;
    management(&fixture, &mut read, &mut write).await;
    drop((read, write));
    fixture.status(false, |s| s["online"] == false).await;
    fixture.status(true, |s| s["online"] == false).await;
    assert_eq!(fixture.address(), address);
    policy(&fixture, ROTATED, 508).await;
    let connected = accept(&listener).await;
    let (mut read, mut write) = connected.into_split();
    startup(&mut read, &mut write, true).await;
    fixture.radio_ready("tcp", 508).await;
    authenticate(&fixture, &mut read, &mut write, ROTATED, INITIAL, 0xc1).await;
    management(&fixture, &mut read, &mut write).await;
    verify_unrelated_tcp(&fixture.transport, unrelated, &mut unaffected, 0xd1).await;
    fixture.shutdown().await;
    closed(&mut read).await;
    closed(&mut unaffected).await;
    drop((read, write));
    let retired = fixture.native.to_json();
    let mut restarted =
        Fixture::start(&config(&device, None, port, FINAL, Some(200)), root.path()).await;
    assert_ne!(restarted.transport.internal_identity().await, identity);
    fresh(&restarted);
    let mut unaffected = accept(&other).await;
    let connected = accept(&listener).await;
    let (mut read, mut write) = connected.into_split();
    startup(&mut read, &mut write, true).await;
    restarted.radio_ready("tcp", 200).await;
    policy(&restarted, FINAL, 200).await;
    authenticate(&restarted, &mut read, &mut write, FINAL, ROTATED, 0xe1).await;
    management(&restarted, &mut read, &mut write).await;
    let unrelated = native_address(&restarted.daemon.interface_records()[1]);
    verify_unrelated_tcp(&restarted.transport, unrelated, &mut unaffected, 0xf1).await;
    assert_eq!(fixture.native.to_json(), retired);
    restarted.shutdown().await;
    closed(&mut read).await;
    closed(&mut unaffected).await;
}
