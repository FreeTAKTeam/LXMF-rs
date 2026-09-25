use super::{run_serial_stream_with_ifac, SerialInterface, SerialStreamOptions};
use crate::buffer::OutputBuffer;
use crate::hash::AddressHash;
use crate::iface::{
    decode_packet_ifac, encode_packet_ifac, hdlc::Hdlc, IfacState, InterfaceManager,
    InterfaceSharedConfig, TxMessage, TxMessageType,
};
use crate::packet::{Packet, PacketDataBuffer};
use std::sync::{atomic::Ordering, Arc};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout};
use tokio_util::sync::CancellationToken;

fn authenticated_frame(ifac_state: &IfacState, packet: &Packet) -> Vec<u8> {
    let payload = encode_packet_ifac(ifac_state, packet).expect("encode IFAC packet");
    let mut wire = vec![0_u8; payload.len().saturating_mul(2).saturating_add(2)];
    let mut output = OutputBuffer::new(&mut wire[..]);
    let written = Hdlc::encode(&payload, &mut output).expect("encode HDLC frame");
    wire.truncate(written);
    wire
}

#[tokio::test]
async fn serial_stream_ifac_rejects_wrong_key_and_roundtrips_authenticated_packets() {
    let mut manager = InterfaceManager::new(8);
    let serial = SerialInterface::new("in-memory serial", 115_200);
    let runtime_status = serial.runtime_status_handle();
    let context = manager.new_context(serial);
    let address = *context.channel.address();
    let ifac_state = context.channel.ifac_state.clone();
    let ifac_violations = context.channel.ifac_violations.clone();

    let shared_config = InterfaceSharedConfig {
        ifac_size: Some(128),
        network_name: Some("serial-ifac-test".to_string()),
        passphrase: Some("serial-ifac-test-credential".to_string()),
        ..InterfaceSharedConfig::default()
    };
    assert!(manager.set_shared_config(address, shared_config));

    let wrong_config = InterfaceSharedConfig {
        ifac_size: Some(128),
        network_name: Some("serial-ifac-test".to_string()),
        passphrase: Some("wrong-serial-ifac-credential".to_string()),
        ..InterfaceSharedConfig::default()
    };
    let wrong_context = wrong_config
        .ifac_context_with_default_size(16)
        .expect("derive wrong-key IFAC context")
        .expect("wrong-key IFAC is enabled");
    let wrong_ifac_state: IfacState = Arc::new(std::sync::RwLock::new(Some(wrong_context)));

    let receiver = manager.receiver();
    let (rx_channel, tx_channel) = context.channel.split();
    let tx_channel = Arc::new(tokio::sync::Mutex::new(tx_channel));
    let (serial_stream, mut peer_stream) = tokio::io::duplex(4_096);
    let cancel = CancellationToken::new();
    let session = tokio::spawn(run_serial_stream_with_ifac(
        serial_stream,
        SerialStreamOptions {
            iface_address: address,
            device: "in-memory serial".to_string(),
            mtu: 512,
            cancel: cancel.clone(),
            rx_channel,
            tx_channel,
            runtime_status: runtime_status.clone(),
        },
        ifac_state.clone(),
        ifac_violations.clone(),
    ));

    let rejected_packet = Packet {
        destination: AddressHash::new_from_slice(&[0x51; 16]),
        data: PacketDataBuffer::new_from_slice(b"wrong IFAC key"),
        ..Packet::default()
    };
    peer_stream
        .write_all(&authenticated_frame(&wrong_ifac_state, &rejected_packet))
        .await
        .expect("write wrong-key serial frame");

    timeout(Duration::from_secs(1), async {
        while ifac_violations.load(Ordering::Relaxed) == 0 {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("wrong-key IFAC frame should increment the violation counter");
    sleep(Duration::from_millis(20)).await;
    assert!(
        receiver.lock().await.try_recv().is_err(),
        "wrong-key frame must not reach transport admission"
    );

    let inbound_packet = Packet {
        destination: AddressHash::new_from_slice(&[0x52; 16]),
        data: PacketDataBuffer::new_from_slice(b"authenticated serial ingress"),
        ..Packet::default()
    };
    peer_stream
        .write_all(&authenticated_frame(&ifac_state, &inbound_packet))
        .await
        .expect("write authenticated serial frame");

    let received = timeout(Duration::from_secs(1), async {
        loop {
            let result = {
                let mut receiver = receiver.lock().await;
                receiver.try_recv()
            };
            match result {
                Ok(message) => return message,
                Err(mpsc::error::TryRecvError::Empty) => sleep(Duration::from_millis(5)).await,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    panic!("serial transport receive queue disconnected")
                }
            }
        }
    })
    .await
    .expect("valid IFAC packet should be admitted");
    assert_eq!(received.address, address);
    assert_eq!(received.packet.destination, inbound_packet.destination);
    assert_eq!(received.packet.data.as_slice(), b"authenticated serial ingress");
    assert_eq!(received.packet.ifac.map(|ifac| ifac.length), Some(0));

    let outbound_packet = Packet {
        destination: AddressHash::new_from_slice(&[0x53; 16]),
        data: PacketDataBuffer::new_from_slice(b"authenticated serial egress"),
        ..Packet::default()
    };
    let expected_wire = authenticated_frame(&ifac_state, &outbound_packet);
    let trace = manager
        .send(TxMessage {
            tx_type: TxMessageType::Direct(address),
            packet: outbound_packet.clone(),
        })
        .await;
    assert_eq!(trace.sent_ifaces, 1);

    let mut actual_wire = vec![0_u8; expected_wire.len()];
    timeout(Duration::from_secs(1), peer_stream.read_exact(&mut actual_wire))
        .await
        .expect("serial IFAC egress timed out")
        .expect("read authenticated serial frame");
    assert_eq!(actual_wire, expected_wire);

    let mut decoded = vec![0_u8; 1_024];
    let mut decoded_output = OutputBuffer::new(&mut decoded[..]);
    Hdlc::decode(&actual_wire, &mut decoded_output).expect("decode outbound HDLC");
    let decoded_packet = decode_packet_ifac(&ifac_state, decoded_output.as_slice())
        .expect("decode outbound IFAC packet");
    assert_eq!(decoded_packet.destination, outbound_packet.destination);
    assert_eq!(decoded_packet.data.as_slice(), b"authenticated serial egress");
    assert_eq!(ifac_violations.load(Ordering::Relaxed), 1);

    cancel.cancel();
    timeout(Duration::from_secs(1), session)
        .await
        .expect("serial IFAC stream should stop after cancellation")
        .expect("join serial IFAC stream");

    let status = runtime_status.snapshot();
    assert_eq!(status.packets_rx, 1);
    assert_eq!(status.packets_tx, 1);
    assert_eq!(status.deserialize_errors, 1);
}
