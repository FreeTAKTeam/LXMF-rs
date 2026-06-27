use std::sync::Arc;

use rns_transport::buffer::OutputBuffer;

use rns_transport::hash::AddressHash;

use rns_transport::iface::kiss::{
    run_kiss_stream, KissActivityProbeConfig, KissCommandFrame, KissIdBeaconConfig, KissInterface,
    KissPayloadAdapter, KissStreamOptions, KISS_FLOW_CONTROL_TIMEOUT, KISS_READ_FRAME_TIMEOUT,
};

use rns_transport::iface::{TxMessage, TxMessageType};

use rns_transport::kiss::{
    decode_frames, encode_command_frame, encode_data_frame, KissCommand, KissFrame,
    KissStreamDecoder, CMD_DATA, CMD_P, CMD_READY, CMD_SLOTTIME, CMD_TXDELAY, CMD_TXTAIL, FEND,
    FESC, TFEND, TFESC,
};

use rns_transport::packet::Packet;
use rns_transport::serde::Serialize;

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
            payload_adapter: KissPayloadAdapter::Raw,
            strip_command_port_nibble: true,
            command_tx: Some(command_tx),
            data_rx_tx: None,
            management_frame_rx: None,
            runtime_status: None,
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
            payload_adapter: KissPayloadAdapter::Raw,
            strip_command_port_nibble: false,
            command_tx: None,
            data_rx_tx: Some(data_rx_tx),
            management_frame_rx: None,
            runtime_status: None,
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
async fn run_kiss_stream_updates_runtime_status_for_data_rx_and_tx() {
    let (mut peer, stream) = tokio::io::duplex(512);
    let iface_address = AddressHash::new_from_slice(b"kiss-status");
    let (rx_send, mut rx_recv) = tokio::sync::mpsc::channel(1);
    let (tx_send, tx_recv) = tokio::sync::mpsc::channel(1);
    let tx_recv = Arc::new(tokio::sync::Mutex::new(tx_recv));
    let runtime_status = KissInterface::new("test-kiss", 1200).runtime_status_handle();
    let cancel = CancellationToken::new();

    let worker_cancel = cancel.clone();
    let worker = tokio::spawn(run_kiss_stream(
        stream,
        KissStreamOptions {
            iface_address,
            device: "test-kiss".to_string(),
            mtu: 256,
            flow_control: false,
            flow_control_timeout: KISS_FLOW_CONTROL_TIMEOUT,
            read_frame_timeout: KISS_READ_FRAME_TIMEOUT,
            initial_frames: Vec::new(),
            shutdown_frames: Vec::new(),
            id_beacon: None,
            activity_probe: None,
            payload_adapter: KissPayloadAdapter::Raw,
            strip_command_port_nibble: true,
            command_tx: None,
            data_rx_tx: None,
            management_frame_rx: None,
            runtime_status: Some(runtime_status.clone()),
        },
        worker_cancel,
        rx_send,
        tx_recv,
    ));

    tx_send
        .send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet: Packet::default() })
        .await
        .expect("send outbound packet");
    let mut tx_wire = [0_u8; 256];
    let tx_wire_len = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        tokio::io::AsyncReadExt::read(&mut peer, &mut tx_wire),
    )
    .await
    .expect("outbound kiss frame")
    .expect("read outbound kiss frame");
    assert!(tx_wire_len > 0);

    let mut packet_payload = [0_u8; 256];
    let mut output = OutputBuffer::new(&mut packet_payload);
    Packet::default().serialize(&mut output).expect("serialize inbound packet");
    let inbound_frame = encode_data_frame(output.as_slice());
    tokio::io::AsyncWriteExt::write_all(&mut peer, &inbound_frame)
        .await
        .expect("write inbound packet");
    let rx = tokio::time::timeout(std::time::Duration::from_secs(1), rx_recv.recv())
        .await
        .expect("rx packet")
        .expect("rx message");
    assert_eq!(rx.address, iface_address);

    let snapshot = runtime_status.snapshot();
    assert_eq!(snapshot.packets_tx, 1);
    assert_eq!(snapshot.data_frames_tx, 1);
    assert_eq!(snapshot.bytes_tx, tx_wire_len as u64);
    assert_eq!(snapshot.packets_rx, 1);
    assert_eq!(snapshot.data_frames_rx, 1);
    assert_eq!(snapshot.bytes_rx, inbound_frame.len() as u64);

    cancel.cancel();
    drop(peer);
    worker.await.expect("worker exits");
}

#[tokio::test]
async fn run_kiss_stream_writes_outbound_management_command_frames() {
    let (mut peer, stream) = tokio::io::duplex(256);
    let (rx_send, _rx_recv) = tokio::sync::mpsc::channel(1);
    let (_tx_send, tx_recv) = tokio::sync::mpsc::channel(1);
    let tx_recv = Arc::new(tokio::sync::Mutex::new(tx_recv));
    let (management_tx, management_rx) =
        tokio::sync::mpsc::channel(KISS_TEST_CALLBACK_CHANNEL_CAPACITY);
    let management_rx = Arc::new(tokio::sync::Mutex::new(management_rx));
    let cancel = CancellationToken::new();

    let worker_cancel = cancel.clone();
    let worker = tokio::spawn(run_kiss_stream(
        stream,
        KissStreamOptions {
            iface_address: AddressHash::default(),
            device: "test-rnode-management".to_string(),
            mtu: 64,
            flow_control: true,
            flow_control_timeout: KISS_FLOW_CONTROL_TIMEOUT,
            read_frame_timeout: KISS_READ_FRAME_TIMEOUT,
            initial_frames: Vec::new(),
            shutdown_frames: Vec::new(),
            id_beacon: None,
            activity_probe: None,
            payload_adapter: KissPayloadAdapter::Raw,
            strip_command_port_nibble: false,
            command_tx: None,
            data_rx_tx: None,
            management_frame_rx: Some(management_rx),
            runtime_status: None,
        },
        worker_cancel,
        rx_send,
        tx_recv,
    ));

    let radio_query_frame = encode_command_frame(0x06, &[0xff]);
    let blink_frame = encode_command_frame(0x30, &[0x03]);
    management_tx
        .send(radio_query_frame.clone())
        .await
        .expect("queue radio query management frame");
    management_tx.send(blink_frame.clone()).await.expect("queue blink management frame");

    let mut seen = Vec::new();
    let mut buffer = [0_u8; 256];
    for _ in 0..2 {
        let n = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            tokio::io::AsyncReadExt::read(&mut peer, &mut buffer),
        )
        .await
        .expect("management command frame")
        .expect("read management command");
        seen.extend_from_slice(&buffer[..n]);
        if seen.windows(radio_query_frame.len()).any(|window| window == radio_query_frame.as_slice())
            && seen.windows(blink_frame.len()).any(|window| window == blink_frame.as_slice())
        {
            break;
        }
    }
    assert!(
        seen.windows(radio_query_frame.len()).any(|window| window == radio_query_frame.as_slice()),
        "radio-state query frame missing from stream bytes: {seen:02x?}"
    );
    assert!(
        seen.windows(blink_frame.len()).any(|window| window == blink_frame.as_slice()),
        "blink frame missing from stream bytes: {seen:02x?}"
    );

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
            payload_adapter: KissPayloadAdapter::Raw,
            strip_command_port_nibble: true,
            command_tx: Some(command_tx),
            data_rx_tx: Some(data_rx_tx),
            management_frame_rx: None,
            runtime_status: None,
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
            payload_adapter: KissPayloadAdapter::Raw,
            strip_command_port_nibble: true,
            command_tx: None,
            data_rx_tx: None,
            management_frame_rx: None,
            runtime_status: None,
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
