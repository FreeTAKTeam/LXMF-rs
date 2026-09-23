use super::super::{run_i2p_peer_loop_with_ifac, I2pRuntimeStatus, I2pTunnelState};
use crate::buffer::OutputBuffer;
use crate::hash::AddressHash;
use crate::iface::{
    decode_packet_ifac, encode_packet_ifac, hdlc::Hdlc, IfacState, InterfaceSharedConfig,
    RxMessage, TxMessage, TxMessageType,
};
use crate::packet::{Packet, PacketDataBuffer};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

fn authenticated_hdlc_frame(ifac_state: &IfacState, packet: &Packet) -> Vec<u8> {
    let payload = encode_packet_ifac(ifac_state, packet).expect("encode IFAC packet");
    let mut wire = vec![0_u8; payload.len().saturating_mul(2).saturating_add(2)];
    let mut output = OutputBuffer::new(&mut wire);
    let written = Hdlc::encode(&payload, &mut output).expect("encode HDLC frame");
    wire.truncate(written);
    wire
}

#[tokio::test]
async fn i2p_peer_stream_ifac_rejects_wrong_key_and_roundtrips_authenticated_packets() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind fake SAM");
    let sam_addr = listener.local_addr().expect("SAM address").to_string();
    let shared_config = InterfaceSharedConfig {
        ifac_size: Some(128),
        network_name: Some("i2p-ifac-test".to_string()),
        passphrase: Some("i2p-test-credential".to_string()),
        ..InterfaceSharedConfig::default()
    };
    let ifac_state: IfacState = Arc::new(std::sync::RwLock::new(Some(
        shared_config
            .ifac_context_with_default_size(8)
            .expect("derive IFAC context")
            .expect("IFAC enabled"),
    )));
    let wrong_config = InterfaceSharedConfig {
        passphrase: Some("wrong-i2p-test-credential".to_string()),
        ..shared_config.clone()
    };
    let wrong_ifac_state: IfacState = Arc::new(std::sync::RwLock::new(Some(
        wrong_config
            .ifac_context_with_default_size(8)
            .expect("derive wrong-key IFAC context")
            .expect("wrong-key IFAC enabled"),
    )));

    let rejected_packet = Packet {
        destination: AddressHash::new([0x51; 16]),
        data: PacketDataBuffer::new_from_slice(b"wrong I2P IFAC key"),
        ..Packet::default()
    };
    let inbound_packet = Packet {
        destination: AddressHash::new([0x52; 16]),
        data: PacketDataBuffer::new_from_slice(b"authenticated I2P IFAC ingress"),
        ..Packet::default()
    };
    let outbound_packet = Packet {
        destination: AddressHash::new([0x53; 16]),
        data: PacketDataBuffer::new_from_slice(b"authenticated I2P IFAC egress"),
        ..Packet::default()
    };
    let wrong_frame = authenticated_hdlc_frame(&wrong_ifac_state, &rejected_packet);
    let inbound_frame = authenticated_hdlc_frame(&ifac_state, &inbound_packet);
    let expected_outbound = authenticated_hdlc_frame(&ifac_state, &outbound_packet);
    let (release_inbound_tx, release_inbound_rx) = oneshot::channel();
    let expected_outbound_len = expected_outbound.len();
    let server = tokio::spawn(async move {
        let (session_socket, _) = listener.accept().await.expect("accept SAM session");
        let mut session_reader = BufReader::new(session_socket);
        for response in [
            "HELLO REPLY RESULT=OK VERSION=3.3\n",
            "SESSION STATUS RESULT=OK DESTINATION=fake.b32.i2p\n",
        ] {
            let mut line = String::new();
            session_reader.read_line(&mut line).await.expect("read SAM session command");
            session_reader
                .get_mut()
                .write_all(response.as_bytes())
                .await
                .expect("write SAM session response");
        }

        let (lookup_socket, _) = listener.accept().await.expect("accept SAM lookup");
        let mut lookup_reader = BufReader::new(lookup_socket);
        for response in [
            "HELLO REPLY RESULT=OK VERSION=3.3\n",
            "NAMING REPLY RESULT=OK NAME=peer.b32.i2p VALUE=resolved-destination\n",
        ] {
            let mut line = String::new();
            lookup_reader.read_line(&mut line).await.expect("read SAM lookup command");
            lookup_reader
                .get_mut()
                .write_all(response.as_bytes())
                .await
                .expect("write SAM lookup response");
        }

        let (connect_socket, _) = listener.accept().await.expect("accept SAM stream");
        let mut connect_reader = BufReader::new(connect_socket);
        for response in ["HELLO REPLY RESULT=OK VERSION=3.3\n", "STREAM STATUS RESULT=OK\n"] {
            let mut line = String::new();
            connect_reader.read_line(&mut line).await.expect("read SAM connect command");
            connect_reader
                .get_mut()
                .write_all(response.as_bytes())
                .await
                .expect("write SAM connect response");
        }
        let stream = connect_reader.get_mut();
        stream.write_all(&wrong_frame).await.expect("write wrong-key frame");
        let _ = release_inbound_rx.await;
        stream.write_all(&inbound_frame).await.expect("write valid IFAC frame");
        let mut outbound = vec![0; expected_outbound_len];
        stream.read_exact(&mut outbound).await.expect("read authenticated egress");
        outbound
    });

    let peer = "peer.b32.i2p".to_string();
    let iface_address = AddressHash::new([0x33; 16]);
    let runtime_status = Arc::new(std::sync::Mutex::new(I2pRuntimeStatus::new(
        sam_addr.clone(),
        false,
        std::slice::from_ref(&peer),
    )));
    let cancel = CancellationToken::new();
    let iface_stop = CancellationToken::new();
    let (rx_channel, mut rx_messages) = mpsc::channel::<RxMessage>(8);
    let (peer_tx, peer_rx) = mpsc::channel::<TxMessage>(8);
    let ifac_violations = Arc::new(AtomicU64::new(0));
    let peer_loop = tokio::spawn(run_i2p_peer_loop_with_ifac(
        peer.clone(),
        iface_address,
        sam_addr,
        None,
        super::super::I2pInterface::DEFAULT_MTU,
        Duration::from_millis(10),
        Arc::clone(&runtime_status),
        cancel.clone(),
        iface_stop.clone(),
        rx_channel,
        peer_rx,
        ifac_state.clone(),
        ifac_violations.clone(),
    ));

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let connected = {
                let status = runtime_status.lock().expect("I2P runtime status");
                status.peers[&peer].state == I2pTunnelState::Connected
            };
            if connected {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("I2P peer connected through fake SAM");

    tokio::time::timeout(Duration::from_secs(1), async {
        while ifac_violations.load(Ordering::Relaxed) == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("wrong-key IFAC frame was rejected");
    assert!(rx_messages.try_recv().is_err(), "wrong-key frame must not reach packet admission");
    release_inbound_tx.send(()).expect("fake SAM waits for valid-frame release");

    let received = tokio::time::timeout(Duration::from_secs(1), rx_messages.recv())
        .await
        .expect("authenticated ingress timed out")
        .expect("I2P receive channel closed");
    assert_eq!(received.address, iface_address);
    assert_eq!(received.packet.destination, inbound_packet.destination);
    assert_eq!(received.packet.data.as_slice(), b"authenticated I2P IFAC ingress");
    assert_eq!(received.packet.ifac.map(|ifac| ifac.length), Some(0));

    peer_tx
        .send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet: outbound_packet.clone() })
        .await
        .expect("queue authenticated egress");
    let actual_outbound = tokio::time::timeout(Duration::from_secs(1), server)
        .await
        .expect("fake SAM egress timed out")
        .expect("fake SAM server task");
    assert_eq!(actual_outbound, expected_outbound);
    let mut decoded_frame = vec![0; actual_outbound.len()];
    let mut output = OutputBuffer::new(&mut decoded_frame);
    Hdlc::decode(&actual_outbound, &mut output).expect("decode egress HDLC");
    let decoded = decode_packet_ifac(&ifac_state, output.as_slice()).expect("decode egress IFAC");
    assert_eq!(decoded.destination, outbound_packet.destination);
    assert_eq!(decoded.data.as_slice(), b"authenticated I2P IFAC egress");
    assert_eq!(ifac_violations.load(Ordering::Relaxed), 1);

    cancel.cancel();
    iface_stop.cancel();
    tokio::time::timeout(Duration::from_secs(1), peer_loop)
        .await
        .expect("I2P peer loop shutdown timed out")
        .expect("I2P peer loop task");
}
