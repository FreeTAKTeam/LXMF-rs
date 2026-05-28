use std::io;
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::tcp_session::handle_tcp_native_session;
use crate::{
    build_capture_command_payload, embedded_to_io, hex_lower, parse_hex_16, resolve_runtime_seq,
    upload_attachment_via_rpc, CaptureProfileArg, NativeListenerMode, NativePeerMode,
    TcpBridgeMode,
};
use rns_embedded_core::{
    lxmf_min::{decode_envelope, encode_envelope, MinimalEnvelope},
    packet::{decode_frame, encode_frame, PacketFrame},
    transport::EmbeddedTransport,
};
use rns_embedded_runtime::{
    tcp::TcpEmbeddedTransport, FRAME_KIND_ANNOUNCE, FRAME_KIND_CAPTURE_COMMAND,
    FRAME_KIND_LXMF_MESSAGE, FRAME_KIND_TEST_PING,
};

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
                    log::trace!(
                        "TCP_NATIVE_PEER frame kind=0x{:02x} seq={} bytes={} role=announce",
                        frame.kind,
                        frame.sequence,
                        frame.payload.len()
                    );
                }
                FRAME_KIND_LXMF_MESSAGE => {
                    let envelope = decode_envelope(&frame.payload).map_err(embedded_to_io)?;
                    log::trace!(
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
                    log::trace!(
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

    log::info!("TCP_NATIVE_PEER ok: addr={} responses={} mode={:?}", addr, responses, mode);
    Ok(())
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
    log::info!("TCP_NATIVE_LISTENER listening bind={}", bind);

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
        log::info!("TCP_NATIVE_LISTENER accepted peer={}", peer_addr);
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
    log::info!("TCP_NATIVE_BRIDGE listening bind={} mode={:?}", bind, mode);

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
        log::info!("TCP_NATIVE_BRIDGE accepted peer={}", peer_addr);
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
        log::info!(
            "TCP_NATIVE_BRIDGE ok: peer={} mode={:?} attachment_id={}",
            peer_addr,
            mode,
            attachment_id
        );
        if !serve {
            break;
        }
    }
    Ok(())
}
