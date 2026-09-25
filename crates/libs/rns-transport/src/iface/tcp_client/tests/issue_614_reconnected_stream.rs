use super::*;

#[tokio::test]
async fn tcp_client_receives_packets_after_established_stream_reconnects() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
    let addr = listener.local_addr().expect("listener address");
    let (reconnect_tx, mut reconnect_rx) = tokio::sync::mpsc::channel(1);
    let mut manager = InterfaceManager::new(8);
    let rx_recv = manager.rx_recv.clone();
    let client = TcpClient::new(addr.to_string()).with_reconnect_events(reconnect_tx);
    let runtime_status = client.runtime_status_handle();
    let context = manager.new_context(client);
    let iface_address = context.channel.address;
    let cancel = context.cancel.clone();
    let task = tokio::spawn(TcpClient::spawn(context));

    let (first_stream, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("initial connection deadline")
        .expect("accept initial connection");
    tokio::time::sleep(Duration::from_millis(2_100)).await;
    drop(first_stream);

    let (mut reconnected_stream, _) =
        tokio::time::timeout(Duration::from_secs(2), listener.accept())
            .await
            .expect("reconnect deadline")
            .expect("accept reconnected stream");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), reconnect_rx.recv())
            .await
            .expect("reconnect event deadline")
            .expect("reconnect event"),
        iface_address
    );

    let packet = Packet {
        data: PacketDataBuffer::new_from_slice(b"tcp-reconnected-stream"),
        ..Packet::default()
    };
    let mut raw = vec![0_u8; TcpClient::DEFAULT_MTU];
    let raw_len = {
        let mut output = OutputBuffer::new(&mut raw);
        packet.serialize(&mut output).expect("serialize packet");
        output.offset()
    };
    let mut wire = vec![0_u8; tcp_wire_buffer_capacity(TcpClient::DEFAULT_MTU)];
    let wire_len = {
        let mut output = OutputBuffer::new(&mut wire);
        Hdlc::encode(&raw[..raw_len], &mut output).expect("encode packet frame");
        output.offset()
    };
    reconnected_stream.write_all(&wire[..wire_len]).await.expect("write packet on new stream");

    let message =
        tokio::time::timeout(Duration::from_secs(2), async { rx_recv.lock().await.recv().await })
            .await
            .expect("reconnected packet receive deadline")
            .expect("packet received from replacement stream");
    assert_eq!(message.address, iface_address);
    assert_eq!(message.packet.data.as_slice(), b"tcp-reconnected-stream");
    assert!(runtime_status.to_json()["bytes_rx"].as_u64().is_some_and(|bytes| bytes > 0));

    cancel.cancel();
    drop(reconnected_stream);
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("TCP client task shutdown deadline")
        .expect("TCP client task");
    assert_eq!(runtime_status.to_json()["stream_state"].as_str(), Some("closed"));
}
