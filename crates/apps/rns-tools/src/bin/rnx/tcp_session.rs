use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::{capture_profile_name_from_wire, embedded_to_io, hex_lower};
use rns_embedded_core::{
    lxmf_min::decode_envelope, packet::PacketFrame, transport::EmbeddedTransport,
};
use rns_embedded_runtime::{
    tcp::TcpEmbeddedTransport, FRAME_KIND_ANNOUNCE, FRAME_KIND_CAPTURE_ATTACHMENT_CHUNK,
    FRAME_KIND_CAPTURE_ATTACHMENT_DONE, FRAME_KIND_CAPTURE_RESULT, FRAME_KIND_LXMF_MESSAGE,
};
use rns_rpc::e2e_harness::timestamp_millis;

pub(crate) struct TcpSessionOutcome {
    pub(crate) responses: usize,
    pub(crate) lxmf_reply_body: Option<Vec<u8>>,
    pub(crate) capture_bytes: Option<Vec<u8>>,
}

pub(crate) fn handle_tcp_native_session(
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
                    log::trace!(
                        "{label} frame kind=0x{:02x} seq={} bytes={} role=announce",
                        frame.kind,
                        frame.sequence,
                        frame.payload.len()
                    );
                    if let Some(outbound) = deferred_outbound {
                        if !outbound_sent || (repeat_until_capture_starts && !capture_started) {
                            transport.send_frame(outbound).map_err(embedded_to_io)?;
                            log::trace!(
                                "{label} sent request kind=0x{:02x} seq={} mode={}",
                                outbound.kind,
                                outbound.sequence,
                                mode_name
                            );
                            outbound_sent = true;
                        }
                    }
                }
                FRAME_KIND_LXMF_MESSAGE => {
                    let envelope = decode_envelope(&frame.payload).map_err(embedded_to_io)?;
                    log::trace!(
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
                        log::trace!(
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
                        log::trace!(
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
                    log::trace!(
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
                    log::trace!(
                        "{label} frame kind=0x{:02x} seq={} total_chunks={} total_bytes={}",
                        frame.kind,
                        frame.sequence,
                        total_chunks,
                        total_bytes
                    );
                    if let Some(expected) = capture_total_bytes {
                        if expected != total_bytes {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                format!(
                                    "capture total byte mismatch expected={expected} got={total_bytes}"
                                ),
                            ));
                        }
                    }
                    if let Some(expected) = capture_total_chunks {
                        if expected != total_chunks {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                format!(
                                    "capture total chunk mismatch expected={expected} got={total_chunks}"
                                ),
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
                    log::info!(
                        "{label} capture saved path={} bytes={}",
                        path.display(),
                        capture_bytes.len()
                    );
                    responses = responses.saturating_add(1);
                    break;
                }
                _ => {
                    log::trace!(
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
        log::info!("{label} ok: peer={} responses={} mode={}", peer_addr, responses, mode_name);
    }
    Ok(TcpSessionOutcome {
        responses,
        lxmf_reply_body,
        capture_bytes: (!capture_bytes.is_empty()).then_some(capture_bytes),
    })
}
