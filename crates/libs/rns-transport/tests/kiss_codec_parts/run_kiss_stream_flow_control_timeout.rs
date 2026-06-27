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
            payload_adapter: KissPayloadAdapter::Raw,
            strip_command_port_nibble: false,
            command_tx: None,
            data_rx_tx: None,
            management_frame_rx: None,
            runtime_status: None,
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
