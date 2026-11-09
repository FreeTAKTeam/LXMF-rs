use super::*;
use crate::hash::AddressHash;
use crate::iface::{encode_packet_ifac, InterfaceManager};
use crate::kiss::encode_data_frame;
use crate::packet::{Packet, PacketDataBuffer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

#[tokio::test]
async fn lora_stream_records_the_signal_stats_ahead_of_a_data_frame_against_it() {
    let mut manager = InterfaceManager::new(8);
    let interface = LoraInterface::new("software-only-lora", 115_200, LoraConfig::us915_default());
    let context = manager.new_context(interface);
    let address = *context.channel.address();
    let ifac_state = context.channel.ifac_state.clone();
    let ifac_violations = context.channel.ifac_violations.clone();
    let packet_signals = context.channel.packet_signals.clone();
    let receiver = manager.receiver();
    let (rx_channel, tx_channel) = context.channel.split();
    let (stream, mut peer) = tokio::io::duplex(4_096);
    let cancel = CancellationToken::new();
    let interface = context.inner.clone();
    let management_frame_rx =
        interface.lock().expect("LoRa interface mutex poisoned").management_frame_rx.clone();
    let config = LoraConfig::us915_default();
    let session = tokio::spawn(run_lora_kiss_stream(
        stream,
        LoraStreamRun {
            interface: interface.clone(),
            cancel: cancel.clone(),
            iface_stop: cancel.clone(),
            iface_address: address,
            endpoint_label: "in-memory LoRa signal".to_string(),
            config,
            flow_control: false,
            id_beacon: None,
            activity_probe: None,
            startup_response_timeout: Duration::from_secs(60),
            management_frame_rx,
            rx_channel,
            tx_channel: Arc::new(tokio::sync::Mutex::new(tx_channel)),
            ifac_state: ifac_state.clone(),
            ifac_violations,
            packet_signals: packet_signals.clone(),
        },
    ));
    // The stream is up once its startup commands have been written.
    let startup_len: usize = config.command_frames().iter().map(Vec::len).sum();
    let mut startup = vec![0; startup_len];
    timeout(Duration::from_secs(1), peer.read_exact(&mut startup))
        .await
        .expect("startup frames deadline")
        .expect("startup frames");

    let heard = Packet {
        destination: AddressHash::new_from_slice(&[0x71; 16]),
        data: PacketDataBuffer::new_from_slice(b"heard at -60 dBm"),
        ..Packet::default()
    };
    let silent = Packet {
        destination: AddressHash::new_from_slice(&[0x72; 16]),
        data: PacketDataBuffer::new_from_slice(b"no stats ahead of this one"),
        ..Packet::default()
    };
    let frame = |packet: &Packet| {
        encode_data_frame(&encode_packet_ifac(&ifac_state, packet).expect("encode packet"))
    };
    let mut bytes = encode_command_frame(CMD_STAT_RSSI, &[97]);
    bytes.extend(encode_command_frame(CMD_STAT_SNR, &[0xF8]));
    bytes.extend(frame(&heard));
    bytes.extend(frame(&silent));
    peer.write_all(&bytes).await.expect("write frames");

    for expected in [&heard, &silent] {
        let received = timeout(Duration::from_secs(1), async { receiver.lock().await.recv().await })
            .await
            .expect("packet deadline")
            .expect("rx channel open");
        assert_eq!(received.packet.destination, expected.destination);
    }
    assert_eq!(
        packet_signals.get(&heard.hash()),
        Some(PacketSignal { rssi: Some(-60.0), snr: Some(-2.0), q: None }),
        "the two stats ahead of the frame are recorded against it, in frame order"
    );
    assert_eq!(packet_signals.get(&silent.hash()), None, "a frame with no stats ahead carries none");
    let status = interface.lock().expect("LoRa interface mutex poisoned").radio_status();
    assert_eq!((status.rssi_dbm, status.snr_db), (None, None), "the frame took them");

    cancel.cancel();
    timeout(Duration::from_secs(2), session).await.expect("stream stops").expect("stream task");
}
