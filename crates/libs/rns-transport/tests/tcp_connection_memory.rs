use std::net::{Ipv4Addr, SocketAddrV4, TcpListener as StdTcpListener};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rns_transport::buffer::OutputBuffer;
use rns_transport::iface::hdlc::Hdlc;
use rns_transport::iface::tcp_client::TcpClient;
use rns_transport::iface::tcp_server::TcpServer;
use rns_transport::iface::InterfaceManager;
use rns_transport::packet::{Packet, PacketDataBuffer};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

fn reserve_loopback_addr() -> SocketAddrV4 {
    let listener = StdTcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("reserve loopback port");
    let addr = match listener.local_addr().expect("reserved address") {
        std::net::SocketAddr::V4(addr) => addr,
        std::net::SocketAddr::V6(_) => unreachable!("IPv4 bind returned IPv6"),
    };
    drop(listener);
    addr
}

async fn wait_for(mut condition: impl FnMut() -> bool, label: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {label}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tcp_server_handles_many_idle_then_active_small_connections() {
    const CONNECTIONS: usize = 128;
    let addr = reserve_loopback_addr();
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(512)));
    let receiver = manager.lock().await.receiver();
    let server = TcpServer::new(addr.to_string(), manager.clone());
    let status = server.runtime_status_handle();
    let server_iface = manager.lock().await.spawn(server, TcpServer::spawn);

    wait_for(
        || status.to_json()["listener_state"].as_str() == Some("listening"),
        "TCP listener startup",
    )
    .await;

    let mut clients = Vec::with_capacity(CONNECTIONS);
    for _ in 0..CONNECTIONS {
        clients.push(TcpStream::connect(addr).await.expect("connect idle client"));
    }
    wait_for(
        || status.to_json()["accepted_connections"].as_u64() == Some(CONNECTIONS as u64),
        "idle connections",
    )
    .await;

    let packet = Packet {
        data: PacketDataBuffer::new_from_slice(b"small-active-packet"),
        ..Default::default()
    };
    let raw = packet.to_bytes().expect("serialize packet");
    let mut frame = vec![0_u8; raw.len() * 2 + 2];
    let frame_len = {
        let mut output = OutputBuffer::new(frame.as_mut_slice());
        Hdlc::encode(&raw, &mut output).expect("encode frame");
        output.offset()
    };
    for client in &mut clients {
        client.write_all(&frame[..frame_len]).await.expect("send small packet");
    }

    let mut received = 0_usize;
    while received < CONNECTIONS {
        let message = tokio::time::timeout(Duration::from_secs(10), receiver.lock().await.recv())
            .await
            .expect("receive deadline")
            .expect("received packet");
        assert_eq!(message.packet.data.as_slice(), b"small-active-packet");
        received += 1;
    }

    drop(clients);
    assert!(manager.lock().await.stop_interface(server_iface));
}

#[test]
fn default_tcp_mtu_remains_reticulum_compatible() {
    assert_eq!(TcpClient::DEFAULT_MTU, 262_144);
}
