use crate::hash::AddressHash;
use crate::iface::{
    decode_packet_ifac, encode_packet_ifac, IfacState, InterfaceManager, InterfaceSharedConfig,
    TxMessage, TxMessageType,
};
use crate::kiss::{encode_data_frame, KissFrame, KissStreamDecoder};
use crate::packet::{Packet, PacketDataBuffer};
use std::sync::atomic::Ordering;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout};

fn lora_ifac_context(config: InterfaceSharedConfig) -> IfacState {
    Arc::new(std::sync::RwLock::new(
        config.ifac_context_with_default_size(8).expect("derive LoRa IFAC context"),
    ))
}

fn lora_ifac_frame(state: &IfacState, packet: &Packet) -> Vec<u8> {
    encode_data_frame(&encode_packet_ifac(state, packet).expect("encode LoRa IFAC packet"))
}

#[tokio::test]
async fn lora_production_stream_rejects_wrong_ifac_and_authenticates_ingress_egress() {
    let mut manager = InterfaceManager::new(8);
    let interface = LoraInterface::new("software-only-lora", 115_200, LoraConfig::us915_default());
    let context = manager.new_context(interface);
    let address = *context.channel.address();
    let ifac_state = context.channel.ifac_state.clone();
    assert!(manager.set_shared_config(
        address,
        InterfaceSharedConfig {
            network_name: Some("lora-ifac-test".to_string()),
            passphrase: Some("lora-ifac-test-secret".to_string()),
            ..InterfaceSharedConfig::default()
        }
    ));

    let wrong_state = lora_ifac_context(InterfaceSharedConfig {
        network_name: Some("lora-ifac-test".to_string()),
        passphrase: Some("wrong-lora-ifac-secret".to_string()),
        ..InterfaceSharedConfig::default()
    });
    let receiver = manager.receiver();
    let ifac_violations = context.channel.ifac_violations.clone();
    let (rx_channel, tx_channel) = context.channel.split();
    let (stream, mut peer_stream) = tokio::io::duplex(4_096);
    let cancel = CancellationToken::new();
    let interface = context.inner.clone();
    let management_frame_rx = interface
        .lock()
        .expect("LoRa interface mutex poisoned")
        .management_frame_rx
        .clone();
    let session = tokio::spawn(run_lora_kiss_stream(
        stream,
        LoraStreamRun {
            interface,
            cancel: cancel.clone(),
            iface_address: address,
            endpoint_label: "in-memory LoRa raw-payload adapter".to_string(),
            config: LoraConfig::us915_default(),
            flow_control: false,
            id_beacon: None,
            activity_probe: None,
            startup_response_timeout: Duration::from_secs(60),
            management_frame_rx,
            rx_channel,
            tx_channel: Arc::new(tokio::sync::Mutex::new(tx_channel)),
            ifac_state: ifac_state.clone(),
            ifac_violations: ifac_violations.clone(),
        },
    ));

    let rejected_packet = Packet {
        destination: AddressHash::new_from_slice(&[0x71; 16]),
        data: PacketDataBuffer::new_from_slice(b"wrong LoRa IFAC key"),
        ..Packet::default()
    };
    peer_stream
        .write_all(&lora_ifac_frame(&wrong_state, &rejected_packet))
        .await
        .expect("write wrong-key LoRa KISS frame");
    timeout(Duration::from_secs(1), async {
        while ifac_violations.load(Ordering::Relaxed) == 0 {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("wrong-key LoRa frame increments IFAC violation counter");
    sleep(Duration::from_millis(20)).await;
    assert!(
        receiver.lock().await.try_recv().is_err(),
        "wrong-key LoRa frame must not reach transport admission"
    );

    let inbound_packet = Packet {
        destination: AddressHash::new_from_slice(&[0x72; 16]),
        data: PacketDataBuffer::new_from_slice(b"authenticated LoRa ingress"),
        ..Packet::default()
    };
    peer_stream
        .write_all(&lora_ifac_frame(&ifac_state, &inbound_packet))
        .await
        .expect("write authenticated LoRa KISS frame");
    let received = timeout(Duration::from_secs(1), async {
        loop {
            match receiver.lock().await.try_recv() {
                Ok(message) => return message,
                Err(mpsc::error::TryRecvError::Empty) => sleep(Duration::from_millis(5)).await,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    panic!("LoRa transport receive queue disconnected")
                }
            }
        }
    })
    .await
    .expect("authenticated LoRa packet should be admitted");
    assert_eq!(received.address, address);
    assert_eq!(received.packet.destination, inbound_packet.destination);
    assert_eq!(received.packet.data.as_slice(), b"authenticated LoRa ingress");
    assert_eq!(received.packet.ifac.map(|ifac| ifac.length), Some(0));

    let outbound_packet = Packet {
        destination: AddressHash::new_from_slice(&[0x73; 16]),
        data: PacketDataBuffer::new_from_slice(b"authenticated LoRa egress"),
        ..Packet::default()
    };
    let trace = manager
        .send(TxMessage {
            tx_type: TxMessageType::Direct(address),
            packet: outbound_packet.clone(),
        })
        .await;
    assert_eq!(trace.sent_ifaces, 1);
    let mut decoder = KissStreamDecoder::new(usize::from(LoraConfig::us915_default().max_payload_bytes))
        .with_command_port_nibble_stripping(false);
    let mut read_buffer = [0_u8; 512];
    let decoded_packet = timeout(Duration::from_secs(1), async {
        'read_frames: loop {
            let read = peer_stream.read(&mut read_buffer).await.expect("read LoRa stream bytes");
            assert_ne!(read, 0, "LoRa stream closed before authenticated egress");
            for frame in decoder.push_bytes(&read_buffer[..read]).expect("decode LoRa KISS frames") {
                if let KissFrame::Data(payload) = frame {
                    break 'read_frames decode_packet_ifac(&ifac_state, &payload)
                        .expect("decode outbound LoRa IFAC packet");
                }
            }
        }
    })
        .await
        .expect("LoRa IFAC egress timed out");
    assert_eq!(decoded_packet.destination, outbound_packet.destination);
    assert_eq!(decoded_packet.data.as_slice(), b"authenticated LoRa egress");
    assert_eq!(ifac_violations.load(Ordering::Relaxed), 1);

    cancel.cancel();
    timeout(Duration::from_secs(1), session)
        .await
        .expect("LoRa production stream should stop after cancellation")
        .expect("join LoRa production stream");
}
