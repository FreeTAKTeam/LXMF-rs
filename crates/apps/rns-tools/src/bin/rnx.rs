use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use clap::{Parser, ValueEnum};
use rns_embedded_core::{
    lxmf_min::{MinimalEnvelope, decode_envelope, encode_envelope},
    packet::{PacketFrame, decode_frame, encode_frame},
};
use rns_embedded_runtime::{
    BLE_FRAME_NATIVE_WIRE, FRAME_KIND_ANNOUNCE, FRAME_KIND_LXMF_MESSAGE, FRAME_KIND_TEST_PING,
};
use rns_rpc::e2e_harness::{
    build_daemon_args, build_http_post, build_rpc_frame, build_send_params,
    build_tcp_client_config, is_ready_line, parse_http_response_body, parse_rpc_frame,
    timestamp_millis,
};
use rns_rpc::rpc::replay::{execute_trace, load_trace_file, save_capture_file};
use std::collections::HashSet;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command as ProcessCommand, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use sha2::{Digest, Sha256};
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use btleplug::api::{Central, CharPropFlags, Manager as _, Peripheral as _, ScanFilter, WriteType};
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use btleplug::platform::{Manager, Peripheral};
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use futures::StreamExt;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use uuid::Uuid;

#[derive(Parser, Debug)]
#[command(name = "rnx")]
struct Cli {
    #[arg(long)]
    config: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand, Debug)]
enum Command {
    E2e {
        #[arg(long, default_value_t = 4243)]
        a_port: u16,
        #[arg(long, default_value_t = 4244)]
        b_port: u16,
        #[arg(long, default_value_t = 60)]
        timeout_secs: u64,
        #[arg(long, default_value_t = false)]
        keep: bool,
        #[arg(long = "mode", value_enum)]
        modes: Vec<DeliveryMode>,
    },
    MeshSim {
        #[arg(long, default_value_t = 5)]
        nodes: usize,
        #[arg(long, default_value_t = 4340)]
        base_rpc_port: u16,
        #[arg(long, default_value_t = 90)]
        timeout_secs: u64,
        #[arg(long, default_value_t = false)]
        keep: bool,
        #[arg(long = "mode", value_enum)]
        modes: Vec<DeliveryMode>,
    },
    Replay {
        #[arg(long)]
        trace: PathBuf,
        #[arg(long)]
        capture_out: Option<PathBuf>,
        #[arg(long, default_value = "replay-identity")]
        identity_hash: String,
    },
    CameraUpload {
        #[arg(long, default_value = "127.0.0.1:4243")]
        rpc: String,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, default_value = "image/jpeg")]
        content_type: String,
        #[arg(long, default_value_t = 8192)]
        chunk_size: usize,
    },
    CameraCaptureUpload {
        #[arg(long, default_value = "127.0.0.1:4243")]
        rpc: String,
        #[arg(long)]
        peripheral_id: String,
        #[arg(long)]
        service_uuid: String,
        #[arg(long)]
        write_char_uuid: String,
        #[arg(long)]
        notify_char_uuid: String,
        #[arg(long, default_value = "image/jpeg")]
        content_type: String,
        #[arg(long, default_value_t = 8192)]
        chunk_size: usize,
        #[arg(long, default_value_t = 20)]
        timeout_secs: u64,
    },
    BleScan {
        #[arg(long, default_value_t = 10)]
        timeout_secs: u64,
        #[arg(long, default_value_t = 0)]
        limit: usize,
        #[arg(long)]
        service_uuid: Option<String>,
        #[arg(long)]
        manufacturer_prefix: Option<String>,
    },
    BleFindCamera {
        #[arg(long, default_value_t = 12)]
        scan_secs: u64,
        #[arg(long, default_value = "LXMF")]
        name_hint: String,
        #[arg(long)]
        service_uuid: String,
        #[arg(long)]
        write_char_uuid: String,
        #[arg(long)]
        notify_char_uuid: String,
    },
    BleNativePeer {
        #[arg(long, default_value_t = 12)]
        scan_secs: u64,
        #[arg(long, default_value = "LXMF")]
        name_hint: String,
        #[arg(long)]
        peripheral_id: Option<String>,
        #[arg(long)]
        service_uuid: String,
        #[arg(long)]
        write_char_uuid: String,
        #[arg(long)]
        notify_char_uuid: String,
        #[arg(long, value_enum, default_value_t = NativePeerMode::LxmfPing)]
        mode: NativePeerMode,
        #[arg(long)]
        runtime_seq: Option<u32>,
        #[arg(long, default_value = "ping")]
        payload: String,
        #[arg(long, default_value = "22222222222222222222222222222222")]
        destination_hex: String,
        #[arg(long, default_value = "99999999999999999999999999999999")]
        source_hex: String,
        #[arg(long, default_value_t = 8)]
        timeout_secs: u64,
    },
    BleNativeBridge {
        #[arg(long, default_value_t = 12)]
        scan_secs: u64,
        #[arg(long, default_value = "LXMF")]
        name_hint: String,
        #[arg(long)]
        peripheral_id: Option<String>,
        #[arg(long)]
        service_uuid: String,
        #[arg(long)]
        write_char_uuid: String,
        #[arg(long)]
        notify_char_uuid: String,
        #[arg(long, default_value = "127.0.0.1:4243")]
        rpc: String,
        #[arg(long)]
        runtime_seq: Option<u32>,
        #[arg(long, default_value = "bridge-ping")]
        payload: String,
        #[arg(long, default_value = "22222222222222222222222222222222")]
        destination_hex: String,
        #[arg(long, default_value = "99999999999999999999999999999999")]
        source_hex: String,
        #[arg(long, default_value_t = 8)]
        timeout_secs: u64,
        #[arg(long, default_value = "text/plain")]
        content_type: String,
    },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum, Hash)]
enum DeliveryMode {
    Direct,
    Opportunistic,
    Propagated,
    Paper,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum NativePeerMode {
    RawPing,
    LxmfPing,
}

fn main() {
    let cli = Cli::parse();
    if let Err(err) = run(cli) {
        eprintln!("rnx error: {}", err);
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> io::Result<()> {
    match cli.command {
        Command::E2e { a_port, b_port, timeout_secs, keep, modes } => {
            run_e2e(a_port, b_port, timeout_secs, keep, modes)
        }
        Command::MeshSim { nodes, base_rpc_port, timeout_secs, keep, modes } => {
            run_mesh_sim(nodes, base_rpc_port, timeout_secs, keep, modes)
        }
        Command::Replay { trace, capture_out, identity_hash } => {
            run_replay(trace, capture_out, identity_hash)
        }
        Command::CameraUpload { rpc, file, name, content_type, chunk_size } => {
            run_camera_upload(rpc, file, name, content_type, chunk_size)
        }
        Command::CameraCaptureUpload {
            rpc,
            peripheral_id,
            service_uuid,
            write_char_uuid,
            notify_char_uuid,
            content_type,
            chunk_size,
            timeout_secs,
        } => run_camera_capture_upload(
            rpc,
            peripheral_id,
            service_uuid,
            write_char_uuid,
            notify_char_uuid,
            content_type,
            chunk_size,
            timeout_secs,
        ),
        Command::BleScan { timeout_secs, limit, service_uuid, manufacturer_prefix } => {
            run_ble_scan(timeout_secs, limit, service_uuid, manufacturer_prefix)
        }
        Command::BleFindCamera {
            scan_secs,
            name_hint,
            service_uuid,
            write_char_uuid,
            notify_char_uuid,
        } => {
            run_ble_find_camera(
                scan_secs,
                name_hint,
                service_uuid,
                write_char_uuid,
                notify_char_uuid,
            )
        }
        Command::BleNativePeer {
            scan_secs,
            name_hint,
            peripheral_id,
            service_uuid,
            write_char_uuid,
            notify_char_uuid,
            mode,
            runtime_seq,
            payload,
            destination_hex,
            source_hex,
            timeout_secs,
        } => run_ble_native_peer(
            scan_secs,
            name_hint,
            peripheral_id,
            service_uuid,
            write_char_uuid,
            notify_char_uuid,
            mode,
            runtime_seq,
            payload,
            destination_hex,
            source_hex,
            timeout_secs,
        ),
        Command::BleNativeBridge {
            scan_secs,
            name_hint,
            peripheral_id,
            service_uuid,
            write_char_uuid,
            notify_char_uuid,
            rpc,
            runtime_seq,
            payload,
            destination_hex,
            source_hex,
            timeout_secs,
            content_type,
        } => run_ble_native_bridge(
            scan_secs,
            name_hint,
            peripheral_id,
            service_uuid,
            write_char_uuid,
            notify_char_uuid,
            rpc,
            runtime_seq,
            payload,
            destination_hex,
            source_hex,
            timeout_secs,
            content_type,
        ),
    }
}

fn run_camera_upload(
    rpc: String,
    file: PathBuf,
    name: Option<String>,
    content_type: String,
    chunk_size: usize,
) -> io::Result<()> {
    if chunk_size == 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "--chunk-size must be > 0"));
    }
    let payload = fs::read(&file)?;
    if payload.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "input file is empty"));
    }
    let attachment_name = name.or_else(|| {
        file.file_name().and_then(|value| value.to_str()).map(ToOwned::to_owned)
    });
    let attachment_name = attachment_name.unwrap_or_else(|| "camera-capture.bin".to_string());

    let attachment_id = upload_attachment_via_rpc(
        rpc.as_str(),
        attachment_name,
        content_type,
        payload.as_slice(),
        chunk_size,
    )?;

    println!(
        "CAMERA_UPLOAD ok: file={} bytes={} chunk_size={} attachment_id={}",
        file.display(),
        payload.len(),
        chunk_size,
        attachment_id
    );
    Ok(())
}

fn upload_attachment_via_rpc(
    rpc: &str,
    attachment_name: String,
    content_type: String,
    payload: &[u8],
    chunk_size: usize,
) -> io::Result<String> {
    let checksum_sha256 = sha256_hex(payload);
    let mut req_id = 1_u64;
    let start_response = rpc_call(
        rpc,
        req_id,
        "sdk_attachment_upload_start_v2",
        Some(serde_json::json!({
            "name": attachment_name,
            "content_type": content_type,
            "total_size": payload.len(),
            "checksum_sha256": checksum_sha256,
            "topic_ids": [],
            "extensions": {}
        })),
    )?;
    let start_result = ensure_rpc_ok(start_response, "sdk_attachment_upload_start_v2")?
        .ok_or_else(|| io::Error::other("upload_start missing result"))?;
    let upload = start_result
        .get("upload")
        .ok_or_else(|| io::Error::other("upload_start missing upload object"))?;
    let upload_id = upload
        .get("upload_id")
        .and_then(|value| value.as_str())
        .ok_or_else(|| io::Error::other("upload_start missing upload_id"))?
        .to_string();
    let chunk_size_hint = upload
        .get("chunk_size_hint")
        .and_then(|value| value.as_u64())
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(65_536);

    let effective_chunk_size = chunk_size.min(chunk_size_hint).max(1);
    let mut next_offset = upload
        .get("next_offset")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    req_id = req_id.wrapping_add(1);

    while usize::try_from(next_offset).ok().is_some_and(|offset| offset < payload.len()) {
        let start = usize::try_from(next_offset)
            .map_err(|_| io::Error::other("upload offset overflow"))?;
        let end = start.saturating_add(effective_chunk_size).min(payload.len());
        let bytes_base64 = BASE64_STANDARD.encode(&payload[start..end]);
        let chunk_response = rpc_call(
            rpc,
            req_id,
            "sdk_attachment_upload_chunk_v2",
            Some(serde_json::json!({
                "upload_id": upload_id,
                "offset": next_offset,
                "bytes_base64": bytes_base64,
                "extensions": {}
            })),
        )?;
        let chunk_result = ensure_rpc_ok(chunk_response, "sdk_attachment_upload_chunk_v2")?
            .ok_or_else(|| io::Error::other("upload_chunk missing result"))?;
        let upload_chunk = chunk_result
            .get("upload_chunk")
            .ok_or_else(|| io::Error::other("upload_chunk missing object"))?;
        let accepted =
            upload_chunk.get("accepted").and_then(|value| value.as_bool()).unwrap_or(false);
        if !accepted {
            return Err(io::Error::other("upload_chunk returned accepted=false"));
        }
        let returned_next_offset = upload_chunk
            .get("next_offset")
            .and_then(|value| value.as_u64())
            .ok_or_else(|| io::Error::other("upload_chunk missing next_offset"))?;
        if returned_next_offset <= next_offset {
            return Err(io::Error::other("upload_chunk did not advance next_offset"));
        }
        next_offset = returned_next_offset;
        req_id = req_id.wrapping_add(1);
    }

    let commit_response = rpc_call(
        rpc,
        req_id,
        "sdk_attachment_upload_commit_v2",
        Some(serde_json::json!({
            "upload_id": upload_id,
            "extensions": {}
        })),
    )?;
    let commit_result = ensure_rpc_ok(commit_response, "sdk_attachment_upload_commit_v2")?
        .ok_or_else(|| io::Error::other("upload_commit missing result"))?;
    let attachment_id = commit_result
        .get("attachment")
        .and_then(|value| value.get("attachment_id"))
        .and_then(|value| value.as_str())
        .unwrap_or("<unknown>");
    Ok(attachment_id.to_string())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn run_camera_capture_upload(
    rpc: String,
    peripheral_id: String,
    service_uuid: String,
    write_char_uuid: String,
    notify_char_uuid: String,
    content_type: String,
    chunk_size: usize,
    timeout_secs: u64,
) -> io::Result<()> {
    if chunk_size == 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "--chunk-size must be > 0"));
    }
    let timeout = Duration::from_secs(timeout_secs.max(1));
    let bytes = capture_camera_over_ble(
        peripheral_id.as_str(),
        service_uuid.as_str(),
        write_char_uuid.as_str(),
        notify_char_uuid.as_str(),
        timeout,
    )?;
    if bytes.is_empty() {
        return Err(io::Error::other("camera capture returned empty payload"));
    }

    let name = format!("capture-{}.jpg", timestamp_millis());
    let attachment_id =
        upload_attachment_via_rpc(rpc.as_str(), name, content_type, bytes.as_slice(), chunk_size)?;
    println!(
        "CAMERA_CAPTURE_UPLOAD ok: bytes={} attachment_id={}",
        bytes.len(),
        attachment_id
    );
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn run_camera_capture_upload(
    _rpc: String,
    _peripheral_id: String,
    _service_uuid: String,
    _write_char_uuid: String,
    _notify_char_uuid: String,
    _content_type: String,
    _chunk_size: usize,
    _timeout_secs: u64,
) -> io::Result<()> {
    Err(io::Error::other(
        "camera-capture-upload is only supported on linux/macos/windows",
    ))
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn capture_camera_over_ble(
    peripheral_id: &str,
    service_uuid: &str,
    write_char_uuid: &str,
    notify_char_uuid: &str,
    timeout: Duration,
) -> io::Result<Vec<u8>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(io::Error::other)?;
    runtime.block_on(async {
        let manager = Manager::new().await.map_err(io::Error::other)?;
        let adapters = manager.adapters().await.map_err(io::Error::other)?;
        let adapter = adapters
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::other("no BLE adapter available"))?;
        adapter.start_scan(ScanFilter::default()).await.map_err(io::Error::other)?;

        let service_uuid = parse_gatt_uuid(service_uuid)?;
        let write_uuid = parse_gatt_uuid(write_char_uuid)?;
        let notify_uuid = parse_gatt_uuid(notify_char_uuid)?;
        let peripheral = match find_peripheral(&adapter, peripheral_id, Some(service_uuid), timeout).await {
            Ok(peripheral) => peripheral,
            Err(error) => {
                #[cfg(target_os = "macos")]
                {
                    return Err(io::Error::other(format!(
                        "{error}; on macOS use `rnx ble-find-camera` first and pass the returned id"
                    )));
                }
                #[cfg(not(target_os = "macos"))]
                {
                    find_camera_peripheral_by_profile(
                        &adapter,
                        peripheral_id,
                        service_uuid,
                        write_uuid,
                        notify_uuid,
                        timeout,
                    )
                    .await?
                }
            }
        };
        if !peripheral.is_connected().await.map_err(io::Error::other)? {
            peripheral.connect().await.map_err(io::Error::other)?;
        }
        peripheral.discover_services().await.map_err(io::Error::other)?;

        let characteristics = peripheral.characteristics();
        let write_char = characteristics
            .iter()
            .find(|ch| ch.uuid == write_uuid && ch.service_uuid == service_uuid)
            .cloned()
            .ok_or_else(|| io::Error::other("write characteristic not found"))?;
        let notify_char = characteristics
            .iter()
            .find(|ch| ch.uuid == notify_uuid && ch.service_uuid == service_uuid)
            .cloned()
            .ok_or_else(|| io::Error::other("notify characteristic not found"))?;

        let write_type = if write_char.properties.contains(CharPropFlags::WRITE) {
            WriteType::WithResponse
        } else if write_char.properties.contains(CharPropFlags::WRITE_WITHOUT_RESPONSE) {
            WriteType::WithoutResponse
        } else {
            return Err(io::Error::other("write characteristic has no write capability"));
        };

        let mut notifications = peripheral.notifications().await.map_err(io::Error::other)?;
        peripheral.subscribe(&notify_char).await.map_err(io::Error::other)?;

        const FRAME_CAPTURE_REQ: u8 = 0x02;
        const FRAME_CAPTURE_ACK: u8 = 0x03;
        const FRAME_CHUNK: u8 = 0x04;
        const FRAME_CHUNK_ACK: u8 = 0x05;
        const FRAME_DONE: u8 = 0x06;
        const FRAME_ERROR: u8 = 0x07;
        const FRAME_NACK: u8 = 0x08;

        peripheral
            .write(&write_char, &[FRAME_CAPTURE_REQ], write_type)
            .await
            .map_err(io::Error::other)?;

        let deadline = Instant::now() + timeout;
        let mut transfer_id: Option<u32> = None;
        let mut expected_seq: u16 = 0;
        let mut bytes = Vec::new();

        loop {
            if Instant::now() >= deadline {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "camera capture timed out"));
            }

            let remaining = deadline.saturating_duration_since(Instant::now());
            let notification = tokio::time::timeout(remaining, notifications.next())
                .await
                .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "capture notification timeout"))?
                .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "notification stream closed"))?;

            if notification.uuid != notify_uuid || notification.value.is_empty() {
                continue;
            }

            let frame = notification.value;
            match frame[0] {
                FRAME_CAPTURE_ACK => continue,
                FRAME_ERROR => {
                    let message = String::from_utf8_lossy(&frame[1..]).to_string();
                    return Err(io::Error::other(format!("camera error: {message}")));
                }
                FRAME_DONE => break,
                FRAME_CHUNK => {
                    if frame.len() < 15 {
                        continue;
                    }
                    let fid = u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]);
                    let seq = u16::from_le_bytes([frame[5], frame[6]]);
                    let _total = u16::from_le_bytes([frame[7], frame[8]]);
                    let payload_len = u16::from_le_bytes([frame[9], frame[10]]) as usize;
                    let _crc32 = u32::from_le_bytes([frame[11], frame[12], frame[13], frame[14]]);
                    let payload = &frame[15..];
                    if payload.len() != payload_len {
                        continue;
                    }
                    if transfer_id.is_none() {
                        transfer_id = Some(fid);
                    }
                    if transfer_id != Some(fid) {
                        continue;
                    }
                    if seq == expected_seq {
                        bytes.extend_from_slice(payload);
                        let mut ack = Vec::with_capacity(7);
                        ack.push(FRAME_CHUNK_ACK);
                        ack.extend_from_slice(&fid.to_le_bytes());
                        ack.extend_from_slice(&seq.to_le_bytes());
                        peripheral.write(&write_char, &ack, write_type).await.map_err(io::Error::other)?;
                        expected_seq = expected_seq.saturating_add(1);
                    } else if seq < expected_seq {
                        let mut ack = Vec::with_capacity(7);
                        ack.push(FRAME_CHUNK_ACK);
                        ack.extend_from_slice(&fid.to_le_bytes());
                        ack.extend_from_slice(&seq.to_le_bytes());
                        peripheral.write(&write_char, &ack, write_type).await.map_err(io::Error::other)?;
                    } else {
                        let mut nack = Vec::with_capacity(7);
                        nack.push(FRAME_NACK);
                        nack.extend_from_slice(&fid.to_le_bytes());
                        nack.extend_from_slice(&expected_seq.to_le_bytes());
                        peripheral.write(&write_char, &nack, write_type).await.map_err(io::Error::other)?;
                    }
                }
                _ => {}
            }
        }

        let _ = peripheral.unsubscribe(&notify_char).await;
        let _ = peripheral.disconnect().await;
        Ok(bytes)
    })
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn run_ble_scan(
    timeout_secs: u64,
    limit: usize,
    service_uuid: Option<String>,
    manufacturer_prefix: Option<String>,
) -> io::Result<()> {
    let timeout = Duration::from_secs(timeout_secs.max(1));
    let service_filter = match service_uuid {
        Some(value) => Some(parse_gatt_uuid(value.as_str())?),
        None => None,
    };
    let manufacturer_filter = manufacturer_prefix
        .as_deref()
        .map(normalize_identifier);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(io::Error::other)?;
    runtime.block_on(async move {
        let manager = Manager::new().await.map_err(io::Error::other)?;
        let adapters = manager.adapters().await.map_err(io::Error::other)?;
        let adapter = adapters
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::other("no BLE adapter available"))?;
        adapter.start_scan(ScanFilter::default()).await.map_err(io::Error::other)?;
        tokio::time::sleep(timeout).await;

        let peripherals = adapter.peripherals().await.map_err(io::Error::other)?;
        let mut shown = 0usize;
        for peripheral in peripherals {
            let id = peripheral.id().to_string();
            let properties = peripheral.properties().await.map_err(io::Error::other)?;
            let Some(properties) = properties else {
                continue;
            };
            if let Some(expected_service) = service_filter {
                if !properties.services.iter().any(|service| *service == expected_service) {
                    continue;
                }
            }
            if let Some(expected_marker) = manufacturer_filter.as_deref() {
                let mut marker_match = false;
                for payload in properties.manufacturer_data.values() {
                    let hex = hex_lower(payload);
                    let ascii = ascii_lower(payload);
                    if hex.contains(expected_marker) || ascii.contains(expected_marker) {
                        marker_match = true;
                        break;
                    }
                }
                if !marker_match {
                    continue;
                }
            }
            let manufacturer = format_manufacturer_data(&properties.manufacturer_data);
            println!(
                "BLE_SCAN device id={} name={} address={} rssi={:?} services={:?} manufacturer={}",
                id,
                properties.local_name.as_deref().unwrap_or("<none>"),
                properties.address,
                properties.rssi,
                properties.services,
                manufacturer
            );
            shown = shown.saturating_add(1);
            if limit > 0 && shown >= limit {
                break;
            }
        }
        println!("BLE_SCAN done: devices_shown={shown}");
        Ok(())
    })
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn run_ble_find_camera(
    scan_secs: u64,
    name_hint: String,
    service_uuid: String,
    write_char_uuid: String,
    notify_char_uuid: String,
) -> io::Result<()> {
    let scan_timeout = Duration::from_secs(scan_secs.max(1));
    let service_uuid = parse_gatt_uuid(service_uuid.as_str())?;
    let write_uuid = parse_gatt_uuid(write_char_uuid.as_str())?;
    let notify_uuid = parse_gatt_uuid(notify_char_uuid.as_str())?;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(io::Error::other)?;
    runtime.block_on(async move {
        let hint_norm = normalize_identifier(name_hint.as_str());
        let deadline = Instant::now() + scan_timeout;
        let manager = Manager::new().await.map_err(io::Error::other)?;
        let adapters = manager.adapters().await.map_err(io::Error::other)?;
        let adapter = adapters
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::other("no BLE adapter available"))?;
        adapter.start_scan(ScanFilter::default()).await.map_err(io::Error::other)?;

        while Instant::now() < deadline {
            let peripherals = adapter.peripherals().await.map_err(io::Error::other)?;
            let mut candidates: Vec<(u8, i16, Peripheral, String)> = Vec::with_capacity(peripherals.len());
            for peripheral in peripherals {
                let mut rank = 3_u8;
                let mut rssi = -127_i16;
                let mut name = "<none>".to_string();
                if let Some(props) = peripheral.properties().await.map_err(io::Error::other)? {
                    rssi = props.rssi.unwrap_or(-127);
                    if let Some(local_name) = props.local_name {
                        name = local_name;
                        let norm = normalize_identifier(name.as_str());
                        if !hint_norm.is_empty() && norm.contains(&hint_norm) {
                            rank = 0;
                        } else if norm.contains("lxmf") {
                            rank = 1;
                        } else if norm != "<none>" {
                            rank = 2;
                        }
                    }
                }
                candidates.push((rank, rssi, peripheral, name));
            }
            candidates.sort_by(|(rank_a, rssi_a, _, _), (rank_b, rssi_b, _, _)| {
                rank_a.cmp(rank_b).then(rssi_b.cmp(rssi_a))
            });

            for (_, rssi, peripheral, name) in candidates {
                let id = peripheral.id().to_string();
                let connected = peripheral.is_connected().await.map_err(io::Error::other)?;
                if !connected {
                    if tokio::time::timeout(Duration::from_millis(650), peripheral.connect())
                        .await
                        .is_err()
                    {
                        continue;
                    }
                }
                if peripheral.discover_services().await.is_err() {
                    let _ = peripheral.disconnect().await;
                    continue;
                }
                let chars = peripheral.characteristics();
                let has_write = chars
                    .iter()
                    .any(|ch| ch.uuid == write_uuid && ch.service_uuid == service_uuid);
                let has_notify = chars
                    .iter()
                    .any(|ch| ch.uuid == notify_uuid && ch.service_uuid == service_uuid);

                if has_write && has_notify {
                    println!("BLE_FIND_CAMERA match id={} name={} rssi={}", id, name, rssi);
                    let _ = peripheral.disconnect().await;
                    return Ok(());
                }
                let _ = peripheral.disconnect().await;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }

        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no scanned peripheral matched requested camera profile",
        ))
    })
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn run_ble_native_peer(
    scan_secs: u64,
    name_hint: String,
    peripheral_id: Option<String>,
    service_uuid: String,
    write_char_uuid: String,
    notify_char_uuid: String,
    mode: NativePeerMode,
    runtime_seq: Option<u32>,
    payload: String,
    destination_hex: String,
    source_hex: String,
    timeout_secs: u64,
) -> io::Result<()> {
    let scan_timeout = Duration::from_secs(scan_secs.max(1));
    let timeout = Duration::from_secs(timeout_secs.max(1));
    let service_uuid = parse_gatt_uuid(service_uuid.as_str())?;
    let write_uuid = parse_gatt_uuid(write_char_uuid.as_str())?;
    let notify_uuid = parse_gatt_uuid(notify_char_uuid.as_str())?;
    let device_hint = peripheral_id.unwrap_or_else(|| name_hint.clone());
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

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(io::Error::other)?;
    runtime.block_on(async move {
        let manager = Manager::new().await.map_err(io::Error::other)?;
        let adapters = manager.adapters().await.map_err(io::Error::other)?;
        let adapter = adapters
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::other("no BLE adapter available"))?;
        adapter.start_scan(ScanFilter::default()).await.map_err(io::Error::other)?;
        let peripheral: Peripheral =
            find_peripheral(&adapter, device_hint.as_str(), Some(service_uuid), scan_timeout)
                .await?;

        let connected = peripheral.is_connected().await.map_err(io::Error::other)?;
        if !connected {
            peripheral.connect().await.map_err(io::Error::other)?;
        }
        peripheral.discover_services().await.map_err(io::Error::other)?;
        let characteristics = peripheral.characteristics();
        let write_char = characteristics
            .iter()
            .find(|ch| ch.uuid == write_uuid && ch.service_uuid == service_uuid)
            .cloned()
            .ok_or_else(|| io::Error::other("write characteristic not found"))?;
        let notify_char = characteristics
            .iter()
            .find(|ch| ch.uuid == notify_uuid && ch.service_uuid == service_uuid)
            .cloned()
            .ok_or_else(|| io::Error::other("notify characteristic not found"))?;
        let write_type = if write_char.properties.contains(CharPropFlags::WRITE) {
            WriteType::WithResponse
        } else if write_char.properties.contains(CharPropFlags::WRITE_WITHOUT_RESPONSE) {
            WriteType::WithoutResponse
        } else {
            return Err(io::Error::other("write characteristic has no write capability"));
        };

        let mut notifications = peripheral.notifications().await.map_err(io::Error::other)?;
        peripheral.subscribe(&notify_char).await.map_err(io::Error::other)?;

        let mut outbound = Vec::with_capacity(1 + runtime_frame.len());
        outbound.push(BLE_FRAME_NATIVE_WIRE);
        outbound.extend_from_slice(&runtime_frame);
        peripheral
            .write(&write_char, outbound.as_slice(), write_type)
            .await
            .map_err(io::Error::other)?;

        let deadline = Instant::now() + timeout;
        let mut responses = 0usize;
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let notification = match tokio::time::timeout(remaining, notifications.next()).await {
                Ok(Some(notification)) => notification,
                Ok(None) => break,
                Err(_) => break,
            };
            if notification.uuid != notify_uuid || notification.value.is_empty() {
                continue;
            }
            if notification.value[0] != BLE_FRAME_NATIVE_WIRE {
                continue;
            }
            let frame = decode_frame(&notification.value[1..]).map_err(embedded_to_io)?;
            match frame.kind {
                FRAME_KIND_ANNOUNCE => {
                    println!(
                        "BLE_NATIVE_PEER frame kind=0x{:02x} seq={} bytes={} role=announce",
                        frame.kind,
                        frame.sequence,
                        frame.payload.len()
                    );
                }
                FRAME_KIND_LXMF_MESSAGE => {
                    let envelope = decode_envelope(&frame.payload).map_err(embedded_to_io)?;
                    println!(
                        "BLE_NATIVE_PEER frame kind=0x{:02x} seq={} body={} source={} destination={}",
                        frame.kind,
                        frame.sequence,
                        String::from_utf8_lossy(&envelope.body),
                        hex_lower(&envelope.source),
                        hex_lower(&envelope.destination)
                    );
                    responses = responses.saturating_add(1);
                    if mode == NativePeerMode::LxmfPing
                        && envelope.body.starts_with(b"pong:")
                    {
                        break;
                    }
                }
                _ => {
                    println!(
                        "BLE_NATIVE_PEER frame kind=0x{:02x} seq={} payload_hex={}",
                        frame.kind,
                        frame.sequence,
                        hex_lower(&frame.payload)
                    );
                    responses = responses.saturating_add(1);
                    if mode == NativePeerMode::RawPing && frame.kind != FRAME_KIND_ANNOUNCE {
                        break;
                    }
                }
            }
        }

        let _ = peripheral.unsubscribe(&notify_char).await;
        let _ = peripheral.disconnect().await;
        println!(
            "BLE_NATIVE_PEER ok: device_id={} responses={} mode={:?}",
            peripheral.id(),
            responses,
            mode
        );
        Ok(())
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn run_ble_native_peer(
    _scan_secs: u64,
    _name_hint: String,
    _peripheral_id: Option<String>,
    _service_uuid: String,
    _write_char_uuid: String,
    _notify_char_uuid: String,
    _mode: NativePeerMode,
    _runtime_seq: Option<u32>,
    _payload: String,
    _destination_hex: String,
    _source_hex: String,
    _timeout_secs: u64,
) -> io::Result<()> {
    Err(io::Error::other(
        "ble-native-peer is only supported on linux/macos/windows",
    ))
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn run_ble_native_bridge(
    scan_secs: u64,
    name_hint: String,
    peripheral_id: Option<String>,
    service_uuid: String,
    write_char_uuid: String,
    notify_char_uuid: String,
    rpc: String,
    runtime_seq: Option<u32>,
    payload: String,
    destination_hex: String,
    source_hex: String,
    timeout_secs: u64,
    content_type: String,
) -> io::Result<()> {
    let scan_timeout = Duration::from_secs(scan_secs.max(1));
    let timeout = Duration::from_secs(timeout_secs.max(1));
    let service_uuid = parse_gatt_uuid(service_uuid.as_str())?;
    let write_uuid = parse_gatt_uuid(write_char_uuid.as_str())?;
    let notify_uuid = parse_gatt_uuid(notify_char_uuid.as_str())?;
    let device_hint = peripheral_id.unwrap_or_else(|| name_hint.clone());
    let runtime_seq = resolve_runtime_seq(runtime_seq);
    let source = parse_hex_16(source_hex.as_str())?;
    let destination = parse_hex_16(destination_hex.as_str())?;
    let envelope = MinimalEnvelope {
        source,
        destination,
        sequence: u64::from(runtime_seq),
        body: payload.clone().into_bytes(),
    };
    let runtime_frame = encode_frame(
        &PacketFrame::new(
            FRAME_KIND_LXMF_MESSAGE,
            runtime_seq,
            encode_envelope(&envelope).map_err(embedded_to_io)?,
        )
        .map_err(embedded_to_io)?,
    )
    .map_err(embedded_to_io)?;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(io::Error::other)?;
    runtime.block_on(async move {
        let manager = Manager::new().await.map_err(io::Error::other)?;
        let adapters = manager.adapters().await.map_err(io::Error::other)?;
        let adapter = adapters
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::other("no BLE adapter available"))?;
        adapter.start_scan(ScanFilter::default()).await.map_err(io::Error::other)?;
        let peripheral: Peripheral =
            find_peripheral(&adapter, device_hint.as_str(), Some(service_uuid), scan_timeout)
                .await?;

        let connected = peripheral.is_connected().await.map_err(io::Error::other)?;
        if !connected {
            peripheral.connect().await.map_err(io::Error::other)?;
        }
        peripheral.discover_services().await.map_err(io::Error::other)?;
        let characteristics = peripheral.characteristics();
        let write_char = characteristics
            .iter()
            .find(|ch| ch.uuid == write_uuid && ch.service_uuid == service_uuid)
            .cloned()
            .ok_or_else(|| io::Error::other("write characteristic not found"))?;
        let notify_char = characteristics
            .iter()
            .find(|ch| ch.uuid == notify_uuid && ch.service_uuid == service_uuid)
            .cloned()
            .ok_or_else(|| io::Error::other("notify characteristic not found"))?;
        let write_type = if write_char.properties.contains(CharPropFlags::WRITE) {
            WriteType::WithResponse
        } else if write_char.properties.contains(CharPropFlags::WRITE_WITHOUT_RESPONSE) {
            WriteType::WithoutResponse
        } else {
            return Err(io::Error::other("write characteristic has no write capability"));
        };

        let mut notifications = peripheral.notifications().await.map_err(io::Error::other)?;
        peripheral.subscribe(&notify_char).await.map_err(io::Error::other)?;

        let mut outbound = Vec::with_capacity(1 + runtime_frame.len());
        outbound.push(BLE_FRAME_NATIVE_WIRE);
        outbound.extend_from_slice(&runtime_frame);
        peripheral
            .write(&write_char, outbound.as_slice(), write_type)
            .await
            .map_err(io::Error::other)?;

        let deadline = Instant::now() + timeout;
        let mut attachment_id: Option<String> = None;
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let notification = match tokio::time::timeout(remaining, notifications.next()).await {
                Ok(Some(notification)) => notification,
                Ok(None) => break,
                Err(_) => break,
            };
            if notification.uuid != notify_uuid || notification.value.is_empty() {
                continue;
            }
            if notification.value[0] != BLE_FRAME_NATIVE_WIRE {
                continue;
            }
            let frame = decode_frame(&notification.value[1..]).map_err(embedded_to_io)?;
            if frame.kind != FRAME_KIND_LXMF_MESSAGE {
                continue;
            }
            let reply = decode_envelope(&frame.payload).map_err(embedded_to_io)?;
            let attachment = upload_attachment_via_rpc(
                rpc.as_str(),
                "ble-native-bridge.txt".to_string(),
                content_type.clone(),
                reply.body.as_slice(),
                4096,
            )?;
            println!(
                "BLE_NATIVE_BRIDGE ok: device_id={} frame_seq={} body={} attachment_id={}",
                peripheral.id(),
                frame.sequence,
                String::from_utf8_lossy(&reply.body),
                attachment
            );
            attachment_id = Some(attachment);
            break;
        }

        let _ = peripheral.unsubscribe(&notify_char).await;
        let _ = peripheral.disconnect().await;
        if attachment_id.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "no LXMF response frame received before timeout",
            ));
        }
        Ok(())
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn run_ble_native_bridge(
    _scan_secs: u64,
    _name_hint: String,
    _peripheral_id: Option<String>,
    _service_uuid: String,
    _write_char_uuid: String,
    _notify_char_uuid: String,
    _rpc: String,
    _runtime_seq: Option<u32>,
    _payload: String,
    _destination_hex: String,
    _source_hex: String,
    _timeout_secs: u64,
    _content_type: String,
) -> io::Result<()> {
    Err(io::Error::other(
        "ble-native-bridge is only supported on linux/macos/windows",
    ))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn run_ble_find_camera(
    _scan_secs: u64,
    _name_hint: String,
    _service_uuid: String,
    _write_char_uuid: String,
    _notify_char_uuid: String,
) -> io::Result<()> {
    Err(io::Error::other(
        "ble-find-camera is only supported on linux/macos/windows",
    ))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn run_ble_scan(
    _timeout_secs: u64,
    _limit: usize,
    _service_uuid: Option<String>,
    _manufacturer_prefix: Option<String>,
) -> io::Result<()> {
    Err(io::Error::other("ble-scan is only supported on linux/macos/windows"))
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn format_manufacturer_data(data: &std::collections::HashMap<u16, Vec<u8>>) -> String {
    let mut entries: Vec<String> = data
        .iter()
        .map(|(company, payload)| {
            format!(
                "{company:#06x}:{}:{}",
                hex_lower(payload),
                ascii_safe(payload)
            )
        })
        .collect();
    entries.sort_unstable();
    format!("[{}]", entries.join(","))
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn ascii_safe(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| {
            if byte.is_ascii_graphic() || *byte == b' ' {
                *byte as char
            } else {
                '.'
            }
        })
        .collect()
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn ascii_lower(bytes: &[u8]) -> String {
    ascii_safe(bytes).to_lowercase()
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
async fn find_peripheral(
    adapter: &btleplug::platform::Adapter,
    peripheral_id: &str,
    service_uuid: Option<Uuid>,
    timeout: Duration,
) -> io::Result<Peripheral> {
    let deadline = Instant::now() + timeout;
    loop {
        let peripherals = adapter.peripherals().await.map_err(io::Error::other)?;
        for peripheral in peripherals {
            if peripheral_matches(&peripheral, peripheral_id, service_uuid).await? {
                return Ok(peripheral);
            }
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "BLE peripheral not found"));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
async fn find_camera_peripheral_by_profile(
    adapter: &btleplug::platform::Adapter,
    peripheral_hint: &str,
    service_uuid: Uuid,
    write_uuid: Uuid,
    notify_uuid: Uuid,
    timeout: Duration,
) -> io::Result<Peripheral> {
    let deadline = Instant::now() + timeout;
    let hint_norm = normalize_identifier(peripheral_hint);
    loop {
        let peripherals = adapter.peripherals().await.map_err(io::Error::other)?;
        let mut candidates: Vec<(u8, Peripheral)> = Vec::with_capacity(peripherals.len());
        for peripheral in peripherals {
            let mut rank = 3_u8;
            if let Ok(Some(props)) = peripheral.properties().await.map_err(io::Error::other) {
                if let Some(name) = props.local_name {
                    let name_norm = normalize_identifier(name.as_str());
                    if !hint_norm.is_empty() && name_norm.contains(&hint_norm) {
                        rank = 0;
                    } else if name_norm.contains("lxmfcamstub") || name_norm.contains("lxmf") {
                        rank = 1;
                    }
                }
            }
            candidates.push((rank, peripheral));
        }
        candidates.sort_by_key(|(rank, _)| *rank);
        for (_, peripheral) in candidates {
            let connected = peripheral.is_connected().await.map_err(io::Error::other)?;
            if !connected {
                if tokio::time::timeout(Duration::from_millis(700), peripheral.connect())
                    .await
                    .is_err()
                {
                    continue;
                }
            }
            if peripheral.discover_services().await.is_err() {
                let _ = peripheral.disconnect().await;
                continue;
            }
            let chars = peripheral.characteristics();
            let has_write = chars
                .iter()
                .any(|ch| ch.uuid == write_uuid && ch.service_uuid == service_uuid);
            let has_notify = chars
                .iter()
                .any(|ch| ch.uuid == notify_uuid && ch.service_uuid == service_uuid);
            if has_write && has_notify {
                return Ok(peripheral);
            }
            let _ = peripheral.disconnect().await;
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "BLE peripheral not found",
            ));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
async fn peripheral_matches(
    peripheral: &Peripheral,
    configured_id: &str,
    service_uuid: Option<Uuid>,
) -> io::Result<bool> {
    if identifiers_match(configured_id, &peripheral.id().to_string()) {
        return Ok(true);
    }
    let properties = peripheral.properties().await.map_err(io::Error::other)?;
    if let Some(properties) = properties {
        if identifiers_match(configured_id, &properties.address.to_string()) {
            return Ok(true);
        }
        if let Some(local_name) = properties.local_name {
            if identifiers_match(configured_id, &local_name) {
                return Ok(true);
            }
        }
        if let Some(expected_service) = service_uuid {
            if properties.services.into_iter().any(|service| service == expected_service) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn identifiers_match(configured: &str, discovered: &str) -> bool {
    let configured = normalize_identifier(configured);
    let discovered = normalize_identifier(discovered);
    configured == discovered
        || discovered.contains(&configured)
        || configured.contains(&discovered)
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn normalize_identifier(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter(|ch| !matches!(ch, ':' | '-'))
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn parse_gatt_uuid(value: &str) -> io::Result<Uuid> {
    let normalized = value.trim();
    if normalized.len() == 4 && normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Uuid::parse_str(&format!("0000{normalized}-0000-1000-8000-00805f9b34fb"))
            .map_err(io::Error::other);
    }
    if normalized.len() == 8 && normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Uuid::parse_str(&format!("{normalized}-0000-1000-8000-00805f9b34fb"))
            .map_err(io::Error::other);
    }
    Uuid::parse_str(normalized).map_err(io::Error::other)
}

fn parse_hex_16(value: &str) -> io::Result<[u8; 16]> {
    let normalized = value.trim();
    if normalized.len() != 32 || !normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected 32 hex characters for a 16-byte address",
        ));
    }
    let bytes = hex::decode(normalized)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
    let mut out = [0_u8; 16];
    out.copy_from_slice(bytes.as_slice());
    Ok(out)
}

fn embedded_to_io(error: rns_embedded_core::EmbeddedError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("{error:?}"))
}

fn resolve_runtime_seq(explicit: Option<u32>) -> u32 {
    explicit.unwrap_or_else(|| {
        let seq = (timestamp_millis() & 0xffff_ffff) as u32;
        seq.max(1)
    })
}

fn run_replay(
    trace: PathBuf,
    capture_out: Option<PathBuf>,
    identity_hash: String,
) -> io::Result<()> {
    let trace_data = load_trace_file(&trace).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("failed to load replay trace '{}': {error}", trace.display()),
        )
    })?;
    let daemon = rns_rpc::RpcDaemon::test_instance_with_identity(identity_hash.as_str());
    let capture = execute_trace(&daemon, &trace_data).map_err(io::Error::other)?;
    if let Some(path) = capture_out {
        save_capture_file(&path, &capture).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("failed to write replay capture '{}': {error}", path.display()),
            )
        })?;
    }
    println!(
        "REPLAY ok: trace='{}' steps={} digest={}",
        capture.trace_name, capture.steps_executed, capture.response_digest_sha256
    );
    Ok(())
}

fn run_e2e(
    a_port: u16,
    b_port: u16,
    timeout_secs: u64,
    keep: bool,
    modes: Vec<DeliveryMode>,
) -> io::Result<()> {
    let timeout = Duration::from_secs(timeout_secs);
    let mut reserved_ports = HashSet::new();
    let a_rpc_listener = reserve_port(a_port, &reserved_ports)?;
    let a_rpc_port = a_rpc_listener.local_addr()?.port();
    reserved_ports.insert(a_rpc_port);
    let b_rpc_listener = reserve_port(b_port, &reserved_ports)?;
    let b_rpc_port = b_rpc_listener.local_addr()?.port();
    reserved_ports.insert(b_rpc_port);

    let a_transport_listener =
        reserve_port(derive_preferred_transport_port(a_rpc_port, 100)?, &reserved_ports)?;
    let a_transport_port = a_transport_listener.local_addr()?.port();
    reserved_ports.insert(a_transport_port);
    let b_transport_listener =
        reserve_port(derive_preferred_transport_port(b_rpc_port, 100)?, &reserved_ports)?;
    let b_transport_port = b_transport_listener.local_addr()?.port();

    let a_rpc = format!("127.0.0.1:{}", a_rpc_port);
    let b_rpc = format!("127.0.0.1:{}", b_rpc_port);
    let a_transport = format!("127.0.0.1:{}", a_transport_port);
    let b_transport = format!("127.0.0.1:{}", b_transport_port);

    let a_dir = tempfile::TempDir::new()?;
    let b_dir = tempfile::TempDir::new()?;
    let a_db = a_dir.path().join("reticulum.db");
    let b_db = b_dir.path().join("reticulum.db");
    let a_config = a_dir.path().join("reticulum.toml");
    let b_config = b_dir.path().join("reticulum.toml");

    fs::write(&a_config, build_tcp_client_config("127.0.0.1", b_transport_port))?;
    fs::write(&b_config, build_tcp_client_config("127.0.0.1", a_transport_port))?;

    drop(a_rpc_listener);
    drop(a_transport_listener);
    let mut a_child = spawn_daemon(&a_rpc, &a_db, &a_transport, &a_config)?;
    let a_destination_hash = wait_for_ready(
        a_child.stdout.take().ok_or_else(|| io::Error::other("missing daemon stdout"))?,
        timeout,
    );
    let a_destination_hash = match a_destination_hash {
        Ok(hash) => hash,
        Err(err) => {
            cleanup_child(&mut a_child, keep);
            return Err(err);
        }
    };

    drop(b_rpc_listener);
    drop(b_transport_listener);
    let mut b_child = spawn_daemon(&b_rpc, &b_db, &b_transport, &b_config)?;
    let b_destination_hash = wait_for_ready(
        b_child.stdout.take().ok_or_else(|| io::Error::other("missing daemon stdout"))?,
        timeout,
    );
    let b_destination_hash = match b_destination_hash {
        Ok(hash) => hash,
        Err(err) => {
            cleanup_child(&mut a_child, keep);
            cleanup_child(&mut b_child, keep);
            return Err(err);
        }
    };

    let mut req_id = 1u64;
    rpc_call(&b_rpc, req_id, "announce_now", None)?;
    req_id = req_id.wrapping_add(1);
    let b_destination_for_a =
        poll_for_any_peer(&a_rpc, timeout, req_id, a_destination_hash.as_deref())?;
    let Some(b_destination_for_a) = b_destination_for_a else {
        cleanup_child(&mut a_child, keep);
        cleanup_child(&mut b_child, keep);
        return Err(io::Error::new(io::ErrorKind::TimedOut, "daemon A did not discover daemon B"));
    };
    req_id = req_id.wrapping_add(1);

    rpc_call(&a_rpc, req_id, "announce_now", None)?;
    req_id = req_id.wrapping_add(1);
    let a_destination_for_b =
        poll_for_any_peer(&b_rpc, timeout, req_id, b_destination_hash.as_deref())?;
    let Some(a_destination_for_b) = a_destination_for_b else {
        cleanup_child(&mut a_child, keep);
        cleanup_child(&mut b_child, keep);
        return Err(io::Error::new(io::ErrorKind::TimedOut, "daemon B did not discover daemon A"));
    };
    req_id = req_id.wrapping_add(1);

    let selected_modes = selected_delivery_modes(&modes);
    for mode in selected_modes {
        match mode {
            DeliveryMode::Direct | DeliveryMode::Opportunistic | DeliveryMode::Propagated => {
                run_delivery_mode(
                    mode,
                    &a_rpc,
                    &b_rpc,
                    &a_destination_for_b,
                    &b_destination_for_a,
                    timeout,
                    &mut req_id,
                )?;
                run_delivery_mode(
                    mode,
                    &b_rpc,
                    &a_rpc,
                    &b_destination_for_a,
                    &a_destination_for_b,
                    timeout,
                    &mut req_id,
                )?;
            }
            DeliveryMode::Paper => {
                run_paper_workflow(
                    &a_rpc,
                    &b_rpc,
                    &a_destination_for_b,
                    &b_destination_for_a,
                    timeout,
                    &mut req_id,
                )?;
            }
        }
    }

    cleanup_child(&mut a_child, keep);
    cleanup_child(&mut b_child, keep);
    println!("E2E ok: peer discovery A<->B succeeded");
    println!("E2E ok: compatibility delivery modes completed");
    Ok(())
}

struct MeshNodeProcess {
    rpc: String,
    destination_hash: String,
    child: Child,
}

fn run_mesh_sim(
    nodes: usize,
    base_rpc_port: u16,
    timeout_secs: u64,
    keep: bool,
    modes: Vec<DeliveryMode>,
) -> io::Result<()> {
    if !(3..=10).contains(&nodes) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "nodes must be in range 3..=10"));
    }

    let timeout = Duration::from_secs(timeout_secs);
    let mut reserved_ports = HashSet::new();
    let mut rpc_listeners = Vec::with_capacity(nodes);
    let mut rpc_ports = Vec::with_capacity(nodes);
    let mut transport_listeners = Vec::with_capacity(nodes);
    let mut transport_ports = Vec::with_capacity(nodes);

    for idx in 0..nodes {
        let offset = u16::try_from(idx).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "nodes index exceeds u16 range")
        })?;
        let preferred_rpc = base_rpc_port
            .checked_add(offset)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "rpc port overflow"))?;
        let rpc_listener = reserve_port(preferred_rpc, &reserved_ports)?;
        let rpc_port = rpc_listener.local_addr()?.port();
        reserved_ports.insert(rpc_port);
        rpc_ports.push(rpc_port);
        rpc_listeners.push(rpc_listener);
    }

    for rpc_port in &rpc_ports {
        let preferred_transport = derive_preferred_transport_port(*rpc_port, 100)?;
        let transport_listener = reserve_port(preferred_transport, &reserved_ports)?;
        let transport_port = transport_listener.local_addr()?.port();
        reserved_ports.insert(transport_port);
        transport_ports.push(transport_port);
        transport_listeners.push(transport_listener);
    }

    let mut temp_dirs = Vec::with_capacity(nodes);
    let mut db_paths = Vec::with_capacity(nodes);
    let mut config_paths = Vec::with_capacity(nodes);
    for idx in 0..nodes {
        let dir = tempfile::TempDir::new()?;
        let db_path = dir.path().join(format!("reticulum-{idx}.db"));
        let config_path = dir.path().join(format!("reticulum-{idx}.toml"));
        fs::write(&config_path, build_mesh_client_config(idx, &transport_ports))?;
        db_paths.push(db_path);
        config_paths.push(config_path);
        temp_dirs.push(dir);
    }

    drop(rpc_listeners);
    drop(transport_listeners);

    let mut node_processes = Vec::with_capacity(nodes);
    for idx in 0..nodes {
        let rpc = format!("127.0.0.1:{}", rpc_ports[idx]);
        let transport = format!("127.0.0.1:{}", transport_ports[idx]);
        let mut child = match spawn_daemon(&rpc, &db_paths[idx], &transport, &config_paths[idx]) {
            Ok(child) => child,
            Err(err) => {
                cleanup_mesh_children(&mut node_processes, keep);
                return Err(err);
            }
        };
        let destination_hash = match wait_for_ready(
            child.stdout.take().ok_or_else(|| io::Error::other("missing daemon stdout"))?,
            timeout,
        ) {
            Ok(Some(hash)) => hash,
            Ok(None) => {
                cleanup_mesh_children(&mut node_processes, keep);
                cleanup_child(&mut child, keep);
                return Err(io::Error::other("daemon did not report destination hash"));
            }
            Err(err) => {
                cleanup_mesh_children(&mut node_processes, keep);
                cleanup_child(&mut child, keep);
                return Err(err);
            }
        };

        node_processes.push(MeshNodeProcess { rpc, destination_hash, child });
    }

    let mut request_id = 10_u64;
    let selected_modes = selected_mesh_delivery_modes(&modes);
    let first = 0_usize;
    let last = nodes - 1;

    let result = (|| -> io::Result<()> {
        for node in &node_processes {
            rpc_call(&node.rpc, request_id, "announce_now", None)?;
            request_id = request_id.wrapping_add(1);
        }

        for node in &node_processes {
            let discovered =
                poll_for_any_peer(&node.rpc, timeout, request_id, Some(&node.destination_hash))?;
            if discovered.is_none() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "mesh propagation failed: a node did not discover any peer",
                ));
            }
            request_id = request_id.wrapping_add(1);
        }

        for mode in selected_modes {
            match mode {
                DeliveryMode::Direct | DeliveryMode::Opportunistic | DeliveryMode::Propagated => {
                    run_delivery_mode(
                        mode,
                        &node_processes[first].rpc,
                        &node_processes[last].rpc,
                        &node_processes[first].destination_hash,
                        &node_processes[last].destination_hash,
                        timeout,
                        &mut request_id,
                    )?;
                    run_delivery_mode(
                        mode,
                        &node_processes[last].rpc,
                        &node_processes[first].rpc,
                        &node_processes[last].destination_hash,
                        &node_processes[first].destination_hash,
                        timeout,
                        &mut request_id,
                    )?;
                }
                DeliveryMode::Paper => {
                    run_paper_workflow(
                        &node_processes[first].rpc,
                        &node_processes[last].rpc,
                        &node_processes[first].destination_hash,
                        &node_processes[last].destination_hash,
                        timeout,
                        &mut request_id,
                    )?;
                }
            }
        }

        println!("MESH ok: nodes={} announce propagation established across mesh", nodes);
        println!("MESH ok: multi-hop delivery workflows completed");
        Ok(())
    })();

    cleanup_mesh_children(&mut node_processes, keep);
    drop(temp_dirs);
    result
}

fn cleanup_mesh_children(node_processes: &mut [MeshNodeProcess], keep: bool) {
    for node in node_processes {
        cleanup_child(&mut node.child, keep);
    }
}

fn build_mesh_client_config(node_index: usize, transport_ports: &[u16]) -> String {
    let node_count = transport_ports.len();
    let next = (node_index + 1) % node_count;
    let previous = (node_index + node_count - 1) % node_count;
    let mut neighbors = vec![next];
    if previous != next {
        neighbors.push(previous);
    }

    let mut config = String::new();
    for neighbor in neighbors {
        config.push_str(&format!(
            "[[interfaces]]\ntype = \"tcp_client\"\nenabled = true\nhost = \"127.0.0.1\"\nport = {}\n\n",
            transport_ports[neighbor]
        ));
    }
    config
}

fn selected_mesh_delivery_modes(modes: &[DeliveryMode]) -> Vec<DeliveryMode> {
    if modes.is_empty() {
        return vec![DeliveryMode::Direct];
    }
    selected_delivery_modes(modes)
}

fn selected_delivery_modes(modes: &[DeliveryMode]) -> Vec<DeliveryMode> {
    if modes.is_empty() {
        return vec![
            DeliveryMode::Direct,
            DeliveryMode::Opportunistic,
            DeliveryMode::Propagated,
            DeliveryMode::Paper,
        ];
    }
    let mut selected = Vec::new();
    let mut seen = HashSet::new();
    for mode in modes {
        if seen.insert(*mode) {
            selected.push(*mode);
        }
    }
    selected
}

fn mode_label(mode: DeliveryMode) -> &'static str {
    match mode {
        DeliveryMode::Direct => "direct",
        DeliveryMode::Opportunistic => "opportunistic",
        DeliveryMode::Propagated => "propagated",
        DeliveryMode::Paper => "paper",
    }
}

fn build_mode_send_params(
    message_id: &str,
    source: &str,
    destination: &str,
    content: &str,
    mode: DeliveryMode,
) -> serde_json::Value {
    let mut params = build_send_params(message_id, source, destination, content);
    if let Some(object) = params.as_object_mut() {
        object.insert("method".to_string(), serde_json::json!(mode_label(mode)));
        if matches!(mode, DeliveryMode::Propagated) {
            object.insert("include_ticket".to_string(), serde_json::json!(true));
            object.insert("try_propagation_on_fail".to_string(), serde_json::json!(true));
            object.insert("stamp_cost".to_string(), serde_json::json!(8));
        }
    }
    params
}

fn run_delivery_mode(
    mode: DeliveryMode,
    sender_rpc: &str,
    receiver_rpc: &str,
    sender_destination: &str,
    receiver_destination: &str,
    timeout: Duration,
    request_id: &mut u64,
) -> io::Result<()> {
    let label = mode_label(mode);
    let message_id = format!("e2e-{}-{}", label, timestamp_millis());
    let content = format!("hello from rnx e2e ({label})");
    let params = build_mode_send_params(
        &message_id,
        sender_destination,
        receiver_destination,
        &content,
        mode,
    );
    let response = rpc_call(sender_rpc, *request_id, "send_message_v2", Some(params))?;
    ensure_rpc_ok(response, format!("send_message_v2 ({label})").as_str())?;
    *request_id = (*request_id).wrapping_add(1);

    let delivered = poll_for_inbound_content(receiver_rpc, &content, timeout, *request_id)?;
    if !delivered {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("delivery mode '{label}' did not deliver message '{message_id}'"),
        ));
    }
    *request_id = (*request_id).wrapping_add(1);

    let trace_contains_status =
        poll_for_delivery_trace_status(sender_rpc, &message_id, label, timeout, *request_id)?;
    if !trace_contains_status {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("delivery trace for '{message_id}' did not contain mode '{label}'"),
        ));
    }
    *request_id = (*request_id).wrapping_add(1);

    println!("E2E ok: mode={} message {} delivered", label, message_id);
    Ok(())
}

fn run_paper_workflow(
    sender_rpc: &str,
    receiver_rpc: &str,
    sender_destination: &str,
    receiver_destination: &str,
    timeout: Duration,
    request_id: &mut u64,
) -> io::Result<()> {
    let message_id = format!("e2e-paper-{}", timestamp_millis());
    let content = "hello from rnx e2e (paper)";
    let send_params = build_mode_send_params(
        &message_id,
        sender_destination,
        receiver_destination,
        content,
        DeliveryMode::Paper,
    );
    let response = rpc_call(sender_rpc, *request_id, "send_message_v2", Some(send_params))?;
    ensure_rpc_ok(response, "send_message_v2 (paper)")?;
    *request_id = (*request_id).wrapping_add(1);

    let delivered = poll_for_inbound_content(receiver_rpc, content, timeout, *request_id)?;
    if !delivered {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "paper workflow did not deliver baseline message",
        ));
    }
    *request_id = (*request_id).wrapping_add(1);

    let paper_encode_response = rpc_call(
        sender_rpc,
        *request_id,
        "sdk_paper_encode_v2",
        Some(serde_json::json!({ "message_id": message_id })),
    )?;
    let paper_encode_result = ensure_rpc_ok(paper_encode_response, "sdk_paper_encode_v2")?
        .ok_or_else(|| io::Error::other("sdk_paper_encode_v2 missing result body"))?;
    let uri = paper_encode_result
        .get("envelope")
        .and_then(|value| value.get("uri"))
        .and_then(|value| value.as_str())
        .ok_or_else(|| io::Error::other("sdk_paper_encode_v2 missing envelope uri"))?
        .to_string();
    *request_id = (*request_id).wrapping_add(1);

    let paper_decode_response = rpc_call(
        receiver_rpc,
        *request_id,
        "sdk_paper_decode_v2",
        Some(serde_json::json!({ "uri": uri })),
    )?;
    let paper_decode_result = ensure_rpc_ok(paper_decode_response, "sdk_paper_decode_v2")?
        .ok_or_else(|| io::Error::other("sdk_paper_decode_v2 missing result body"))?;
    let accepted =
        paper_decode_result.get("accepted").and_then(|value| value.as_bool()).unwrap_or(false);
    if !accepted {
        return Err(io::Error::other("sdk_paper_decode_v2 returned accepted=false"));
    }
    *request_id = (*request_id).wrapping_add(1);

    println!("E2E ok: mode=paper message {} encoded/decoded", message_id);
    Ok(())
}

fn poll_for_delivery_trace_status(
    rpc: &str,
    message_id: &str,
    expected_mode: &str,
    timeout: Duration,
    mut request_id: u64,
) -> io::Result<bool> {
    let deadline = Instant::now() + timeout;
    let expected_status = format!("sent: {expected_mode}");
    loop {
        let response = rpc_call(
            rpc,
            request_id,
            "message_delivery_trace",
            Some(serde_json::json!({ "message_id": message_id })),
        )?;
        request_id = request_id.wrapping_add(1);
        let result = ensure_rpc_ok(response, "message_delivery_trace")?;
        let has_expected_status = result
            .and_then(|value| value.get("transitions").cloned())
            .and_then(|value| value.as_array().cloned())
            .map(|transitions| {
                transitions.iter().any(|transition| {
                    transition
                        .get("status")
                        .and_then(|value| value.as_str())
                        .is_some_and(|status| status.contains(&expected_status))
                })
            })
            .unwrap_or(false);
        if has_expected_status {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn ensure_rpc_ok(
    response: rns_rpc::RpcResponse,
    context: &str,
) -> io::Result<Option<serde_json::Value>> {
    if let Some(error) = response.error {
        return Err(io::Error::other(format!(
            "{} failed: {} ({})",
            context, error.message, error.code
        )));
    }
    Ok(response.result)
}

fn spawn_daemon(rpc: &str, db_path: &Path, transport: &str, config: &Path) -> io::Result<Child> {
    let mut cmd = ProcessCommand::new(reticulumd_path()?);
    cmd.args(build_daemon_args(
        rpc,
        &db_path.to_string_lossy(),
        0,
        Some(transport),
        Some(&config.to_string_lossy()),
    ));
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::null());
    cmd.spawn()
}

fn derive_preferred_transport_port(rpc_port: u16, offset: u16) -> io::Result<u16> {
    rpc_port.checked_add(offset).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "transport port overflow derived from rpc port")
    })
}

fn reserve_port(preferred: u16, reserved: &HashSet<u16>) -> io::Result<TcpListener> {
    if !reserved.contains(&preferred) {
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", preferred)) {
            return Ok(listener);
        }
    }

    for _ in 0..16 {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        if !reserved.contains(&port) {
            return Ok(listener);
        }
    }

    Err(io::Error::new(io::ErrorKind::AddrNotAvailable, "failed to reserve a network port"))
}

fn reticulumd_path() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let dir = exe.parent().ok_or_else(|| io::Error::other("missing exe parent"))?;
    let candidate = dir.join("reticulumd");
    if candidate.exists() {
        Ok(candidate)
    } else {
        Ok(PathBuf::from("reticulumd"))
    }
}

fn wait_for_ready<R: Read + Send + 'static>(
    reader: R,
    timeout: Duration,
) -> io::Result<Option<String>> {
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut lines = BufReader::new(reader).lines();
        while let Some(Ok(line)) = lines.next() {
            let _ = tx.send(line);
        }
    });

    let deadline = Instant::now() + timeout;
    let mut local_destination_hash = None;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "daemon did not become ready"));
        }
        let remaining = deadline.saturating_duration_since(now);
        match rx.recv_timeout(remaining) {
            Ok(line) => {
                if local_destination_hash.is_none() {
                    local_destination_hash = parse_delivery_destination_hash(&line);
                }
                if is_ready_line(&line) {
                    return Ok(local_destination_hash);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "daemon stdout closed"));
            }
        }
    }
}

fn rpc_call(
    rpc: &str,
    id: u64,
    method: &str,
    params: Option<serde_json::Value>,
) -> io::Result<rns_rpc::RpcResponse> {
    let frame = build_rpc_frame(id, method, params)?;
    let request = build_http_post("/rpc", rpc, &frame);
    let mut stream = TcpStream::connect(rpc)?;
    stream.write_all(&request)?;
    stream.shutdown(Shutdown::Write)?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    let body = parse_http_response_body(&response)?;
    parse_rpc_frame(&body)
}

fn poll_for_inbound_content(
    rpc: &str,
    expected_content: &str,
    timeout: Duration,
    mut request_id: u64,
) -> io::Result<bool> {
    let deadline = Instant::now() + timeout;
    loop {
        let response = rpc_call(rpc, request_id, "list_messages", None)?;
        request_id = request_id.wrapping_add(1);
        if inbound_content_present(&response, expected_content) {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn poll_for_any_peer(
    rpc: &str,
    timeout: Duration,
    mut request_id: u64,
    exclude_peer: Option<&str>,
) -> io::Result<Option<String>> {
    let deadline = Instant::now() + timeout;
    loop {
        let response = rpc_call(rpc, request_id, "list_peers", None)?;
        request_id = request_id.wrapping_add(1);
        if let Some(peer) = first_peer(&response, exclude_peer) {
            return Ok(Some(peer));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn first_peer(response: &rns_rpc::RpcResponse, exclude_peer: Option<&str>) -> Option<String> {
    let result = response.result.as_ref()?;
    let peers = result.get("peers")?.as_array()?;
    peers.iter().find_map(|entry| {
        let candidate = entry.get("peer").and_then(|value| value.as_str())?;
        if Some(candidate) == exclude_peer {
            None
        } else {
            Some(candidate.to_owned())
        }
    })
}

fn parse_delivery_destination_hash(line: &str) -> Option<String> {
    let marker = "delivery destination hash=";
    let idx = line.find(marker)?;
    let start = idx + marker.len();
    let value = line[start..].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

fn inbound_content_present(response: &rns_rpc::RpcResponse, expected_content: &str) -> bool {
    let Some(result) = response.result.as_ref() else {
        return false;
    };
    let Some(messages) = result.get("messages").and_then(|value| value.as_array()) else {
        return false;
    };
    messages.iter().any(|message| {
        message.get("direction").and_then(|value| value.as_str()) == Some("in")
            && message.get("content").and_then(|value| value.as_str()) == Some(expected_content)
    })
}
fn cleanup_child(child: &mut Child, keep: bool) {
    if keep {
        return;
    }
    let _ = child.kill();
    let _ = child.wait();
}
