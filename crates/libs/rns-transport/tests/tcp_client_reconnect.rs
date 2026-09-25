use std::time::Duration;

use rns_transport::buffer::OutputBuffer;
use rns_transport::iface::hdlc::Hdlc;
use rns_transport::iface::tcp_client::TcpClient;
use rns_transport::iface::{InterfaceManager, TxMessage, TxMessageType};
use rns_transport::packet::{Packet, PacketDataBuffer};
use rns_transport::serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

fn packet_wire_bytes(data: &[u8]) -> Vec<u8> {
    let packet = Packet { data: PacketDataBuffer::new_from_slice(data), ..Packet::default() };
    let mut bytes = vec![0_u8; 1024];
    let encoded_len = {
        let mut output = OutputBuffer::new(&mut bytes);
        packet.serialize(&mut output).expect("serialize packet");
        output.offset()
    };
    bytes.truncate(encoded_len);
    bytes
}

async fn accept_within(listener: &TcpListener, label: &str) -> TcpStream {
    timeout(Duration::from_secs(2), listener.accept())
        .await
        .unwrap_or_else(|_| panic!("{label} timed out"))
        .unwrap_or_else(|error| panic!("{label} failed: {error}"))
        .0
}

#[tokio::test]
async fn tcp_carrier_reconnect_resumes_bidirectional_packet_traffic_on_same_iface() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
    let addr = listener.local_addr().expect("listener address");
    let (reconnect_tx, mut reconnect_rx) = tokio::sync::mpsc::channel(4);
    let mut manager = InterfaceManager::new(8);
    let context =
        manager.new_context(TcpClient::new(addr.to_string()).with_reconnect_events(reconnect_tx));
    let iface = context.channel.address;
    let receiver = manager.receiver();
    let cancel = context.cancel.clone();
    let iface_stop = context.channel.stop.clone();
    let task = tokio::spawn(TcpClient::spawn(context));

    let first_stream = accept_within(&listener, "initial TCP connection").await;
    drop(first_stream);

    let mut recovered_stream = accept_within(&listener, "TCP carrier reconnect").await;
    let recovered_iface = timeout(Duration::from_secs(2), reconnect_rx.recv())
        .await
        .expect("reconnect event timed out")
        .expect("reconnect event channel closed");
    assert_eq!(recovered_iface, iface, "carrier recovery must retain the interface identity");

    let outbound = packet_wire_bytes(b"outbound-after-carrier-reconnect");
    let outbound_frame = Hdlc::frame(&outbound).expect("frame outbound packet");
    let trace = manager
        .send(TxMessage {
            tx_type: TxMessageType::Direct(iface),
            packet: Packet {
                data: PacketDataBuffer::new_from_slice(b"outbound-after-carrier-reconnect"),
                ..Packet::default()
            },
        })
        .await;
    assert_eq!(trace.sent_ifaces, 1);
    let mut received_frame = vec![0_u8; outbound_frame.len()];
    timeout(Duration::from_secs(2), recovered_stream.read_exact(&mut received_frame))
        .await
        .expect("outbound packet timed out")
        .expect("read outbound packet");
    assert_eq!(received_frame, outbound_frame);

    let inbound = packet_wire_bytes(b"inbound-after-carrier-reconnect");
    let inbound_frame = Hdlc::frame(&inbound).expect("frame inbound packet");
    recovered_stream.write_all(&inbound_frame).await.expect("write inbound packet");
    let message = timeout(Duration::from_secs(2), async { receiver.lock().await.recv().await })
        .await
        .expect("inbound packet timed out")
        .expect("interface receiver closed");
    assert_eq!(message.address, iface);
    assert_eq!(message.packet.data.as_slice(), b"inbound-after-carrier-reconnect");

    cancel.cancel();
    drop(recovered_stream);
    timeout(Duration::from_secs(2), task)
        .await
        .expect("TCP client task timed out")
        .expect("TCP client task panicked");
    assert!(iface_stop.is_cancelled());
}
