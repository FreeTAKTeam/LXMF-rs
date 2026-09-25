use super::*;
use crate::buffer::OutputBuffer;
use crate::iface::hdlc::Hdlc;
use crate::packet::{Packet, PacketDataBuffer};
use crate::serde::Serialize;
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn tcp_server_routes_accepted_packets_and_recovers_after_peer_eof() {
    let probe = TcpListener::bind("127.0.0.1:0").await.expect("bind probe listener");
    let addr = probe.local_addr().expect("probe listener address");
    drop(probe);

    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let rx_recv = manager.lock().await.receiver();
    let server = TcpServer::new(addr.to_string(), manager.clone());
    let status = server.runtime_status_handle();
    let parent_iface = manager.lock().await.spawn(server, TcpServer::spawn);

    wait_for_status(&status, |snapshot| {
        snapshot.get("listener_state").and_then(serde_json::Value::as_str) == Some("listening")
    })
    .await;

    let mut first_peer = TcpStream::connect(addr).await.expect("connect first client");
    wait_for_status(&status, |snapshot| {
        snapshot.get("accepted_connections").and_then(serde_json::Value::as_u64) == Some(1)
    })
    .await;

    send_server_packet(&mut first_peer, b"tcp-server-first-client").await;
    let first_message =
        tokio::time::timeout(Duration::from_secs(2), async { rx_recv.lock().await.recv().await })
            .await
            .expect("first packet receive deadline")
            .expect("first client packet");
    assert_ne!(first_message.address, parent_iface);
    assert_eq!(first_message.packet.data.as_slice(), b"tcp-server-first-client");
    assert_eq!(first_message.source, crate::iface::IfaceSource::None);

    first_peer.shutdown().await.expect("close first client write side");
    wait_for_status(&status, |snapshot| {
        snapshot["latest_stream_status"]["stream_state"].as_str() == Some("closed")
    })
    .await;

    let mut second_peer = TcpStream::connect(addr).await.expect("connect second client");
    wait_for_status(&status, |snapshot| {
        snapshot.get("accepted_connections").and_then(serde_json::Value::as_u64) == Some(2)
    })
    .await;
    send_server_packet(&mut second_peer, b"tcp-server-second-client").await;
    let second_message =
        tokio::time::timeout(Duration::from_secs(2), async { rx_recv.lock().await.recv().await })
            .await
            .expect("second packet receive deadline")
            .expect("second client packet");
    assert_eq!(second_message.packet.data.as_slice(), b"tcp-server-second-client");

    second_peer.shutdown().await.expect("close second client write side");
    wait_for_status(&status, |snapshot| {
        snapshot["latest_stream_status"]["stream_state"].as_str() == Some("closed")
    })
    .await;
    manager.lock().await.stop_interface(parent_iface);
    wait_for_status(&status, |snapshot| {
        snapshot.get("listener_state").and_then(serde_json::Value::as_str) == Some("closed")
    })
    .await;
}

async fn send_server_packet(peer: &mut TcpStream, payload: &[u8]) {
    let packet = Packet { data: PacketDataBuffer::new_from_slice(payload), ..Packet::default() };
    let mut raw = vec![0_u8; TcpClient::DEFAULT_MTU];
    let raw_len = {
        let mut output = OutputBuffer::new(&mut raw);
        packet.serialize(&mut output).expect("serialize packet");
        output.offset()
    };
    let mut wire = vec![0_u8; TcpClient::DEFAULT_MTU + 8];
    let wire_len = {
        let mut output = OutputBuffer::new(&mut wire);
        Hdlc::encode(&raw[..raw_len], &mut output).expect("encode packet frame");
        output.offset()
    };
    peer.write_all(&wire[..wire_len]).await.expect("send packet frame");
}
