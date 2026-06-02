use std::sync::Arc;

use rns_transport::hash::AddressHash;
use rns_transport::iface::kiss::{
    run_kiss_stream, KissActivityProbeConfig, KissCommandFrame, KissIdBeaconConfig,
    KissStreamOptions, KISS_FLOW_CONTROL_TIMEOUT, KISS_READ_FRAME_TIMEOUT,
};
use rns_transport::iface::{TxMessage, TxMessageType};
use rns_transport::kiss::{
    decode_frames, encode_command_frame, encode_data_frame, KissCommand, KissFrame,
    KissStreamDecoder, CMD_DATA, CMD_P, CMD_READY, CMD_SLOTTIME, CMD_TXDELAY, CMD_TXTAIL, FEND,
    FESC, TFEND, TFESC,
};
use rns_transport::packet::Packet;
use tokio_util::sync::CancellationToken;

const KISS_TEST_CALLBACK_CHANNEL_CAPACITY: usize = 8;

#[test]
fn encode_data_frame_escapes_fend_and_fesc() {
    let payload = [0x01, FEND, 0x02, FESC, 0x03];

    let frame = encode_data_frame(&payload);

    assert_eq!(frame, vec![FEND, CMD_DATA, 0x01, FESC, TFEND, 0x02, FESC, TFESC, 0x03, FEND]);
}

#[test]
fn encode_command_frame_escapes_payload() {
    let frame = encode_command_frame(CMD_P, &[0x40, FEND, FESC]);

    assert_eq!(frame, vec![FEND, CMD_P, 0x40, FESC, TFEND, FESC, TFESC, FEND]);
}

#[test]
fn kiss_modem_config_commands_match_reference_units() {
    let config = rns_transport::iface::kiss::KissConfig {
        preamble_ms: 350,
        tx_tail_ms: 20,
        persistence: 64,
        slot_time_ms: 20,
        flow_control: true,
        id_beacon: None,
    };

    assert_eq!(
        config.command_frames(),
        vec![
            vec![FEND, CMD_TXDELAY, 35, FEND],
            vec![FEND, CMD_TXTAIL, 2, FEND],
            vec![FEND, CMD_P, 64, FEND],
            vec![FEND, CMD_SLOTTIME, 2, FEND],
            vec![FEND, CMD_READY, 1, FEND],
        ]
    );
}

#[test]
fn kiss_modem_config_always_writes_python_ready_startup_command() {
    let config =
        rns_transport::iface::kiss::KissConfig { flow_control: false, ..Default::default() };

    assert!(
        config
            .command_frames()
            .contains(&vec![FEND, CMD_READY, 1, FEND]),
        "Python KISSInterface.setFlowControl writes CMD_READY during startup even when flow_control is false"
    );
}

#[test]
fn decode_data_frame_unescapes_payload() {
    let input = [FEND, CMD_DATA, 0x41, FESC, TFEND, FESC, TFESC, 0x42, FEND];

    let frames = decode_frames(&input, 64).expect("decode frame");

    assert_eq!(frames, vec![KissFrame::Data(vec![0x41, FEND, FESC, 0x42])]);
}

#[test]
fn decode_ready_frame_reports_flow_control_command() {
    let input = [FEND, CMD_READY, FEND];

    let frames = decode_frames(&input, 64).expect("decode ready frame");

    assert_eq!(frames, vec![KissFrame::Command(KissCommand::Ready)]);
}

#[test]
fn stream_decoder_can_strip_python_kiss_port_nibble() {
    let mut decoder = KissStreamDecoder::new(64).with_command_port_nibble_stripping(true);

    let frames = decoder
        .push_bytes(&[
            FEND,
            0x20 | CMD_DATA,
            b'p',
            b'o',
            b'r',
            b't',
            FEND,
            FEND,
            0x10 | CMD_READY,
            FEND,
        ])
        .expect("decode port-nibble frames");

    assert_eq!(
        frames,
        vec![KissFrame::Data(b"port".to_vec()), KissFrame::Command(KissCommand::Ready)]
    );
}

#[test]
fn default_stream_decoder_preserves_rnode_command_bytes() {
    let mut decoder = KissStreamDecoder::new(64);

    let frames =
        decoder.push_bytes(&[FEND, 0x50, 0x01, 0x4a, FEND]).expect("decode firmware command");

    assert_eq!(frames, vec![KissFrame::Command(KissCommand::Unknown(0x50, vec![0x01, 0x4a]))]);
}

#[test]
fn decode_multiple_frames_and_ignore_empty_boundaries() {
    let input = [FEND, FEND, CMD_DATA, b'a', FEND, FEND, CMD_DATA, b'b', FEND, FEND];

    let frames = decode_frames(&input, 64).expect("decode frames");

    assert_eq!(frames, vec![KissFrame::Data(vec![b'a']), KissFrame::Data(vec![b'b'])]);
}

#[test]
fn decode_unknown_escape_sequence_matches_python_literal_payload() {
    let input = [FEND, CMD_DATA, FESC, 0x00, FEND];

    let frames = decode_frames(&input, 64).expect("decode python-style unknown escape");

    assert_eq!(frames, vec![KissFrame::Data(vec![0x00])]);
}

#[test]
fn decode_trailing_escape_at_frame_end_matches_python_drop_escape() {
    let input = [FEND, CMD_DATA, FESC, FEND];

    let frames = decode_frames(&input, 64).expect("decode python-style trailing escape");

    assert_eq!(frames, vec![KissFrame::Data(vec![])]);
}

#[test]
fn stream_decoder_continues_after_python_lenient_escape_frame() {
    let mut decoder = KissStreamDecoder::new(64);

    let frames = decoder
        .push_bytes(&[FEND, CMD_DATA, FESC, 0x00, FEND])
        .expect("unknown escape should decode like Python");
    assert_eq!(frames, vec![KissFrame::Data(vec![0x00])]);

    let frames = decoder
        .push_bytes(&[FEND, CMD_DATA, b'o', b'k', FEND])
        .expect("decoder should recover for next frame");

    assert_eq!(frames, vec![KissFrame::Data(b"ok".to_vec())]);
}

#[test]
fn decode_oversized_payload_truncates_to_python_hw_mtu() {
    let input = [FEND, CMD_DATA, 0x01, 0x02, 0x03, FEND];

    let frames = decode_frames(&input, 2).expect("decode capped payload");

    assert_eq!(frames, vec![KissFrame::Data(vec![0x01, 0x02])]);
}

#[test]
fn stream_decoder_buffers_split_frames() {
    let mut decoder = KissStreamDecoder::new(64);

    assert!(decoder.push_bytes(&[FEND, CMD_DATA, b'p']).expect("partial decode").is_empty());
    let frames = decoder.push_bytes(&[b'i', b'n', b'g', FEND]).expect("finish decode");

    assert_eq!(frames, vec![KissFrame::Data(b"ping".to_vec())]);
}

#[tokio::test]
async fn run_kiss_stream_reports_unknown_command_frames() {
    let (mut peer, stream) = tokio::io::duplex(256);
    let (rx_send, _rx_recv) = tokio::sync::mpsc::channel(1);
    let (_tx_send, tx_recv) = tokio::sync::mpsc::channel(1);
    let tx_recv = Arc::new(tokio::sync::Mutex::new(tx_recv));
    let (command_tx, mut command_rx) =
        tokio::sync::mpsc::channel(KISS_TEST_CALLBACK_CHANNEL_CAPACITY);
    let cancel = CancellationToken::new();

    let worker_cancel = cancel.clone();
    let worker = tokio::spawn(run_kiss_stream(
        stream,
        KissStreamOptions {
            iface_address: AddressHash::default(),
            device: "test-kiss".to_string(),
            mtu: 64,
            flow_control: false,
            flow_control_timeout: KISS_FLOW_CONTROL_TIMEOUT,
            read_frame_timeout: KISS_READ_FRAME_TIMEOUT,
            initial_frames: Vec::new(),
            shutdown_frames: Vec::new(),
            id_beacon: None,
            activity_probe: None,
            strip_command_port_nibble: true,
            command_tx: Some(command_tx),
            data_rx_tx: None,
        },
        worker_cancel,
        rx_send,
        tx_recv,
    ));

    tokio::io::AsyncWriteExt::write_all(&mut peer, &encode_command_frame(0x12, &[1, 74]))
        .await
        .expect("write command");

    let command = tokio::time::timeout(std::time::Duration::from_secs(1), command_rx.recv())
        .await
        .expect("command callback")
        .expect("command frame");
    assert_eq!(command, KissCommandFrame { command: CMD_P, payload: vec![1, 74] });

    cancel.cancel();
    drop(peer);
    worker.await.expect("worker exits");
}

#[tokio::test]
async fn run_kiss_stream_reports_inbound_data_frames_for_status_hooks() {
    let (mut peer, stream) = tokio::io::duplex(256);
    let (rx_send, _rx_recv) = tokio::sync::mpsc::channel(1);
    let (_tx_send, tx_recv) = tokio::sync::mpsc::channel(1);
    let tx_recv = Arc::new(tokio::sync::Mutex::new(tx_recv));
    let (data_rx_tx, mut data_rx) = tokio::sync::mpsc::channel(KISS_TEST_CALLBACK_CHANNEL_CAPACITY);
    let cancel = CancellationToken::new();

    let worker_cancel = cancel.clone();
    let worker = tokio::spawn(run_kiss_stream(
        stream,
        KissStreamOptions {
            iface_address: AddressHash::default(),
            device: "test-rnode".to_string(),
            mtu: 64,
            flow_control: false,
            flow_control_timeout: KISS_FLOW_CONTROL_TIMEOUT,
            read_frame_timeout: KISS_READ_FRAME_TIMEOUT,
            initial_frames: Vec::new(),
            shutdown_frames: Vec::new(),
            id_beacon: None,
            activity_probe: None,
            strip_command_port_nibble: false,
            command_tx: None,
            data_rx_tx: Some(data_rx_tx),
        },
        worker_cancel,
        rx_send,
        tx_recv,
    ));

    tokio::io::AsyncWriteExt::write_all(&mut peer, &encode_data_frame(b"not-a-packet"))
        .await
        .expect("write data frame");

    tokio::time::timeout(std::time::Duration::from_secs(1), data_rx.recv())
        .await
        .expect("data callback")
        .expect("data frame notification");

    cancel.cancel();
    drop(peer);
    worker.await.expect("worker exits");
}

#[tokio::test]
async fn run_kiss_stream_drops_stale_partial_data_frame_after_python_read_timeout() {
    let (mut peer, stream) = tokio::io::duplex(256);
    let (rx_send, _rx_recv) = tokio::sync::mpsc::channel(1);
    let (_tx_send, tx_recv) = tokio::sync::mpsc::channel(1);
    let tx_recv = Arc::new(tokio::sync::Mutex::new(tx_recv));
    let (command_tx, mut command_rx) =
        tokio::sync::mpsc::channel(KISS_TEST_CALLBACK_CHANNEL_CAPACITY);
    let (data_rx_tx, mut data_rx) = tokio::sync::mpsc::channel(KISS_TEST_CALLBACK_CHANNEL_CAPACITY);
    let cancel = CancellationToken::new();

    let worker_cancel = cancel.clone();
    let worker = tokio::spawn(run_kiss_stream(
        stream,
        KissStreamOptions {
            iface_address: AddressHash::default(),
            device: "test-kiss-read-timeout".to_string(),
            mtu: 64,
            flow_control: false,
            flow_control_timeout: KISS_FLOW_CONTROL_TIMEOUT,
            read_frame_timeout: std::time::Duration::from_millis(30),
            initial_frames: Vec::new(),
            shutdown_frames: Vec::new(),
            id_beacon: None,
            activity_probe: None,
            strip_command_port_nibble: true,
            command_tx: Some(command_tx),
            data_rx_tx: Some(data_rx_tx),
        },
        worker_cancel,
        rx_send,
        tx_recv,
    ));

    tokio::io::AsyncWriteExt::write_all(&mut peer, &[FEND, CMD_DATA, b'x'])
        .await
        .expect("write stale partial data frame");
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    tokio::io::AsyncWriteExt::write_all(&mut peer, &encode_command_frame(0x12, &[1, 74]))
        .await
        .expect("write command after stale partial frame");

    let command = tokio::time::timeout(std::time::Duration::from_secs(1), command_rx.recv())
        .await
        .expect("command callback")
        .expect("command frame");
    assert_eq!(command, KissCommandFrame { command: CMD_P, payload: vec![1, 74] });

    let stale_data =
        tokio::time::timeout(std::time::Duration::from_millis(80), data_rx.recv()).await;
    assert!(stale_data.is_err(), "stale partial data frame should be dropped after timeout");

    cancel.cancel();
    drop(peer);
    worker.await.expect("worker exits");
}

#[tokio::test]
async fn run_kiss_stream_flow_control_allows_first_packet_after_python_configuration() {
    let (mut peer, stream) = tokio::io::duplex(1024);
    let (rx_send, _rx_recv) = tokio::sync::mpsc::channel(1);
    let (tx_send, tx_recv) = tokio::sync::mpsc::channel(1);
    let tx_recv = Arc::new(tokio::sync::Mutex::new(tx_recv));
    let cancel = CancellationToken::new();

    let worker_cancel = cancel.clone();
    let worker = tokio::spawn(run_kiss_stream(
        stream,
        KissStreamOptions {
            iface_address: AddressHash::default(),
            device: "test-kiss-flow".to_string(),
            mtu: 128,
            flow_control: true,
            flow_control_timeout: KISS_FLOW_CONTROL_TIMEOUT,
            read_frame_timeout: KISS_READ_FRAME_TIMEOUT,
            initial_frames: Vec::new(),
            shutdown_frames: Vec::new(),
            id_beacon: None,
            activity_probe: None,
            strip_command_port_nibble: true,
            command_tx: None,
            data_rx_tx: None,
        },
        worker_cancel,
        rx_send,
        tx_recv,
    ));

    tx_send
        .send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet: Packet::default() })
        .await
        .expect("send packet");

    let mut buffer = [0_u8; 1024];
    let first_read = tokio::time::timeout(
        std::time::Duration::from_millis(200),
        tokio::io::AsyncReadExt::read(&mut peer, &mut buffer),
    )
    .await
    .expect("first flow-control packet should not wait for READY")
    .expect("read first flow-control packet");
    assert!(
        decode_frames(&buffer[..first_read], 128)
            .expect("decode first flow-control packet")
            .iter()
            .any(|frame| matches!(frame, KissFrame::Data(_))),
        "first flow-control write should be a KISS data frame"
    );

    let no_second_read = tokio::time::timeout(
        std::time::Duration::from_millis(80),
        tokio::io::AsyncReadExt::read(&mut peer, &mut buffer),
    )
    .await;
    assert!(no_second_read.is_err(), "flow control should lock after first write");

    cancel.cancel();
    drop(peer);
    worker.await.expect("worker exits");
}

#[tokio::test]
async fn run_kiss_stream_flow_control_timeout_unlocks_missed_ready_like_python() {
    let (mut peer, stream) = tokio::io::duplex(1024);
    let (rx_send, _rx_recv) = tokio::sync::mpsc::channel(1);
    let (tx_send, tx_recv) = tokio::sync::mpsc::channel(2);
    let tx_recv = Arc::new(tokio::sync::Mutex::new(tx_recv));
    let cancel = CancellationToken::new();

    let worker_cancel = cancel.clone();
    let worker = tokio::spawn(run_kiss_stream(
        stream,
        KissStreamOptions {
            iface_address: AddressHash::default(),
            device: "test-kiss-flow-timeout".to_string(),
            mtu: 128,
            flow_control: true,
            flow_control_timeout: std::time::Duration::from_millis(30),
            read_frame_timeout: KISS_READ_FRAME_TIMEOUT,
            initial_frames: Vec::new(),
            shutdown_frames: Vec::new(),
            id_beacon: None,
            activity_probe: None,
            strip_command_port_nibble: true,
            command_tx: None,
            data_rx_tx: None,
        },
        worker_cancel,
        rx_send,
        tx_recv,
    ));

    tx_send
        .send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet: Packet::default() })
        .await
        .expect("send first packet");
    tx_send
        .send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet: Packet::default() })
        .await
        .expect("send second packet");

    let mut buffer = [0_u8; 1024];
    let first_read = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        tokio::io::AsyncReadExt::read(&mut peer, &mut buffer),
    )
    .await
    .expect("first packet")
    .expect("read first packet");
    assert!(
        decode_frames(&buffer[..first_read], 128)
            .expect("decode first packet")
            .iter()
            .any(|frame| matches!(frame, KissFrame::Data(_))),
        "first flow-control write should be a KISS data frame"
    );

    let second_read = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        tokio::io::AsyncReadExt::read(&mut peer, &mut buffer),
    )
    .await
    .expect("flow-control timeout should unlock missed READY")
    .expect("read timeout-unlocked packet");
    assert!(
        decode_frames(&buffer[..second_read], 128)
            .expect("decode timeout-unlocked packet")
            .iter()
            .any(|frame| matches!(frame, KissFrame::Data(_))),
        "timeout-unlocked write should be a KISS data frame"
    );

    cancel.cancel();
    drop(peer);
    worker.await.expect("worker exits");
}

#[tokio::test]
async fn run_kiss_stream_writes_activity_probe_after_idle_write_interval() {
    let (mut peer, stream) = tokio::io::duplex(1024);
    let (rx_send, _rx_recv) = tokio::sync::mpsc::channel(1);
    let (_tx_send, tx_recv) = tokio::sync::mpsc::channel(1);
    let tx_recv = Arc::new(tokio::sync::Mutex::new(tx_recv));
    let cancel = CancellationToken::new();

    let worker_cancel = cancel.clone();
    let worker = tokio::spawn(run_kiss_stream(
        stream,
        KissStreamOptions {
            iface_address: AddressHash::default(),
            device: "test-rnode-tcp".to_string(),
            mtu: 128,
            flow_control: false,
            flow_control_timeout: KISS_FLOW_CONTROL_TIMEOUT,
            read_frame_timeout: KISS_READ_FRAME_TIMEOUT,
            initial_frames: Vec::new(),
            shutdown_frames: Vec::new(),
            id_beacon: None,
            activity_probe: Some(KissActivityProbeConfig {
                interval: std::time::Duration::from_millis(20),
                frames: vec![encode_command_frame(0x08, &[0x73])],
            }),
            strip_command_port_nibble: false,
            command_tx: None,
            data_rx_tx: None,
        },
        worker_cancel,
        rx_send,
        tx_recv,
    ));

    let mut buffer = [0_u8; 1024];
    let probe_read = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        tokio::io::AsyncReadExt::read(&mut peer, &mut buffer),
    )
    .await
    .expect("activity probe frame")
    .expect("read activity probe");
    assert_eq!(&buffer[..probe_read], &encode_command_frame(0x08, &[0x73]));

    cancel.cancel();
    drop(peer);
    worker.await.expect("worker exits");
}

#[tokio::test]
async fn run_kiss_stream_transmits_id_beacon_after_first_data_tx() {
    let (mut peer, stream) = tokio::io::duplex(1024);
    let (rx_send, _rx_recv) = tokio::sync::mpsc::channel(1);
    let (tx_send, tx_recv) = tokio::sync::mpsc::channel(1);
    let tx_recv = Arc::new(tokio::sync::Mutex::new(tx_recv));
    let cancel = CancellationToken::new();

    let worker_cancel = cancel.clone();
    let worker = tokio::spawn(run_kiss_stream(
        stream,
        KissStreamOptions {
            iface_address: AddressHash::default(),
            device: "test-kiss".to_string(),
            mtu: 128,
            flow_control: false,
            flow_control_timeout: KISS_FLOW_CONTROL_TIMEOUT,
            read_frame_timeout: KISS_READ_FRAME_TIMEOUT,
            initial_frames: Vec::new(),
            shutdown_frames: Vec::new(),
            id_beacon: Some(KissIdBeaconConfig {
                callsign: b"MYCALL-0".to_vec(),
                interval: std::time::Duration::from_millis(20),
                min_payload_len: 0,
            }),
            activity_probe: None,
            strip_command_port_nibble: true,
            command_tx: None,
            data_rx_tx: None,
        },
        worker_cancel,
        rx_send,
        tx_recv,
    ));

    tx_send
        .send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet: Packet::default() })
        .await
        .expect("send packet");

    let mut buffer = [0_u8; 1024];
    let first_read = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        tokio::io::AsyncReadExt::read(&mut peer, &mut buffer),
    )
    .await
    .expect("first tx frame")
    .expect("read first tx frame");
    assert!(
        decode_frames(&buffer[..first_read], 128)
            .expect("decode first tx")
            .iter()
            .any(|frame| matches!(frame, KissFrame::Data(payload) if payload != b"MYCALL-0")),
        "first KISS data frame should be the actual packet"
    );

    let beacon_read = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        tokio::io::AsyncReadExt::read(&mut peer, &mut buffer),
    )
    .await
    .expect("beacon frame")
    .expect("read beacon frame");
    assert!(
        decode_frames(&buffer[..beacon_read], 128)
            .expect("decode beacon")
            .contains(&KissFrame::Data(b"MYCALL-0".to_vec())),
        "KISS ID beacon should be emitted as a raw data frame"
    );

    cancel.cancel();
    drop(peer);
    worker.await.expect("worker exits");
}

#[tokio::test]
async fn run_kiss_stream_pads_python_kiss_id_beacon_to_minimum_length() {
    let (mut peer, stream) = tokio::io::duplex(1024);
    let (rx_send, _rx_recv) = tokio::sync::mpsc::channel(1);
    let (tx_send, tx_recv) = tokio::sync::mpsc::channel(1);
    let tx_recv = Arc::new(tokio::sync::Mutex::new(tx_recv));
    let cancel = CancellationToken::new();

    let worker_cancel = cancel.clone();
    let worker = tokio::spawn(run_kiss_stream(
        stream,
        KissStreamOptions {
            iface_address: AddressHash::default(),
            device: "test-kiss".to_string(),
            mtu: 128,
            flow_control: false,
            flow_control_timeout: KISS_FLOW_CONTROL_TIMEOUT,
            read_frame_timeout: KISS_READ_FRAME_TIMEOUT,
            initial_frames: Vec::new(),
            shutdown_frames: Vec::new(),
            id_beacon: Some(KissIdBeaconConfig {
                callsign: b"MY".to_vec(),
                interval: std::time::Duration::from_millis(20),
                min_payload_len: 15,
            }),
            activity_probe: None,
            strip_command_port_nibble: true,
            command_tx: None,
            data_rx_tx: None,
        },
        worker_cancel,
        rx_send,
        tx_recv,
    ));

    tx_send
        .send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet: Packet::default() })
        .await
        .expect("send packet");

    let mut buffer = [0_u8; 1024];
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        tokio::io::AsyncReadExt::read(&mut peer, &mut buffer),
    )
    .await
    .expect("first tx frame")
    .expect("read first tx frame");

    let beacon_read = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        tokio::io::AsyncReadExt::read(&mut peer, &mut buffer),
    )
    .await
    .expect("beacon frame")
    .expect("read beacon frame");
    assert!(
        decode_frames(&buffer[..beacon_read], 128).expect("decode beacon").contains(
            &KissFrame::Data({
                let mut payload = b"MY".to_vec();
                payload.resize(15, 0);
                payload
            })
        ),
        "Python KISS ID beacon should be zero-padded to 15 bytes"
    );

    cancel.cancel();
    drop(peer);
    worker.await.expect("worker exits");
}

#[tokio::test]
async fn run_kiss_stream_writes_shutdown_frames_on_cancel() {
    let (mut peer, stream) = tokio::io::duplex(1024);
    let (rx_send, _rx_recv) = tokio::sync::mpsc::channel(1);
    let (_tx_send, tx_recv) = tokio::sync::mpsc::channel(1);
    let tx_recv = Arc::new(tokio::sync::Mutex::new(tx_recv));
    let cancel = CancellationToken::new();

    let worker_cancel = cancel.clone();
    let worker = tokio::spawn(run_kiss_stream(
        stream,
        KissStreamOptions {
            iface_address: AddressHash::default(),
            device: "test-kiss".to_string(),
            mtu: 128,
            flow_control: false,
            flow_control_timeout: KISS_FLOW_CONTROL_TIMEOUT,
            read_frame_timeout: KISS_READ_FRAME_TIMEOUT,
            initial_frames: Vec::new(),
            shutdown_frames: vec![encode_command_frame(0x0a, &[0xff])],
            id_beacon: None,
            activity_probe: None,
            strip_command_port_nibble: true,
            command_tx: None,
            data_rx_tx: None,
        },
        worker_cancel,
        rx_send,
        tx_recv,
    ));

    cancel.cancel();

    let mut buffer = [0_u8; 1024];
    let shutdown_read = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        tokio::io::AsyncReadExt::read(&mut peer, &mut buffer),
    )
    .await
    .expect("shutdown frame")
    .expect("read shutdown frame");
    assert_eq!(&buffer[..shutdown_read], &encode_command_frame(0x0a, &[0xff]));

    drop(peer);
    worker.await.expect("worker exits");
}
