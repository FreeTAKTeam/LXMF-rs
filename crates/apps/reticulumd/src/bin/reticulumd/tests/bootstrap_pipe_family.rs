use super::restore_tests::{native_address, verify_unrelated_tcp};
use super::*;
use crate::interface_hot_apply::{InterfaceHotApplyBridge, InterfaceMutationOwner};
use rns_transport::iface::hdlc::Hdlc;
use rns_transport::iface::{
    encode_packet_ifac, IfacState, InterfaceSharedConfig, TxMessage, TxMessageType,
};
use rns_transport::packet::{Packet, PacketDataBuffer};
use serde_json::{json, Value};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{sleep, timeout, Duration};

const INITIAL: &str = "pipe-family-initial-credential";
const ROTATED: &str = "pipe-family-rotated-credential";
const ATTEMPTED: &str = "pipe-family-attempted-credential";
const FINAL: &str = "pipe-family-final-credential";

fn assert_redacted(value: &impl serde::Serialize) {
    let text = serde_json::to_string(value).expect("diagnostic JSON");
    for credential in [INITIAL, ROTATED, ATTEMPTED, FINAL] {
        assert!(!text.contains(credential), "runtime diagnostics cannot expose credentials");
    }
}

fn config(command: &str, other_port: u16, credential: &str, mtu: usize) -> String {
    format!("interfaces=[{{type='pipe',enabled=true,name='pipe-family',command={},respawn_delay=0.25,mtu={mtu},network_name='pipe-family-network',passphrase='{credential}'}},{{type='tcp_client',enabled=true,name='unrelated',host='127.0.0.1',port={other_port}}}]\n[reticulum]\nshare_instance=false", json!(command))
}

fn executable_command(executable: &Path) -> String {
    format!("'{}'", executable.to_str().expect("fixture path").replace('\'', "'\\''"))
}

fn publish_executable(executable: &Path, control_port: u16) {
    let script = format!(
        r#"#!/usr/bin/env python3
import os, select, socket, struct
peer = socket.create_connection(("127.0.0.1", {control_port}))
peer.sendall(struct.pack("!I", os.getpid()))
while True:
    for source in select.select([0, peer], [], [])[0]:
        data = os.read(0, 4096) if source == 0 else peer.recv(4096)
        if not data:
            raise SystemExit(0)
        if source == 0:
            peer.sendall(data)
        else:
            while data:
                written = os.write(1, data)
                if written == 0:
                    raise RuntimeError("stdout made no progress")
                data = data[written:]
"#
    );
    let staging = executable.with_extension("pending");
    std::fs::write(&staging, script).expect("write complete child executable");
    std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o700))
        .expect("owned executable permissions");
    std::fs::rename(staging, executable).expect("publish complete executable atomically");
}

async fn start(
    text: &str,
    root: &Path,
    path: &Path,
) -> (Arc<Transport>, Arc<rns_rpc::RpcDaemon>, InterfaceMutationOwner) {
    std::fs::write(path, text).expect("configured Pipe file");
    let config = DaemonConfig::from_toml(text).expect("configured Pipe");
    let startup = start_fixture(&Args::parse_from(["reticulumd"]), &config, root).await;
    assert!(startup.interfaces.startup_failures.is_empty());
    let transport = startup.transport.expect("native transport");
    let daemon = Arc::new(rns_rpc::RpcDaemon::test_instance());
    daemon.replace_interfaces(startup.configured_interfaces.clone());
    let owner = InterfaceHotApplyBridge::spawn_with_configured_roots(
        transport.clone(),
        startup.interfaces.into_owned_roots(&startup.configured_interfaces),
        Arc::downgrade(&daemon),
        None,
        startup.configured_startup,
    )
    .with_management_config(Some(path.to_owned()), true);
    daemon.set_interface_mutation_bridge(Arc::new(owner.bridge.clone()));
    (transport, daemon, owner)
}

async fn status(daemon: &Arc<rns_rpc::RpcDaemon>, state: &str, attempts: u64) -> Value {
    timeout(Duration::from_secs(3), async {
        loop {
            let record = &daemon.interface_records()[0];
            let runtime = &record.settings.as_ref().expect("settings")["_runtime"];
            let status = &runtime["pipe"]["status"];
            if status["process_state"] == state
                && status["respawn_attempts"].as_u64().expect("respawn count") >= attempts
            {
                assert_redacted(runtime);
                break status.clone();
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("published native Pipe status deadline")
}

async fn accept(listener: &TcpListener) -> TcpStream {
    let (peer, source) = timeout(Duration::from_secs(3), listener.accept())
        .await
        .expect("native child connection deadline")
        .expect("native child connection");
    assert!(source.ip().is_loopback());
    peer
}

fn direct_child(pid: u32, executable: &Path) {
    let status =
        std::fs::read_to_string(format!("/proc/{pid}/status")).expect("greeted child is alive");
    let parent = status
        .lines()
        .find_map(|line| line.strip_prefix("PPid:"))
        .expect("process parent")
        .trim()
        .parse::<u32>()
        .expect("parent PID");
    assert_eq!(parent, std::process::id(), "native child must be owned by this test process");
    let argv = std::fs::read(format!("/proc/{pid}/cmdline")).expect("live child argv");
    assert!(
        argv.split(|byte| *byte == 0).any(|arg| arg == executable.as_os_str().as_bytes()),
        "greeted process must run this test's owned executable"
    );
}

async fn accept_child(listener: &TcpListener, executable: &Path) -> (TcpStream, u32) {
    let mut peer = accept(listener).await;
    let mut greeting = [0; 4];
    timeout(Duration::from_secs(2), peer.read_exact(&mut greeting))
        .await
        .expect("child PID greeting deadline")
        .expect("complete child PID greeting");
    let pid = u32::from_be_bytes(greeting);
    direct_child(pid, executable);
    (peer, pid)
}

async fn reaped(pid: u32) {
    timeout(Duration::from_secs(2), async {
        loop {
            match std::fs::metadata(format!("/proc/{pid}")) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                Ok(_) => sleep(Duration::from_millis(5)).await,
                Err(error) => panic!("child process observation failed: {error}"),
            }
        }
    })
    .await
    .expect("exact native child is reaped");
}

async fn closed(peer: &mut TcpStream) {
    timeout(Duration::from_secs(2), peer.read_to_end(&mut Vec::new()))
        .await
        .expect("retired native peer EOF deadline")
        .expect("retired native peer EOF");
}

async fn shutdown(transport: &Arc<Transport>, owner: &mut InterfaceMutationOwner) {
    timeout(Duration::from_secs(10), owner.shutdown())
        .await
        .expect("existing native cleanup bound")
        .expect("all native and reconnect workers joined");
    assert!(transport.interface_hashes().await.is_empty());
}

fn auth(credential: &str) -> IfacState {
    Arc::new(std::sync::RwLock::new(
        InterfaceSharedConfig {
            network_name: Some("pipe-family-network".into()),
            passphrase: Some(credential.into()),
            ..Default::default()
        }
        .ifac_context_with_default_size(8)
        .expect("independent peer with Pipe's eight-byte IFAC default"),
    ))
}

async fn authenticate(
    transport: &Arc<Transport>,
    address: AddressHash,
    peer: &mut TcpStream,
    (credential, rejected): (&str, &str),
    marker: u8,
) {
    let packet = Packet {
        destination: AddressHash::new([marker; 16]),
        data: PacketDataBuffer::new_from_slice(&[marker, 0x7e, 0x7d, 0xc0, 0xdb]),
        ..Default::default()
    };
    let manager = transport.iface_manager();
    let mut ingress = transport.iface_rx();
    let violations = || async {
        manager
            .lock()
            .await
            .traffic_snapshots()
            .into_iter()
            .find(|entry| entry.address == address)
            .expect("native Pipe root")
            .ifac_violations
    };
    let before = violations().await;
    let mut negative =
        Hdlc::frame(&encode_packet_ifac(&auth(rejected), &packet).expect("wrong key"))
            .expect("negative HDLC");
    negative.extend(Hdlc::frame(&packet.to_bytes().expect("plaintext")).expect("plaintext HDLC"));
    peer.write_all(&negative).await.expect("native child emits negative frames");
    timeout(Duration::from_secs(2), async {
        while violations().await != before + 2 {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("wrong-key and plaintext rejected before native admission");
    while let Ok(event) = ingress.try_recv() {
        assert_ne!(event.packet.destination, packet.destination, "negative native ingress");
    }
    let state = auth(credential);
    let wire = encode_packet_ifac(&state, &packet).expect("authenticated ingress");
    assert_eq!(wire.len(), packet.to_bytes().expect("plain bytes").len() + 8);
    peer.write_all(&Hdlc::frame(&wire).expect("authenticated HDLC"))
        .await
        .expect("native child authenticated stdout");
    let received = timeout(Duration::from_secs(2), ingress.recv())
        .await
        .expect("authenticated native ingress deadline")
        .expect("authenticated native ingress");
    assert_eq!(received.address, address);
    assert_eq!(received.packet.destination, packet.destination);
    assert_eq!(received.packet.data, packet.data);
    assert_eq!(received.packet.ifac.map(|ifac| ifac.length), Some(0));
    let outbound = Packet { destination: AddressHash::new([marker + 1; 16]), ..packet };
    let expected = Hdlc::frame(&encode_packet_ifac(&state, &outbound).expect("native IFAC egress"))
        .expect("native HDLC egress");
    assert_eq!(
        manager
            .lock()
            .await
            .send(TxMessage { tx_type: TxMessageType::Direct(address), packet: outbound })
            .await
            .sent_ifaces,
        1
    );
    timeout(Duration::from_secs(2), async {
        let mut wire = Vec::new();
        let mut buffer = [0; 2048];
        loop {
            let count = peer.read(&mut buffer).await.expect("native child stdin observation");
            assert_ne!(count, 0, "child closed before exact authenticated egress");
            wire.extend_from_slice(&buffer[..count]);
            assert!(wire.len() <= 16_384, "unexpected output before target frame");
            if wire.windows(expected.len()).any(|candidate| candidate == expected) {
                break;
            }
        }
    })
    .await
    .expect("exact authenticated native egress deadline");
}

async fn assert_policy(transport: &Arc<Transport>, address: AddressHash, key: &str, mtu: usize) {
    let manager = transport.iface_manager();
    let manager = manager.lock().await;
    assert_eq!(manager.mtu(&address), Some(mtu));
    let policy = manager.shared_config(&address).expect("retained native IFAC policy");
    assert_eq!(policy.passphrase.as_deref(), Some(key));
    assert_eq!(policy.ifac_size, None, "carrier default remains authoritative");
}

async fn apply_rotation(daemon: &Arc<rns_rpc::RpcDaemon>) {
    let mut records = daemon.interface_records();
    records[0].settings.as_mut().expect("settings")["passphrase"] = json!(ROTATED);
    let mut events = daemon.subscribe_events();
    let response = daemon
        .handle_rpc(rns_rpc::RpcRequest {
            id: 1,
            method: "set_interfaces".into(),
            params: Some(json!({"interfaces":records})),
        })
        .expect("public Apply");
    assert!(response.error.is_none(), "{response:?}");
    timeout(Duration::from_secs(2), async {
        loop {
            let event = events.recv().await.expect("mutation completion");
            if event.event_type == "interfaces_updated" && event.payload["complete"] == true {
                break;
            }
        }
    })
    .await
    .expect("actual Apply completion");
}

async fn manage(daemon: &Arc<rns_rpc::RpcDaemon>, operation: &'static str) -> rns_rpc::RpcResponse {
    let daemon = daemon.clone();
    let reply = tokio::task::spawn_blocking(move || {
        daemon.handle_rpc(rns_rpc::RpcRequest {
            id: 2,
            method: "manage_interface".into(),
            params: Some(json!({"name":"pipe-family", "operation":operation})),
        })
    })
    .await
    .expect("management worker")
    .expect("management RPC");
    if let Some(error) = &reply.error {
        assert_redacted(error);
    }
    reply
}

#[tokio::test]
async fn configured_pipe_spawn_failure_repair_preserves_ifac_and_process_ownership() {
    let root = tempfile::tempdir().expect("owned state");
    let path = root.path().join("reticulum.toml");
    let executable = root.path().join("native-pipe-child");
    let control = TcpListener::bind("127.0.0.1:0").await.expect("owned child observer");
    let other = TcpListener::bind("127.0.0.1:0").await.expect("unrelated peer");
    let text = config(
        &executable_command(&executable),
        other.local_addr().expect("peer").port(),
        INITIAL,
        512,
    );
    let (transport, daemon, mut owner) = start(&text, root.path(), &path).await;
    let initial = native_address(&daemon.interface_records()[0]);
    let unrelated = native_address(&daemon.interface_records()[1]);
    let mut unaffected = accept(&other).await;
    let failed = status(&daemon, "respawning", 1).await;
    assert_eq!(failed["pipe_is_open"], false);
    assert!(failed["last_error"].as_str().expect("native spawn error").contains("spawn"));
    assert_policy(&transport, initial, INITIAL, 512).await;
    assert_eq!(transport.iface_manager().lock().await.online(&initial), Some(false));
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x11).await;
    let attempts = failed["respawn_attempts"].as_u64().expect("failure count");
    publish_executable(&executable, control.local_addr().expect("control").port());
    let (mut peer, first_pid) = accept_child(&control, &executable).await;
    assert_eq!(native_address(&daemon.interface_records()[0]), initial);
    let running = status(&daemon, "running", attempts).await;
    assert_eq!(running["pipe_is_open"], true);
    assert!(running["last_error"].is_null());
    authenticate(&transport, initial, &mut peer, (INITIAL, ROTATED), 0x14).await;
    assert_policy(&transport, initial, INITIAL, 512).await;
    peer.shutdown().await.expect("close only owned child's control input");
    closed(&mut peer).await;
    reaped(first_pid).await;
    let (mut respawned, next_pid) = accept_child(&control, &executable).await;
    assert_ne!(first_pid, next_pid);
    assert_eq!(native_address(&daemon.interface_records()[0]), initial);
    status(&daemon, "running", attempts + 1).await;
    authenticate(&transport, initial, &mut respawned, (INITIAL, ROTATED), 0x17).await;
    assert_policy(&transport, initial, INITIAL, 512).await;
    assert_eq!(native_address(&daemon.interface_records()[1]), unrelated);
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x1a).await;
    shutdown(&transport, &mut owner).await;
    closed(&mut respawned).await;
    closed(&mut unaffected).await;
    reaped(next_pid).await;
}

#[tokio::test]
async fn pipe_named_ifac_rotation_respawn_failed_reload_rollback_detach_and_reattach() {
    let root = tempfile::tempdir().expect("owned state");
    let path = root.path().join("reticulum.toml");
    let executable = root.path().join("native-pipe-child");
    let control = TcpListener::bind("127.0.0.1:0").await.expect("owned child observer");
    publish_executable(&executable, control.local_addr().expect("control").port());
    let command = executable_command(&executable);
    let other = TcpListener::bind("127.0.0.1:0").await.expect("unrelated peer");
    let other_port = other.local_addr().expect("unrelated endpoint").port();
    let (transport, daemon, mut owner) =
        start(&config(&command, other_port, INITIAL, 512), root.path(), &path).await;
    let initial = native_address(&daemon.interface_records()[0]);
    let unrelated = native_address(&daemon.interface_records()[1]);
    let (mut peer, initial_pid) = accept_child(&control, &executable).await;
    let mut unaffected = accept(&other).await;
    status(&daemon, "running", 0).await;
    authenticate(&transport, initial, &mut peer, (INITIAL, ROTATED), 0x31).await;
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x34).await;
    apply_rotation(&daemon).await;
    assert_eq!(native_address(&daemon.interface_records()[0]), initial);
    direct_child(initial_pid, &executable);
    authenticate(&transport, initial, &mut peer, (ROTATED, INITIAL), 0x37).await;
    peer.shutdown().await.expect("exit only current native child");
    closed(&mut peer).await;
    reaped(initial_pid).await;
    let (mut respawned, respawned_pid) = accept_child(&control, &executable).await;
    assert_ne!(respawned_pid, initial_pid);
    assert_eq!(native_address(&daemon.interface_records()[0]), initial);
    status(&daemon, "running", 1).await;
    assert_policy(&transport, initial, ROTATED, 512).await;
    authenticate(&transport, initial, &mut respawned, (ROTATED, INITIAL), 0x3a).await;
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x3d).await;

    std::fs::write(&path, config("'", other_port, ATTEMPTED, 1024))
        .expect("valid TOML, malformed shlex command");
    assert!(manage(&daemon, "reload").await.error.is_some());
    assert_eq!(native_address(&daemon.interface_records()[0]), initial);
    direct_child(respawned_pid, &executable);
    authenticate(&transport, initial, &mut respawned, (ROTATED, ATTEMPTED), 0x40).await;
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x43).await;

    let missing = executable_command(&root.path().join("never-created"));
    std::fs::write(&path, config(&missing, other_port, ATTEMPTED, 1024))
        .expect("valid missing-executable replacement");
    let reply = manage(&daemon, "reload").await;
    let error = reply.error.expect("actual spawn failure").message;
    assert!(
        error.contains("respawning") && error.contains("previous interface restored"),
        "{error}"
    );
    closed(&mut respawned).await;
    reaped(respawned_pid).await;
    assert_eq!(transport.iface_manager().lock().await.online(&initial), None);
    let rollback = native_address(&daemon.interface_records()[0]);
    assert_ne!(rollback, initial);
    let (mut restored, rollback_pid) = accept_child(&control, &executable).await;
    assert_policy(&transport, rollback, ROTATED, 512).await;
    authenticate(&transport, rollback, &mut restored, (ROTATED, ATTEMPTED), 0x46).await;
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x49).await;

    std::fs::write(&path, config(&command, other_port, FINAL, 1024))
        .expect("changed-key and MTU replacement");
    assert!(manage(&daemon, "reload").await.error.is_none());
    closed(&mut restored).await;
    reaped(rollback_pid).await;
    assert_eq!(transport.iface_manager().lock().await.online(&rollback), None);
    let replacement = native_address(&daemon.interface_records()[0]);
    let (mut replaced, replacement_pid) = accept_child(&control, &executable).await;
    assert_policy(&transport, replacement, FINAL, 1024).await;
    authenticate(&transport, replacement, &mut replaced, (FINAL, ROTATED), 0x4c).await;
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x4f).await;
    assert!(manage(&daemon, "detach").await.error.is_none());
    closed(&mut replaced).await;
    reaped(replacement_pid).await;
    assert_eq!(transport.iface_manager().lock().await.online(&replacement), None);
    assert!(!daemon.interface_records()[0].enabled);
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x52).await;
    assert!(manage(&daemon, "attach").await.error.is_none());
    let attached = native_address(&daemon.interface_records()[0]);
    assert_ne!(attached, replacement);
    let (mut reattached, attached_pid) = accept_child(&control, &executable).await;
    assert_policy(&transport, attached, FINAL, 1024).await;
    authenticate(&transport, attached, &mut reattached, (FINAL, ROTATED), 0x55).await;
    assert_eq!(native_address(&daemon.interface_records()[1]), unrelated);
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x58).await;
    shutdown(&transport, &mut owner).await;
    closed(&mut reattached).await;
    closed(&mut unaffected).await;
    reaped(attached_pid).await;
}
