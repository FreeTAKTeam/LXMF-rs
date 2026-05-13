#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use btleplug::api::{Central, CharPropFlags, Manager as _, Peripheral as _, ScanFilter, WriteType};
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use btleplug::platform::Manager;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use futures::StreamExt;

use std::io;
use std::time::{Duration, Instant};

#[cfg(any(target_os = "linux", target_os = "windows"))]
use crate::helpers::find_camera_peripheral_by_profile;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use crate::helpers::{find_peripheral, parse_gatt_uuid};
use crate::upload_attachment_via_rpc;
use rns_rpc::e2e_harness::timestamp_millis;

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
pub(crate) fn run_camera_capture_upload(
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
    println!("CAMERA_CAPTURE_UPLOAD ok: bytes={} attachment_id={}", bytes.len(), attachment_id);
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub(crate) fn run_camera_capture_upload(
    _rpc: String,
    _peripheral_id: String,
    _service_uuid: String,
    _write_char_uuid: String,
    _notify_char_uuid: String,
    _content_type: String,
    _chunk_size: usize,
    _timeout_secs: u64,
) -> io::Result<()> {
    Err(io::Error::other("camera-capture-upload is only supported on linux/macos/windows"))
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
        let peripheral =
            match find_peripheral(&adapter, peripheral_id, Some(service_uuid), timeout).await {
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
                        let _ = &error;
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
                .map_err(|_| {
                    io::Error::new(io::ErrorKind::TimedOut, "capture notification timeout")
                })?
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::UnexpectedEof, "notification stream closed")
                })?;

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
                        peripheral
                            .write(&write_char, &ack, write_type)
                            .await
                            .map_err(io::Error::other)?;
                        expected_seq = expected_seq.saturating_add(1);
                    } else if seq < expected_seq {
                        let mut ack = Vec::with_capacity(7);
                        ack.push(FRAME_CHUNK_ACK);
                        ack.extend_from_slice(&fid.to_le_bytes());
                        ack.extend_from_slice(&seq.to_le_bytes());
                        peripheral
                            .write(&write_char, &ack, write_type)
                            .await
                            .map_err(io::Error::other)?;
                    } else {
                        let mut nack = Vec::with_capacity(7);
                        nack.push(FRAME_NACK);
                        nack.extend_from_slice(&fid.to_le_bytes());
                        nack.extend_from_slice(&expected_seq.to_le_bytes());
                        peripheral
                            .write(&write_char, &nack, write_type)
                            .await
                            .map_err(io::Error::other)?;
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
