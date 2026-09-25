#![cfg(unix)]

use rns_rpc::rpc::{codec, RpcRequest, RpcResponse};
use rns_transport::iface::hdlc::Hdlc;
use rns_transport::packet::{Header, Packet, PacketContext, PacketDataBuffer, PacketType};
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const PINNED_RETICULUM: &str = "99de23c040d507e3fefca19e87b182302902725d";

#[test]
fn standalone_daemon_shutdown_persists_proof_replay_filter_across_restart() {
    let temp = tempfile::tempdir().expect("temporary daemon storage");
    let storage = temp.path().join("storage");
    std::fs::create_dir_all(&storage).expect("create storage directory");
    let config = temp.path().join("reticulum.toml");
    std::fs::write(&config, "[reticulum]\nenable_transport = true\n")
        .expect("write transport config");
    let (transport_port, rpc_port) = reserve_two_ports();
    let packet = proof_packet();
    let packet_hash = packet.hash().as_slice().to_vec();
    let frame = Hdlc::frame(&packet.to_bytes().expect("serialize proof packet"))
        .expect("HDLC-frame proof packet");

    let mut first = start_daemon(temp.path(), &storage, &config, transport_port, rpc_port);
    wait_for_rpc(rpc_port, &mut first);
    let first_ingress = send_frames_and_keep_open(transport_port, &frame, 2);
    let first_status =
        wait_for_status(rpc_port, &mut first, |status| interface_filter_hits(status) == Some(1));
    assert_eq!(interface_filter_hits(&first_status), Some(1));
    drop(first_ingress);
    stop_gracefully(&mut first);

    let persisted = std::fs::read(storage.join("packet_hashlist.raw"))
        .expect("first daemon shutdown must persist the replay hashlist");
    assert!(
        persisted.chunks_exact(packet_hash.len()).any(|hash| hash == packet_hash),
        "packet_hashlist.raw did not contain the accepted proof hash"
    );

    let mut second = start_daemon(temp.path(), &storage, &config, transport_port, rpc_port);
    let restored_status = wait_for_rpc(rpc_port, &mut second);
    assert!(restored_status["running"].as_bool().unwrap_or(false));
    let second_ingress = send_frames_and_keep_open(transport_port, &frame, 1);
    let rejected_status = wait_for_status(rpc_port, &mut second, |status| {
        interface_filter_hits(status).is_some_and(|hits| hits >= 1)
    });
    assert!(
        interface_filter_hits(&rejected_status).unwrap_or_default() >= 1,
        "second process did not count the replay as a production ingress filter hit; pinned behavior is packet_filter rejection at Reticulum {PINNED_RETICULUM}"
    );
    drop(second_ingress);
    stop_gracefully(&mut second);
}

fn proof_packet() -> Packet {
    Packet {
        header: Header { packet_type: PacketType::Proof, ..Header::default() },
        context: PacketContext::None,
        destination: rns_transport::hash::AddressHash::new_from_slice(&[0x91; 16]),
        data: PacketDataBuffer::new_from_slice(&[0x37; 32]),
        ..Packet::default()
    }
}

fn reserve_two_ports() -> (u16, u16) {
    let transport_listener = TcpListener::bind(("127.0.0.1", 0)).expect("reserve transport port");
    let rpc_listener = TcpListener::bind(("127.0.0.1", 0)).expect("reserve RPC port");
    let transport_port = transport_listener.local_addr().expect("read transport port").port();
    let rpc_port = rpc_listener.local_addr().expect("read RPC port").port();
    drop((transport_listener, rpc_listener));
    (transport_port, rpc_port)
}

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn start_daemon(
    root: &Path,
    storage: &Path,
    config: &Path,
    transport_port: u16,
    rpc_port: u16,
) -> Daemon {
    let child = Command::new(env!("CARGO_BIN_EXE_reticulumd"))
        .arg("--db")
        .arg(storage.join("reticulum.db"))
        .arg("--identity")
        .arg(storage.join("identity"))
        .arg("--config")
        .arg(config)
        .arg("--transport")
        .arg(format!("127.0.0.1:{transport_port}"))
        .arg("--rpc")
        .arg(format!("127.0.0.1:{rpc_port}"))
        .arg("--rpc-unix")
        .arg(root.join(format!("rpc-{rpc_port}.sock")))
        .env("RUST_LOG", "debug")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn standalone reticulumd process");
    Daemon(child)
}

fn wait_for_rpc(port: u16, child: &mut Daemon) -> Value {
    wait_until(child, "daemon RPC readiness", || rpc_status(port).ok())
}

fn wait_for_status<F>(port: u16, child: &mut Daemon, mut predicate: F) -> Value
where
    F: FnMut(&Value) -> bool,
{
    wait_until(child, "transport status predicate", || {
        rpc_status(port).ok().filter(|status| predicate(status))
    })
}

fn wait_until<F>(child: &mut Daemon, description: &str, mut probe: F) -> Value
where
    F: FnMut() -> Option<Value>,
{
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(value) = probe() {
            return value;
        }
        if let Ok(Some(status)) = child.0.try_wait() {
            panic!("daemon exited before {description}: {status}");
        }
        assert!(Instant::now() < deadline, "timed out waiting for {description}");
        thread::sleep(Duration::from_millis(25));
    }
}

fn send_frames_and_keep_open(port: u16, frame: &[u8], copies: usize) -> TcpStream {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut stream = loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(stream) => break stream,
            Err(_) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(25));
            }
            Err(error) => panic!("connect daemon ingress: {error}"),
        }
    };
    stream.set_write_timeout(Some(Duration::from_secs(2))).expect("set ingress write timeout");
    for _ in 0..copies {
        stream.write_all(frame).expect("write production-ingress HDLC packet");
    }
    stream.flush().expect("flush production-ingress HDLC frames");
    stream
}

fn rpc_status(port: u16) -> std::io::Result<Value> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(250))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let body = codec::encode_frame(&RpcRequest {
        id: 1,
        method: "daemon_status_ex".to_owned(),
        params: None,
    })?;
    write!(
        stream,
        "POST /rpc HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/msgpack\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    let body_start = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|offset| offset + 4)
        .ok_or_else(|| std::io::Error::other("RPC response omitted HTTP header boundary"))?;
    let decoded: RpcResponse = codec::decode_frame(&response[body_start..])?;
    decoded.result.ok_or_else(|| {
        std::io::Error::other(format!("daemon status RPC returned error: {:?}", decoded.error))
    })
}

fn interface_filter_hits(status: &Value) -> Option<u64> {
    status["reticulum"]["transport"]["interfaces"]
        .as_array()?
        .iter()
        .filter_map(|interface| interface["violations"]["packet_filter_hits"].as_u64())
        .max()
}

fn stop_gracefully(child: &mut Daemon) {
    let status = Command::new("kill")
        .args(["-INT", &child.0.id().to_string()])
        .status()
        .expect("send SIGINT to exact daemon child");
    assert!(status.success(), "failed to signal daemon child with SIGINT");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.0.try_wait().expect("wait for graceful daemon exit") {
            assert!(status.success(), "daemon failed during graceful shutdown: {status}");
            return;
        }
        assert!(Instant::now() < deadline, "daemon did not exit after SIGINT");
        thread::sleep(Duration::from_millis(25));
    }
}
