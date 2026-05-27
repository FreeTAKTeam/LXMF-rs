#![allow(clippy::too_many_arguments)]

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use clap::{Parser, ValueEnum};
use rns_rpc::e2e_harness::timestamp_millis;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self};
use std::path::PathBuf;

#[path = "rnx/ble.rs"]
mod ble;
#[path = "rnx/ble_camera.rs"]
mod ble_camera;
#[path = "rnx/ble_native.rs"]
mod ble_native;
#[path = "rnx/harness.rs"]
mod harness;
#[path = "rnx/ble_helpers.rs"]
mod helpers;
#[path = "rnx/resource_repro.rs"]
mod resource_repro;
#[path = "rnx/scenario.rs"]
mod scenario;
#[path = "rnx/scenario_mesh.rs"]
mod scenario_mesh;
#[path = "rnx/tcp.rs"]
mod tcp;
#[path = "rnx/tcp_session.rs"]
mod tcp_session;

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
    ResourceRepro {
        #[arg(long, default_value_t = 4243)]
        a_port: u16,
        #[arg(long, default_value_t = 4244)]
        b_port: u16,
        #[arg(long, default_value = "134.122.46.48")]
        server_host: String,
        #[arg(long, default_value_t = 37428)]
        server_port: u16,
        #[arg(long, default_value_t = 90)]
        timeout_secs: u64,
        #[arg(long, default_value_t = 4096)]
        large_bytes: usize,
        #[arg(long, default_value_t = false)]
        keep: bool,
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
    TcpNativePeer {
        #[arg(long)]
        addr: String,
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
    TcpNativeListener {
        #[arg(long, default_value = "0.0.0.0:7443")]
        bind: String,
        #[arg(long, default_value_t = false)]
        serve: bool,
        #[arg(long, value_enum, default_value_t = NativeListenerMode::Passive)]
        mode: NativeListenerMode,
        #[arg(long)]
        runtime_seq: Option<u32>,
        #[arg(long, default_value = "ping")]
        payload: String,
        #[arg(long, default_value = "22222222222222222222222222222222")]
        destination_hex: String,
        #[arg(long, default_value = "99999999999999999999999999999999")]
        source_hex: String,
        #[arg(long)]
        capture_out: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = CaptureProfileArg::Default)]
        capture_profile: CaptureProfileArg,
        #[arg(long, default_value_t = 15)]
        timeout_secs: u64,
    },
    TcpNativeBridge {
        #[arg(long, default_value = "0.0.0.0:7443")]
        bind: String,
        #[arg(long, default_value_t = false)]
        serve: bool,
        #[arg(long, value_enum, default_value_t = TcpBridgeMode::Capture)]
        mode: TcpBridgeMode,
        #[arg(long)]
        runtime_seq: Option<u32>,
        #[arg(long, default_value = "bridge-ping")]
        payload: String,
        #[arg(long, default_value = "22222222222222222222222222222222")]
        destination_hex: String,
        #[arg(long, default_value = "99999999999999999999999999999999")]
        source_hex: String,
        #[arg(long, default_value = "127.0.0.1:4243")]
        rpc: String,
        #[arg(long, default_value = "image/jpeg")]
        content_type: String,
        #[arg(long)]
        capture_out: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = CaptureProfileArg::Default)]
        capture_profile: CaptureProfileArg,
        #[arg(long, default_value_t = 8192)]
        chunk_size: usize,
        #[arg(long, default_value_t = 30)]
        timeout_secs: u64,
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

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum CaptureProfileArg {
    Default,
    Thumbnail,
    Balanced,
    High,
    VeryHigh,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum NativeListenerMode {
    Passive,
    RawPing,
    LxmfPing,
    Capture,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum TcpBridgeMode {
    LxmfPing,
    Capture,
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
            scenario::run_e2e(a_port, b_port, timeout_secs, keep, modes)
        }
        Command::ResourceRepro {
            a_port,
            b_port,
            server_host,
            server_port,
            timeout_secs,
            large_bytes,
            keep,
        } => resource_repro::run_resource_repro(
            a_port,
            b_port,
            server_host,
            server_port,
            timeout_secs,
            large_bytes,
            keep,
        ),
        Command::MeshSim { nodes, base_rpc_port, timeout_secs, keep, modes } => {
            scenario_mesh::run_mesh_sim(nodes, base_rpc_port, timeout_secs, keep, modes)
        }
        Command::Replay { trace, capture_out, identity_hash } => {
            scenario::run_replay(trace, capture_out, identity_hash)
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
        } => ble::run_camera_capture_upload(
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
            ble::run_ble_scan(timeout_secs, limit, service_uuid, manufacturer_prefix)
        }
        Command::BleFindCamera {
            scan_secs,
            name_hint,
            service_uuid,
            write_char_uuid,
            notify_char_uuid,
        } => ble::run_ble_find_camera(
            scan_secs,
            name_hint,
            service_uuid,
            write_char_uuid,
            notify_char_uuid,
        ),
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
        } => ble::run_ble_native_peer(
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
        } => ble::run_ble_native_bridge(
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
        Command::TcpNativePeer {
            addr,
            mode,
            runtime_seq,
            payload,
            destination_hex,
            source_hex,
            timeout_secs,
        } => tcp::run_tcp_native_peer(
            addr,
            mode,
            runtime_seq,
            payload,
            destination_hex,
            source_hex,
            timeout_secs,
        ),
        Command::TcpNativeListener {
            bind,
            serve,
            mode,
            runtime_seq,
            payload,
            destination_hex,
            source_hex,
            capture_out,
            capture_profile,
            timeout_secs,
        } => tcp::run_tcp_native_listener(
            bind,
            serve,
            mode,
            runtime_seq,
            payload,
            destination_hex,
            source_hex,
            capture_out,
            capture_profile,
            timeout_secs,
        ),
        Command::TcpNativeBridge {
            bind,
            serve,
            mode,
            runtime_seq,
            payload,
            destination_hex,
            source_hex,
            rpc,
            content_type,
            capture_out,
            capture_profile,
            chunk_size,
            timeout_secs,
        } => tcp::run_tcp_native_bridge(
            bind,
            serve,
            mode,
            runtime_seq,
            payload,
            destination_hex,
            source_hex,
            rpc,
            content_type,
            capture_out,
            capture_profile,
            chunk_size,
            timeout_secs,
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
    let attachment_name =
        name.or_else(|| file.file_name().and_then(|value| value.to_str()).map(ToOwned::to_owned));
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
    let start_response = harness::rpc_call(
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
    let start_result = harness::ensure_rpc_ok(start_response, "sdk_attachment_upload_start_v2")?
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
    let mut next_offset = upload.get("next_offset").and_then(|value| value.as_u64()).unwrap_or(0);
    req_id = req_id.wrapping_add(1);

    while usize::try_from(next_offset).ok().is_some_and(|offset| offset < payload.len()) {
        let start =
            usize::try_from(next_offset).map_err(|_| io::Error::other("upload offset overflow"))?;
        let end = start.saturating_add(effective_chunk_size).min(payload.len());
        let bytes_base64 = BASE64_STANDARD.encode(&payload[start..end]);
        let chunk_response = harness::rpc_call(
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
        let chunk_result =
            harness::ensure_rpc_ok(chunk_response, "sdk_attachment_upload_chunk_v2")?
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

    let commit_response = harness::rpc_call(
        rpc,
        req_id,
        "sdk_attachment_upload_commit_v2",
        Some(serde_json::json!({
            "upload_id": upload_id,
            "extensions": {}
        })),
    )?;
    let commit_result = harness::ensure_rpc_ok(commit_response, "sdk_attachment_upload_commit_v2")?
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
fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
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

fn capture_profile_name_from_wire(raw: u8) -> &'static str {
    match raw {
        0 => "default",
        1 => "thumbnail",
        2 => "balanced",
        3 => "high",
        4 => "very_high",
        _ => "unknown",
    }
}

fn build_capture_command_payload(runtime_seq: u32, profile: CaptureProfileArg) -> Vec<u8> {
    let mut payload = Vec::with_capacity(6);
    payload.push(1);
    payload.extend_from_slice(&runtime_seq.to_le_bytes());
    payload.push(match profile {
        CaptureProfileArg::Default => 0,
        CaptureProfileArg::Thumbnail => 1,
        CaptureProfileArg::Balanced => 2,
        CaptureProfileArg::High => 3,
        CaptureProfileArg::VeryHigh => 4,
    });
    payload
}

fn resolve_runtime_seq(explicit: Option<u32>) -> u32 {
    explicit.unwrap_or_else(|| {
        let seq = (timestamp_millis() & 0xffff_ffff) as u32;
        seq.max(1)
    })
}
