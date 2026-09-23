use super::*;
use crate::hash::AddressHash;
use crate::iface::{IfacState, InterfaceManager, InterfaceSharedConfig, TxMessage, TxMessageType};
use crate::kiss::{encode_data_frame, KissFrame, KissStreamDecoder};
use crate::packet::{Packet, PacketDataBuffer};
use std::sync::{atomic::Ordering, Arc};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout};
use tokio_util::sync::CancellationToken;

fn make_ifac_state(config: InterfaceSharedConfig) -> IfacState {
    Arc::new(std::sync::RwLock::new(
        config.ifac_context_with_default_size(8).expect("derive IFAC context"),
    ))
}

fn kiss_ifac_frame(state: &IfacState, adapter: &KissPayloadAdapter, packet: &Packet) -> Vec<u8> {
    let payload = encode_packet_ifac(state, packet).expect("encode IFAC packet");
    encode_data_frame(&adapter.outbound(&payload))
}

#[tokio::test]
async fn kiss_stream_ifac_rejects_wrong_key_and_authenticates_ingress_egress() {
    let mut manager = InterfaceManager::new(8);
    let interface = KissTcpClientInterface::new("127.0.0.1:0");
    let runtime_status = interface.runtime_status_handle();
    let context = manager.new_context(interface);
    let address = *context.channel.address();
    let ifac_state = context.channel.ifac_state.clone();
    assert!(manager.set_shared_config(
        address,
        InterfaceSharedConfig {
            network_name: Some("kiss-ifac-test".to_string()),
            passphrase: Some("kiss-ifac-test-secret".to_string()),
            ..InterfaceSharedConfig::default()
        }
    ));
    assert_eq!(
        ifac_state.read().expect("IFAC state lock").as_ref().expect("configured IFAC").ifac_size(),
        8,
        "KISS uses the reference default 8-byte IFAC tag"
    );

    let wrong_state = make_ifac_state(InterfaceSharedConfig {
        network_name: Some("kiss-ifac-test".to_string()),
        passphrase: Some("wrong-kiss-ifac-secret".to_string()),
        ..InterfaceSharedConfig::default()
    });
    let adapter = KissPayloadAdapter::Ax25(
        Ax25KissPayloadConfig::new("N0CALL", 0).expect("AX.25 adapter config"),
    );
    let receiver = manager.receiver();
    let ifac_violations = context.channel.ifac_violations.clone();
    let (rx_channel, tx_channel) = context.channel.split();
    let tx_channel = Arc::new(tokio::sync::Mutex::new(tx_channel));
    let (stream, mut peer_stream) = tokio::io::duplex(4_096);
    let cancel = CancellationToken::new();
    let session = tokio::spawn(run_kiss_stream_with_ifac(
        stream,
        KissStreamOptions {
            iface_address: address,
            device: "in-memory KISS TCP".to_string(),
            mtu: 564,
            flow_control: false,
            flow_control_timeout: KISS_FLOW_CONTROL_TIMEOUT,
            read_frame_timeout: KISS_READ_FRAME_TIMEOUT,
            initial_frames: Vec::new(),
            shutdown_frames: Vec::new(),
            id_beacon: None,
            activity_probe: None,
            payload_adapter: adapter.clone(),
            strip_command_port_nibble: false,
            command_tx: None,
            data_rx_tx: None,
            management_frame_rx: None,
            runtime_status: Some(runtime_status.clone()),
        },
        cancel.clone(),
        rx_channel,
        tx_channel,
        ifac_state.clone(),
        ifac_violations.clone(),
    ));

    let rejected_packet = Packet {
        destination: AddressHash::new_from_slice(&[0x61; 16]),
        data: PacketDataBuffer::new_from_slice(b"wrong KISS IFAC key"),
        ..Packet::default()
    };
    peer_stream
        .write_all(&kiss_ifac_frame(&wrong_state, &adapter, &rejected_packet))
        .await
        .expect("write wrong-key KISS frame");
    timeout(Duration::from_secs(1), async {
        while ifac_violations.load(Ordering::Relaxed) == 0 {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("wrong-key KISS frame increments the IFAC violation counter");
    sleep(Duration::from_millis(20)).await;
    assert!(
        receiver.lock().await.try_recv().is_err(),
        "wrong-key KISS frame must not reach transport admission"
    );

    let inbound_packet = Packet {
        destination: AddressHash::new_from_slice(&[0x62; 16]),
        data: PacketDataBuffer::new_from_slice(b"authenticated KISS ingress"),
        ..Packet::default()
    };
    peer_stream
        .write_all(&kiss_ifac_frame(&ifac_state, &adapter, &inbound_packet))
        .await
        .expect("write authenticated KISS frame");
    let received = timeout(Duration::from_secs(1), async {
        loop {
            let result = receiver.lock().await.try_recv();
            match result {
                Ok(message) => return message,
                Err(mpsc::error::TryRecvError::Empty) => sleep(Duration::from_millis(5)).await,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    panic!("KISS transport receive queue disconnected")
                }
            }
        }
    })
    .await
    .expect("authenticated KISS packet should be admitted");
    assert_eq!(received.address, address);
    assert_eq!(received.packet.destination, inbound_packet.destination);
    assert_eq!(received.packet.data.as_slice(), b"authenticated KISS ingress");
    assert_eq!(received.packet.ifac.map(|ifac| ifac.length), Some(0));

    let outbound_packet = Packet {
        destination: AddressHash::new_from_slice(&[0x63; 16]),
        data: PacketDataBuffer::new_from_slice(b"authenticated KISS egress"),
        ..Packet::default()
    };
    let expected_wire = kiss_ifac_frame(&ifac_state, &adapter, &outbound_packet);
    let trace = manager
        .send(TxMessage {
            tx_type: TxMessageType::Direct(address),
            packet: outbound_packet.clone(),
        })
        .await;
    assert_eq!(trace.sent_ifaces, 1);
    let mut actual_wire = vec![0; expected_wire.len()];
    timeout(Duration::from_secs(1), peer_stream.read_exact(&mut actual_wire))
        .await
        .expect("KISS IFAC egress timed out")
        .expect("read authenticated KISS frame");
    assert_eq!(actual_wire, expected_wire);

    let mut decoder = KissStreamDecoder::new(564).with_command_port_nibble_stripping(false);
    let frames = decoder.push_bytes(&actual_wire).expect("decode KISS egress frame");
    let Some(KissFrame::Data(payload)) = frames.first() else {
        panic!("expected a KISS data frame")
    };
    let payload = adapter.inbound(payload).expect("strip AX.25 header");
    let decoded_packet = decode_packet_ifac(&ifac_state, &payload).expect("decode outbound IFAC");
    assert_eq!(decoded_packet.destination, outbound_packet.destination);
    assert_eq!(decoded_packet.data.as_slice(), b"authenticated KISS egress");
    assert_eq!(ifac_violations.load(Ordering::Relaxed), 1);

    cancel.cancel();
    timeout(Duration::from_secs(1), session)
        .await
        .expect("KISS IFAC stream should stop after cancellation")
        .expect("join KISS IFAC stream");

    let status = runtime_status.snapshot();
    assert_eq!(status.packets_rx, 1);
    assert_eq!(status.packets_tx, 1);
    assert_eq!(status.deserialize_errors, 1);
    assert_eq!(status.data_frames_rx, 2);
    assert_eq!(status.data_frames_tx, 1);
}
