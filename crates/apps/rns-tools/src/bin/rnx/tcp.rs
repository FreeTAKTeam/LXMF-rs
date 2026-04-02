use std::io;
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::{
    build_capture_command_payload, capture_profile_name_from_wire, embedded_to_io, hex_lower,
    parse_hex_16, resolve_runtime_seq, upload_attachment_via_rpc, CaptureProfileArg,
    NativeListenerMode, NativePeerMode, TcpBridgeMode,
};
use rns_embedded_core::{
    lxmf_min::{decode_envelope, encode_envelope, MinimalEnvelope},
    packet::{decode_frame, encode_frame, PacketFrame},
    transport::EmbeddedTransport,
};
use rns_embedded_runtime::{
    tcp::TcpEmbeddedTransport, FRAME_KIND_ANNOUNCE, FRAME_KIND_CAPTURE_ATTACHMENT_CHUNK,
    FRAME_KIND_CAPTURE_ATTACHMENT_DONE, FRAME_KIND_CAPTURE_COMMAND, FRAME_KIND_CAPTURE_RESULT,
    FRAME_KIND_LXMF_MESSAGE, FRAME_KIND_TEST_PING,
};
use rns_rpc::e2e_harness::timestamp_millis;

pub(crate) fn run_tcp_native_peer(
    addr: String,
    mode: NativePeerMode,
    runtime_seq: Option<u32>,
    payload: String,
    destination_hex: String,
    source_hex: String,
    timeout_secs: u64,
) -> io::Result<()> {
    let runtime_seq = resolve_runtime_seq(runtime_seq);
    let payload_bytes = payload.into_bytes();
    let runtime_frame = match mode {
        NativePeerMode::RawPing => {
            let frame = PacketFrame::new(FRAME_KIND_TEST_PING, runtime_seq, payload_bytes)
                .map_err(embedded_to_io)?;
            encode_frame(&frame).map_err(embedded_to_io)?
        }
        NativePeerMode::LxmfPing => {
            let source = parse_hex_16(source_hex.as_str())?;
            let destination = parse_hex_16(destination_hex.as_str())?;
            let envelope = MinimalEnvelope {
                source,
                destination,
                sequence: u64::from(runtime_seq),
                body: payload_bytes,
            };
            let frame = PacketFrame::new(
                FRAME_KIND_LXMF_MESSAGE,
                runtime_seq,
                encode_envelope(&envelope).map_err(embedded_to_io)?,
            )
            .map_err(embedded_to_io)?;
            encode_frame(&frame).map_err(embedded_to_io)?
        }
    };

    let socket_addr = addr.parse().map_err(|err| {
        io::Error::new(io::ErrorKind::InvalidInput, format!("invalid addr: {err}"))
    })?;
    let mut transport =
        TcpEmbeddedTransport::connect(socket_addr, u16::MAX).map_err(embedded_to_io)?;
    let frame = decode_frame(runtime_frame.as_slice()).map_err(embedded_to_io)?;
    transport.send_frame(&frame).map_err(embedded_to_io)?;

    let deadline = Instant::now() + Duration::from_secs(timeout_secs.max(1));
    let mut responses = 0usize;
    while Instant::now() < deadline {
        match transport.poll_frame().map_err(embedded_to_io)? {
            Some(frame) => match frame.kind {
                FRAME_KIND_ANNOUNCE => {
                    println!(
                        "TCP_NATIVE_PEER frame kind=0x{:02x} seq={} bytes={} role=announce",
                        frame.kind,
                        frame.sequence,
                        frame.payload.len()
                    );
                }
                FRAME_KIND_LXMF_MESSAGE => {
                    let envelope = decode_envelope(&frame.payload).map_err(embedded_to_io)?;
                    println!(
                        "TCP_NATIVE_PEER frame kind=0x{:02x} seq={} body={} source={} destination={}",
                        frame.kind,
                        frame.sequence,
                        String::from_utf8_lossy(&envelope.body),
                        hex_lower(&envelope.source),
                        hex_lower(&envelope.destination)
                    );
                    responses = responses.saturating_add(1);
                    if mode == NativePeerMode::LxmfPing && envelope.body.starts_with(b"pong:") {
                        break;
                    }
                }
                _ => {
                    println!(
                        "TCP_NATIVE_PEER frame kind=0x{:02x} seq={} payload_hex={}",
                        frame.kind,
                        frame.sequence,
                        hex_lower(&frame.payload)
                    );
                    responses = responses.saturating_add(1);
                    if mode == NativePeerMode::RawPing && frame.kind != FRAME_KIND_ANNOUNCE {
                        break;
                    }
                }
            },
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }

    println!("TCP_NATIVE_PEER ok: addr={} responses={} mode={:?}", addr, responses, mode);
    Ok(())
}

struct TcpSessionOutcome {
    responses: usize,
    lxmf_reply_body: Option<Vec<u8>>,
    capture_bytes: Option<Vec<u8>>,
}

fn handle_tcp_native_session(
    label: &str,
    peer_addr: std::net::SocketAddr,
    transport: &mut TcpEmbeddedTransport,
    mode_name: &str,
    deferred_outbound: Option<&PacketFrame>,
    repeat_until_capture_starts: bool,
    capture_out: Option<PathBuf>,
    print_summary: bool,
    timeout_secs: u64,
) -> io::Result<TcpSessionOutcome> {
    let deadline = Instant::now() + Duration::from_secs(timeout_secs.max(1));
    let mut outbound_sent = false;
    let mut responses = 0usize;
    let mut capture_bytes = Vec::new();
    let mut capture_total_bytes: Option<u32> = None;
    let mut capture_total_chunks: Option<u16> = None;
    let mut capture_started = false;
    let mut lxmf_reply_body: Option<Vec<u8>> = None;

    while Instant::now() < deadline {
        match transport.poll_frame().map_err(embedded_to_io)? {
            Some(frame) => match frame.kind {
                FRAME_KIND_ANNOUNCE => {
                    println!(
                        "{label} frame kind=0x{:02x} seq={} bytes={} role=announce",
                        frame.kind,
                        frame.sequence,
                        frame.payload.len()
                    );
                    if let Some(outbound) = deferred_outbound {
                        if !outbound_sent || (repeat_until_capture_starts && !capture_started) {
                            transport.send_frame(outbound).map_err(embedded_to_io)?;
                            println!(
                                "{label} sent request kind=0x{:02x} seq={} mode={}",
                                outbound.kind, outbound.sequence, mode_name
                            );
                            outbound_sent = true;
                        }
                    }
                }
                FRAME_KIND_LXMF_MESSAGE => {
                    let envelope = decode_envelope(&frame.payload).map_err(embedded_to_io)?;
                    println!(
                        "{label} frame kind=0x{:02x} seq={} body={} source={} destination={}",
                        frame.kind,
                        frame.sequence,
                        String::from_utf8_lossy(&envelope.body),
                        hex_lower(&envelope.source),
                        hex_lower(&envelope.destination)
                    );
                    lxmf_reply_body = Some(envelope.body.clone());
                    responses = responses.saturating_add(1);
                }
                FRAME_KIND_CAPTURE_RESULT => {
                    if frame.payload.len() < 11 {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "capture result payload too short",
                        ));
                    }
                    let status = frame.payload[0];
                    let total_bytes = u32::from_le_bytes([
                        frame.payload[1],
                        frame.payload[2],
                        frame.payload[3],
                        frame.payload[4],
                    ]);
                    let chunk_bytes = u16::from_le_bytes([frame.payload[5], frame.payload[6]]);
                    let width = u16::from_le_bytes([frame.payload[7], frame.payload[8]]);
                    let height = u16::from_le_bytes([frame.payload[9], frame.payload[10]]);
                    let effective_profile = (frame.payload.len() >= 12)
                        .then(|| capture_profile_name_from_wire(frame.payload[11]));
                    capture_total_bytes = Some(total_bytes);
                    capture_started = true;
                    if let Some(effective_profile) = effective_profile {
                        println!(
                            "{label} frame kind=0x{:02x} seq={} status={} total_bytes={} chunk_bytes={} width={} height={} profile={}",
                            frame.kind,
                            frame.sequence,
                            status,
                            total_bytes,
                            chunk_bytes,
                            width,
                            height,
                            effective_profile
                        );
                    } else {
                        println!(
                            "{label} frame kind=0x{:02x} seq={} status={} total_bytes={} chunk_bytes={} width={} height={}",
                            frame.kind, frame.sequence, status, total_bytes, chunk_bytes, width, height
                        );
                    }
                    if status != 0 {
                        responses = responses.saturating_add(1);
                        break;
                    }
                }
                FRAME_KIND_CAPTURE_ATTACHMENT_CHUNK => {
                    if frame.payload.len() < 6 {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "capture chunk payload too short",
                        ));
                    }
                    let seq = u16::from_le_bytes([frame.payload[0], frame.payload[1]]);
                    let total_chunks = u16::from_le_bytes([frame.payload[2], frame.payload[3]]);
                    let payload_len =
                        u16::from_le_bytes([frame.payload[4], frame.payload[5]]) as usize;
                    if frame.payload.len() != 6 + payload_len {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "capture chunk payload length mismatch",
                        ));
                    }
                    capture_total_chunks = Some(total_chunks);
                    capture_bytes.extend_from_slice(&frame.payload[6..]);
                    println!(
                        "{label} frame kind=0x{:02x} seq={} chunk_seq={} total_chunks={} payload_bytes={} collected_bytes={}",
                        frame.kind,
                        frame.sequence,
                        seq,
                        total_chunks,
                        payload_len,
                        capture_bytes.len()
                    );
                }
                FRAME_KIND_CAPTURE_ATTACHMENT_DONE => {
                    if frame.payload.len() < 6 {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "capture done payload too short",
                        ));
                    }
                    let total_chunks = u16::from_le_bytes([frame.payload[0], frame.payload[1]]);
                    let total_bytes = u32::from_le_bytes([
                        frame.payload[2],
                        frame.payload[3],
                        frame.payload[4],
                        frame.payload[5],
                    ]);
                    println!(
                        "{label} frame kind=0x{:02x} seq={} total_chunks={} total_bytes={}",
                        frame.kind, frame.sequence, total_chunks, total_bytes
                    );
                    if let Some(expected) = capture_total_bytes {
                        if expected != total_bytes {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                format!("capture total byte mismatch expected={expected} got={total_bytes}"),
                            ));
                        }
                    }
                    if let Some(expected) = capture_total_chunks {
                        if expected != total_chunks {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                format!("capture total chunk mismatch expected={expected} got={total_chunks}"),
                            ));
                        }
                    }
                    if capture_bytes.len() != usize::try_from(total_bytes).unwrap_or(usize::MAX) {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!(
                                "capture byte count mismatch collected={} expected={}",
                                capture_bytes.len(),
                                total_bytes
                            ),
                        ));
                    }
                    let path = capture_out.clone().unwrap_or_else(|| {
                        PathBuf::from(format!("capture-{}.jpg", timestamp_millis()))
                    });
                    std::fs::write(&path, &capture_bytes)?;
                    println!(
                        "{label} capture saved path={} bytes={}",
                        path.display(),
                        capture_bytes.len()
                    );
                    responses = responses.saturating_add(1);
                    break;
                }
                _ => {
                    println!(
                        "{label} frame kind=0x{:02x} seq={} payload_hex={}",
                        frame.kind,
                        frame.sequence,
                        hex_lower(&frame.payload)
                    );
                    responses = responses.saturating_add(1);
                }
            },
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }

    if print_summary {
        println!("{label} ok: peer={} responses={} mode={}", peer_addr, responses, mode_name);
    }
    Ok(TcpSessionOutcome {
        responses,
        lxmf_reply_body,
        capture_bytes: (!capture_bytes.is_empty()).then_some(capture_bytes),
    })
}

pub(crate) fn run_tcp_native_listener(
    bind: String,
    serve: bool,
    mode: NativeListenerMode,
    runtime_seq: Option<u32>,
    payload: String,
    destination_hex: String,
    source_hex: String,
    capture_out: Option<PathBuf>,
    capture_profile: CaptureProfileArg,
    timeout_secs: u64,
) -> io::Result<()> {
    let listener = TcpListener::bind(bind.as_str())?;
    println!("TCP_NATIVE_LISTENER listening bind={}", bind);

    let deferred_outbound = if mode != NativeListenerMode::Passive {
        let runtime_seq = resolve_runtime_seq(runtime_seq);
        let payload_bytes = payload.into_bytes();
        Some(match mode {
            NativeListenerMode::Passive => unreachable!(),
            NativeListenerMode::RawPing => {
                PacketFrame::new(FRAME_KIND_TEST_PING, runtime_seq, payload_bytes)
                    .map_err(embedded_to_io)?
            }
            NativeListenerMode::LxmfPing => {
                let source = parse_hex_16(source_hex.as_str())?;
                let destination = parse_hex_16(destination_hex.as_str())?;
                let envelope = MinimalEnvelope {
                    source,
                    destination,
                    sequence: u64::from(runtime_seq),
                    body: payload_bytes,
                };
                PacketFrame::new(
                    FRAME_KIND_LXMF_MESSAGE,
                    runtime_seq,
                    encode_envelope(&envelope).map_err(embedded_to_io)?,
                )
                .map_err(embedded_to_io)?
            }
            NativeListenerMode::Capture => PacketFrame::new(
                FRAME_KIND_CAPTURE_COMMAND,
                runtime_seq,
                build_capture_command_payload(runtime_seq, capture_profile),
            )
            .map_err(embedded_to_io)?,
        })
    } else {
        None
    };

    loop {
        let (stream, peer_addr) = listener.accept()?;
        println!("TCP_NATIVE_LISTENER accepted peer={}", peer_addr);
        let mut transport =
            TcpEmbeddedTransport::from_stream(stream, u16::MAX).map_err(embedded_to_io)?;
        let outcome = handle_tcp_native_session(
            "TCP_NATIVE_LISTENER",
            peer_addr,
            &mut transport,
            &format!("{mode:?}"),
            deferred_outbound.as_ref(),
            mode == NativeListenerMode::Capture,
            capture_out.clone(),
            true,
            timeout_secs,
        )?;
        if !serve || outcome.responses > 0 {
            break;
        }
    }
    Ok(())
}

pub(crate) fn run_tcp_native_bridge(
    bind: String,
    serve: bool,
    mode: TcpBridgeMode,
    runtime_seq: Option<u32>,
    payload: String,
    destination_hex: String,
    source_hex: String,
    rpc: String,
    content_type: String,
    capture_out: Option<PathBuf>,
    capture_profile: CaptureProfileArg,
    chunk_size: usize,
    timeout_secs: u64,
) -> io::Result<()> {
    let listener = TcpListener::bind(bind.as_str())?;
    println!("TCP_NATIVE_BRIDGE listening bind={} mode={:?}", bind, mode);

    let runtime_seq = resolve_runtime_seq(runtime_seq);
    let deferred_outbound = match mode {
        TcpBridgeMode::LxmfPing => {
            let source = parse_hex_16(source_hex.as_str())?;
            let destination = parse_hex_16(destination_hex.as_str())?;
            let envelope = MinimalEnvelope {
                source,
                destination,
                sequence: u64::from(runtime_seq),
                body: payload.into_bytes(),
            };
            Some(
                PacketFrame::new(
                    FRAME_KIND_LXMF_MESSAGE,
                    runtime_seq,
                    encode_envelope(&envelope).map_err(embedded_to_io)?,
                )
                .map_err(embedded_to_io)?,
            )
        }
        TcpBridgeMode::Capture => Some(
            PacketFrame::new(
                FRAME_KIND_CAPTURE_COMMAND,
                runtime_seq,
                build_capture_command_payload(runtime_seq, capture_profile),
            )
            .map_err(embedded_to_io)?,
        ),
    };

    loop {
        let (stream, peer_addr) = listener.accept()?;
        println!("TCP_NATIVE_BRIDGE accepted peer={}", peer_addr);
        let mut transport =
            TcpEmbeddedTransport::from_stream(stream, u16::MAX).map_err(embedded_to_io)?;
        let outcome = handle_tcp_native_session(
            "TCP_NATIVE_BRIDGE",
            peer_addr,
            &mut transport,
            &format!("{mode:?}"),
            deferred_outbound.as_ref(),
            mode == TcpBridgeMode::Capture,
            capture_out.clone(),
            false,
            timeout_secs,
        )?;

        let attachment_id = match mode {
            TcpBridgeMode::LxmfPing => {
                let body = outcome.lxmf_reply_body.ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::TimedOut,
                        "tcp bridge did not receive LXMF reply body",
                    )
                })?;
                upload_attachment_via_rpc(
                    rpc.as_str(),
                    "tcp-native-bridge.txt".to_string(),
                    content_type.clone(),
                    body.as_slice(),
                    chunk_size.max(1),
                )?
            }
            TcpBridgeMode::Capture => {
                let bytes = outcome.capture_bytes.ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::TimedOut,
                        "tcp bridge did not receive capture bytes",
                    )
                })?;
                upload_attachment_via_rpc(
                    rpc.as_str(),
                    "tcp-native-capture.jpg".to_string(),
                    content_type.clone(),
                    bytes.as_slice(),
                    chunk_size.max(1),
                )?
            }
        };
        println!(
            "TCP_NATIVE_BRIDGE ok: peer={} mode={:?} attachment_id={}",
            peer_addr, mode, attachment_id
        );
        if !serve {
            break;
        }
    }
    Ok(())
}
