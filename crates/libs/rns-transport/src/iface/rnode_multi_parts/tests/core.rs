include!("../../rnode_multi_ifac_tests.rs");

use std::collections::BTreeMap;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use tokio::io::{duplex, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio_util::sync::CancellationToken;

use crate::buffer::OutputBuffer;
use crate::hash::AddressHash;
use crate::iface::lora::{
    BATTERY_STATE_CHARGING, CMD_BANDWIDTH, CMD_BLINK, CMD_CR, CMD_RANDOM, CMD_SF, CMD_STAT_BAT,
    CMD_STAT_CHTM, CMD_STAT_PHYPRM, CMD_STAT_RSSI, CMD_STAT_SNR,
};
use crate::iface::{IfaceRole, InterfaceManager, TxMessage, TxMessageType};
use crate::kiss::decode_frames;
use crate::packet::{Packet, PacketDataBuffer};
use crate::serde::Serialize;

use super::*;

fn packet_payload(packet: &Packet) -> Vec<u8> {
    let mut buffer = vec![0_u8; 512];
    let mut output = OutputBuffer::new(&mut buffer);
    packet.serialize(&mut output).expect("serialize packet");
    output.as_slice().to_vec()
}

fn valid_test_packet() -> Packet {
    Packet { data: PacketDataBuffer::new_from_slice(b"rnode-test"), ..Packet::default() }
}

#[derive(Default)]
struct FailingReadStream {
    writes: Vec<u8>,
}

impl AsyncRead for FailingReadStream {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Ready(Err(io::Error::other("synthetic rnode multi read failure")))
    }
}

impl AsyncWrite for FailingReadStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.writes.extend_from_slice(buf);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

struct FailingWriteStream;

impl AsyncRead for FailingWriteStream {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Pending
    }
}

impl AsyncWrite for FailingWriteStream {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(Err(io::Error::other("synthetic rnode multi data write failure")))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

fn test_options(child: AddressHash, vport: u8) -> RNodeMultiStreamOptions {
    test_options_with_management(child, vport).0
}

fn test_options_with_management(
    child: AddressHash,
    vport: u8,
) -> (RNodeMultiStreamOptions, RNodeMultiManagementHandle) {
    let mut vport_map = BTreeMap::new();
    vport_map.insert(child, vport);
    let (management_frame_tx, management_frame_rx) = rnode_multi_management_channel();
    let handle = RNodeMultiManagementHandle { tx: management_frame_tx };
    (
        RNodeMultiStreamOptions {
            parent_iface: AddressHash::new([0xAA; 16]),
            device: "test".to_string(),
            subinterfaces: vec![RNodeMultiSubInterfaceConfig {
                name: "child".to_string(),
                vport,
                config: LoraConfig::us915_default(),
                outgoing: true,
            }],
            runtime_status: Arc::new(Mutex::new(RNodeMultiRuntimeStatus::from_subinterfaces(&[]))),
            vport_map,
            mtu: DEFAULT_MTU,
            startup_probe: None,
            initial_frames: Vec::new(),
            shutdown_frames: Vec::new(),
            id_beacon: None,
            management_frame_rx,
        },
        handle,
    )
}

#[test]
fn rnode_multi_uses_python_default_mtu_and_explicit_override() {
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let adapter = RNodeMultiInterface::new("/dev/ttyACM0", manager.clone());

    assert_eq!(RNodeMultiInterface::mtu(), 508);
    assert_eq!(adapter.mtu_value(), 508);
    assert_eq!(adapter.configured_mtu(), 508);
    assert_eq!(
        RNodeMultiInterface::new_tcp("192.0.2.10:8001", manager).with_mtu(1024).mtu_value(),
        1024
    );
}

fn accepted_probe_status() -> RNodeMultiProbeStatus {
    let mut status = RNodeMultiProbeStatus::default();
    status.accept_command(CMD_DETECT, &[DETECT_RESP]).expect("detect");
    status.accept_command(CMD_FW_VERSION, &[1, 74]).expect("firmware");
    status.accept_command(CMD_PLATFORM, &[0x80]).expect("platform");
    status.accept_command(CMD_MCU, &[0x01]).expect("mcu");
    status.accept_command(CMD_INTERFACES, &[0, 0x11, 1, 0x21]).expect("interfaces");
    status
}

#[tokio::test]
async fn rnode_multi_parent_shutdown_cleans_registered_virtual_vport_ifaces() {
    let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let parent_iface = {
        let mut manager = iface_manager.lock().await;
        manager.new_channel_with_role(8, IfaceRole::Multicast).address
    };
    let first_child = iface_manager
        .lock()
        .await
        .register_virtual_iface(parent_iface, IfaceRole::VirtualUnicast)
        .expect("first child iface");
    let second_child = iface_manager
        .lock()
        .await
        .register_virtual_iface(parent_iface, IfaceRole::VirtualUnicast)
        .expect("second child iface");
    let vport_map = BTreeMap::from([(first_child, 2), (second_child, 3)]);

    cleanup_rnode_multi_virtual_ifaces(&iface_manager, &vport_map).await;

    let manager = iface_manager.lock().await;
    assert_eq!(manager.role(&first_child), None);
    assert_eq!(manager.role(&second_child), None);
}

#[test]
fn rnode_multi_probe_status_accepts_interfaces_and_validates_firmware_1_74() {
    let status = accepted_probe_status();

    assert_eq!(status.interfaces.get(&0), Some(&RNodeMultiInterfaceType::Sx126x));
    assert_eq!(status.interfaces.get(&1), Some(&RNodeMultiInterfaceType::Sx128x));
    status.validate_startup_probe(&[0, 1]).expect("valid RNodeMulti probe");
}

#[test]
fn rnode_multi_probe_status_uses_reported_vports_for_sparse_interfaces() {
    let mut status = RNodeMultiProbeStatus::default();
    status.accept_command(CMD_DETECT, &[DETECT_RESP]).expect("detect");
    status.accept_command(CMD_FW_VERSION, &[1, 74]).expect("firmware");
    status.accept_command(CMD_PLATFORM, &[0x80]).expect("platform");
    status.accept_command(CMD_MCU, &[0x01]).expect("mcu");
    status.accept_command(CMD_INTERFACES, &[3, 0x21, 1, 0x11]).expect("interfaces");

    assert_eq!(status.interfaces.get(&1), Some(&RNodeMultiInterfaceType::Sx126x));
    assert_eq!(status.interfaces.get(&3), Some(&RNodeMultiInterfaceType::Sx128x));
    status.validate_startup_probe(&[1, 3]).expect("sparse vports are valid");
    let err = status.validate_startup_probe(&[0]).expect_err("unreported vport rejected");
    assert!(err.contains("configured vport 0"));
}

#[test]
fn rnode_multi_probe_status_rejects_duplicate_reported_vports() {
    let mut status = accepted_probe_status();
    let err = status
        .accept_command(CMD_INTERFACES, &[2, 0x11, 2, 0x21])
        .expect_err("duplicate vport rejected");

    assert!(err.contains("repeated vport 2"));
}

#[test]
fn rnode_multi_probe_status_rejects_single_rnode_1_52_firmware() {
    let mut status = accepted_probe_status();
    status.firmware_version = Some((1, 52));

    let err = status.validate_startup_probe(&[0]).expect_err("old firmware rejected");
    assert!(err.contains("below required 1.74"));
}

#[test]
fn rnode_multi_probe_status_rejects_missing_configured_vport() {
    let status = accepted_probe_status();

    let err = status.validate_startup_probe(&[2]).expect_err("missing vport rejected");
    assert!(err.contains("configured vport 2"));
}

#[test]
fn rnode_multi_probe_status_detects_display_capable_platforms() {
    let mut status = accepted_probe_status();

    status.platform = Some(PLATFORM_ESP32);
    assert!(status.has_display());
    status.platform = Some(PLATFORM_NRF52);
    assert!(status.has_display());
    status.platform = Some(crate::iface::lora::PLATFORM_AVR);
    assert!(!status.has_display());
}

#[test]
fn rnode_multi_shutdown_frames_use_python_leave_payload() {
    let frames =
        rnode_multi_shutdown_frames(&test_options(AddressHash::new([0x05; 16]), 4).subinterfaces);
    let decoded = decode_frames(&frames.concat(), 512).expect("decode shutdown frames");

    assert_eq!(
        decoded,
        vec![
            KissFrame::Command(KissCommand::Unknown(CMD_SEL_INT, vec![4])),
            KissFrame::Command(KissCommand::Unknown(CMD_RADIO_STATE, vec![RADIO_STATE_OFF])),
            KissFrame::Command(KissCommand::Unknown(CMD_LEAVE, vec![0xff])),
        ]
    );
}

#[test]
fn rnode_multi_runtime_status_routes_radio_commands_to_selected_vport() {
    let mut status = RNodeMultiRuntimeStatus::from_subinterfaces(
        &test_options(AddressHash::new([0x04; 16]), 2).subinterfaces,
    );

    assert_eq!(status.selected_vport, 2);
    status.accept_command(CMD_SEL_INT, &[2]).expect("select vport 2");
    status
        .accept_command(CMD_BANDWIDTH, &125_000_u32.to_be_bytes())
        .expect("record vport 2 bandwidth");
    status.accept_command(CMD_SF, &[9]).expect("record vport 2 sf");
    status.accept_command(CMD_CR, &[5]).expect("record vport 2 coding rate");
    status.accept_command(CMD_STAT_RSSI, &[200]).expect("record vport 2 rssi");
    status.accept_command(CMD_STAT_SNR, &[12]).expect("record vport 2 snr");
    status
        .accept_command(CMD_STAT_BAT, &[BATTERY_STATE_CHARGING, 87])
        .expect("record vport 2 battery");
    status.accept_command(CMD_RANDOM, &[0x5a]).expect("record vport 2 random byte");
    status
        .accept_command(CMD_STAT_CHTM, &[0, 50, 0, 75, 1, 0, 1, 29])
        .expect("record vport 2 channel telemetry");
    status
        .accept_command(CMD_STAT_PHYPRM, &[0, 250, 0, 10, 0, 12, 1, 244, 0, 5])
        .expect("record vport 2 phy telemetry");
    status.accept_command(CMD_SEL_INT, &[3]).expect("select vport 3");
    status.accept_command(CMD_SF, &[7]).expect("record vport 3 sf");

    let vport2 = status.status_for_vport(2).expect("vport 2 status");
    assert_eq!(vport2.bandwidth_hz, Some(125_000));
    assert_eq!(vport2.spreading_factor, Some(9));
    assert_eq!(vport2.coding_rate, Some(5));
    assert_eq!(vport2.rssi_dbm, Some(43));
    assert_eq!(vport2.snr_db, Some(3.0));
    assert_eq!(vport2.battery_state, Some(BATTERY_STATE_CHARGING));
    assert_eq!(vport2.battery_percent, Some(87));
    assert_eq!(vport2.random_byte, Some(0x5a));
    assert_eq!(vport2.airtime_short_percent, Some(0.5));
    assert_eq!(vport2.airtime_long_percent, Some(0.75));
    assert_eq!(vport2.channel_load_short_percent, Some(2.56));
    assert_eq!(vport2.channel_load_long_percent, Some(2.85));
    assert_eq!(vport2.symbol_time_ms, Some(0.25));
    assert_eq!(vport2.symbol_rate_baud, Some(10));
    assert_eq!(vport2.preamble_symbols, Some(12));
    assert_eq!(vport2.preamble_time_ms, Some(500));
    assert_eq!(vport2.csma_slot_time_ms, Some(5));
    assert_eq!(vport2.csma_difs_ms, None);
    let vport3 = status.status_for_vport(3).expect("vport 3 status");
    assert_eq!(vport3.spreading_factor, Some(7));
    assert_eq!(vport3.rssi_dbm, None);

    let snapshot = status.to_json();
    assert_eq!(snapshot["stream_state"].as_str(), Some("configured"));
    assert!(snapshot["last_error"].is_null());
    assert_eq!(snapshot["selected_vport"].as_u64(), Some(3));
    assert!(snapshot["startup_probe"].is_null());
    assert_eq!(snapshot["subinterfaces"]["2"]["bandwidth_hz"].as_u64(), Some(125_000));
    assert_eq!(snapshot["subinterfaces"]["2"]["spreading_factor"].as_u64(), Some(9));
    assert_eq!(snapshot["subinterfaces"]["2"]["coding_rate"].as_u64(), Some(5));
    assert_eq!(snapshot["subinterfaces"]["2"]["rssi_dbm"].as_i64(), Some(43));
    assert_eq!(snapshot["subinterfaces"]["2"]["airtime_short_percent"].as_f64(), Some(0.5));
    assert_eq!(snapshot["subinterfaces"]["2"]["battery_state_label"].as_str(), Some("charging"));
    assert_eq!(snapshot["subinterfaces"]["2"]["battery_percent"].as_u64(), Some(87));
    assert_eq!(snapshot["subinterfaces"]["2"]["random_byte"].as_u64(), Some(0x5a));
    assert_eq!(snapshot["subinterfaces"]["2"]["reported_bitrate_bps"].as_f64(), Some(1757.8125));
    assert_eq!(snapshot["subinterfaces"]["2"]["framebuffer_bytes"].as_u64(), Some(0));
    assert_eq!(snapshot["subinterfaces"]["2"]["display_bytes"].as_u64(), Some(0));
    assert_eq!(snapshot["subinterfaces"]["3"]["spreading_factor"].as_u64(), Some(7));
    assert!(snapshot["subinterfaces"]["3"]["rssi_dbm"].is_null());
}

#[tokio::test]
async fn rnode_multi_stream_writes_initial_frames_after_successful_probe() {
    let child = AddressHash::new([0x03; 16]);
    let (stream, mut peer) = duplex(4096);
    let (rx_tx, rx_rx) = tokio::sync::mpsc::channel(4);
    drop(rx_rx);
    let (_tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
    let cancel = CancellationToken::new();
    let mut options = test_options(child, 1);
    let runtime_status = Arc::clone(&options.runtime_status);
    options.startup_probe = Some(RNodeMultiStartupProbe {
        frames: rnode_multi_probe_frames(),
        required_vports: vec![1],
        timeout: Duration::from_millis(500),
    });
    options.initial_frames = vec![encode_command_frame(CMD_SEL_INT, &[1])];
    let task = tokio::spawn(run_rnode_multi_stream(
        stream,
        options,
        cancel.clone(),
        CancellationToken::new(),
        rx_tx,
        Arc::new(tokio::sync::Mutex::new(tx_rx)),
    ));

    let mut bytes = vec![0_u8; 256];
    let n = peer.read(&mut bytes).await.expect("read probe frames");
    let frames = decode_frames(&bytes[..n], 512).expect("decode probe frames");
    assert_eq!(
        frames,
        vec![
            KissFrame::Command(KissCommand::Unknown(CMD_DETECT, vec![DETECT_REQ])),
            KissFrame::Command(KissCommand::Unknown(CMD_FW_VERSION, vec![0])),
            KissFrame::Command(KissCommand::Unknown(CMD_PLATFORM, vec![0])),
            KissFrame::Command(KissCommand::Unknown(CMD_MCU, vec![0])),
            KissFrame::Command(KissCommand::Unknown(CMD_INTERFACES, vec![0])),
        ]
    );

    peer.write_all(&encode_command_frame(CMD_DETECT, &[DETECT_RESP])).await.expect("write detect");
    peer.write_all(&encode_command_frame(CMD_FW_VERSION, &[1, 74])).await.expect("write firmware");
    peer.write_all(&encode_command_frame(CMD_PLATFORM, &[0x80])).await.expect("write platform");
    peer.write_all(&encode_command_frame(CMD_MCU, &[0x01])).await.expect("write mcu");
    peer.write_all(&encode_command_frame(CMD_INTERFACES, &[0, 0x11, 1, 0x21]))
        .await
        .expect("write interfaces");

    let n = tokio::time::timeout(Duration::from_secs(1), peer.read(&mut bytes))
        .await
        .expect("initial frame timeout")
        .expect("read initial frame");
    cancel.cancel();
    task.await.expect("stream task");

    let frames = decode_frames(&bytes[..n], 512).expect("decode initial frame");
    assert_eq!(frames, vec![KissFrame::Command(KissCommand::Unknown(CMD_SEL_INT, vec![1]))]);
    let snapshot =
        runtime_status.lock().expect("rnode multi runtime status mutex poisoned").to_json();
    assert_eq!(snapshot["startup_probe"]["detected"].as_bool(), Some(true));
    assert_eq!(snapshot["startup_probe"]["firmware_version"]["label"].as_str(), Some("1.74"));
    assert_eq!(snapshot["startup_probe"]["platform"].as_u64(), Some(0x80));
    assert_eq!(snapshot["startup_probe"]["mcu"].as_u64(), Some(0x01));
    assert_eq!(snapshot["startup_probe"]["interfaces"]["0"].as_str(), Some("SX126X"));
    assert_eq!(snapshot["startup_probe"]["interfaces"]["1"].as_str(), Some("SX128X"));
    assert_eq!(snapshot["startup_probe"]["interface_summary"].as_str(), Some("0:SX126X,1:SX128X"));
}

#[tokio::test]
async fn rnode_multi_stream_disables_external_framebuffer_before_display_shutdown() {
    let child = AddressHash::new([0x06; 16]);
    let (stream, mut peer) = duplex(4096);
    let (rx_tx, rx_rx) = tokio::sync::mpsc::channel(4);
    drop(rx_rx);
    let (_tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
    let cancel = CancellationToken::new();
    let mut options = test_options(child, 1);
    options.startup_probe = Some(RNodeMultiStartupProbe {
        frames: rnode_multi_probe_frames(),
        required_vports: vec![1],
        timeout: Duration::from_millis(500),
    });
    options.shutdown_frames = rnode_multi_shutdown_frames(&options.subinterfaces);
    let task = tokio::spawn(run_rnode_multi_stream(
        stream,
        options,
        cancel.clone(),
        CancellationToken::new(),
        rx_tx,
        Arc::new(tokio::sync::Mutex::new(tx_rx)),
    ));

    let mut bytes = vec![0_u8; 512];
    let _ = peer.read(&mut bytes).await.expect("read probe frames");
    peer.write_all(&encode_command_frame(CMD_DETECT, &[DETECT_RESP])).await.expect("write detect");
    peer.write_all(&encode_command_frame(CMD_FW_VERSION, &[1, 74])).await.expect("write firmware");
    peer.write_all(&encode_command_frame(CMD_PLATFORM, &[PLATFORM_ESP32]))
        .await
        .expect("write platform");
    peer.write_all(&encode_command_frame(CMD_MCU, &[0x01])).await.expect("write mcu");
    peer.write_all(&encode_command_frame(CMD_INTERFACES, &[1, 0x11]))
        .await
        .expect("write interfaces");

    tokio::time::sleep(Duration::from_millis(20)).await;
    cancel.cancel();
    let n = tokio::time::timeout(Duration::from_secs(1), peer.read(&mut bytes))
        .await
        .expect("shutdown frame timeout")
        .expect("read shutdown frames");
    task.await.expect("stream task");

    let decoded = decode_frames(&bytes[..n], 512).expect("decode shutdown frames");
    assert_eq!(
        decoded,
        vec![
            KissFrame::Command(KissCommand::Unknown(CMD_FB_EXT, vec![0])),
            KissFrame::Command(KissCommand::Unknown(CMD_SEL_INT, vec![1])),
            KissFrame::Command(KissCommand::Unknown(CMD_RADIO_STATE, vec![RADIO_STATE_OFF])),
            KissFrame::Command(KissCommand::Unknown(CMD_LEAVE, vec![0xff])),
        ]
    );
}

#[tokio::test]
async fn rnode_multi_stream_probe_failure_preserves_partial_probe_metadata() {
    let child = AddressHash::new([0x07; 16]);
    let (stream, mut peer) = duplex(4096);
    let (rx_tx, rx_rx) = tokio::sync::mpsc::channel(4);
    drop(rx_rx);
    let (_tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
    let cancel = CancellationToken::new();
    let mut options = test_options(child, 1);
    let runtime_status = Arc::clone(&options.runtime_status);
    options.startup_probe = Some(RNodeMultiStartupProbe {
        frames: rnode_multi_probe_frames(),
        required_vports: vec![1],
        timeout: Duration::from_millis(25),
    });
    let task = tokio::spawn(run_rnode_multi_stream(
        stream,
        options,
        cancel,
        CancellationToken::new(),
        rx_tx,
        Arc::new(tokio::sync::Mutex::new(tx_rx)),
    ));

    let mut bytes = vec![0_u8; 256];
    let _ = peer.read(&mut bytes).await.expect("read probe frames");
    peer.write_all(&encode_command_frame(CMD_DETECT, &[DETECT_RESP])).await.expect("write detect");
    peer.write_all(&encode_command_frame(CMD_FW_VERSION, &[1, 52]))
        .await
        .expect("write old firmware");
    peer.write_all(&encode_command_frame(CMD_PLATFORM, &[0x80])).await.expect("write platform");
    peer.write_all(&encode_command_frame(CMD_MCU, &[0x01])).await.expect("write mcu");
    peer.write_all(&encode_command_frame(CMD_INTERFACES, &[1, 0x21]))
        .await
        .expect("write interfaces");

    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("rnode multi probe failure timeout")
        .expect("rnode multi stream task");

    let snapshot =
        runtime_status.lock().expect("rnode multi runtime status mutex poisoned").to_json();
    assert_eq!(snapshot["stream_state"].as_str(), Some("probe_failed"));
    assert_eq!(
        snapshot["last_error"].as_str(),
        Some("rnode multi firmware version 1.52 is below required 1.74")
    );
    assert_eq!(snapshot["startup_probe"]["detected"].as_bool(), Some(true));
    assert_eq!(snapshot["startup_probe"]["firmware_version"]["label"].as_str(), Some("1.52"));
    assert_eq!(snapshot["startup_probe"]["platform"].as_u64(), Some(0x80));
    assert_eq!(snapshot["startup_probe"]["mcu"].as_u64(), Some(0x01));
    assert_eq!(snapshot["startup_probe"]["interfaces"]["1"].as_str(), Some("SX128X"));
    assert_eq!(snapshot["startup_probe"]["interface_summary"].as_str(), Some("1:SX128X"));
}

#[tokio::test]
async fn rnode_multi_stream_cancel_marks_runtime_closed_without_hardware() {
    let child = AddressHash::new([0x08; 16]);
    let (stream, _peer) = duplex(4096);
    let (rx_tx, rx_rx) = tokio::sync::mpsc::channel(4);
    drop(rx_rx);
    let (_tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
    let cancel = CancellationToken::new();
    let options = test_options(child, 1);
    let runtime_status = options.runtime_status.clone();
    let task = tokio::spawn(run_rnode_multi_stream(
        stream,
        options,
        cancel.clone(),
        CancellationToken::new(),
        rx_tx,
        Arc::new(tokio::sync::Mutex::new(tx_rx)),
    ));

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let running = {
                let status =
                    runtime_status.lock().expect("rnode multi runtime status mutex poisoned");
                status.stream_state == "running"
            };
            if running {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("runtime reached running");
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("rnode multi stream shutdown timeout")
        .expect("rnode multi stream task");

    let status = runtime_status.lock().expect("rnode multi runtime status mutex poisoned");
    assert_eq!(status.stream_state, "closed");
    assert!(status.last_error.is_none());
}

#[tokio::test]
async fn rnode_multi_stream_read_failure_preserves_failed_runtime_state() {
    let child = AddressHash::new([0x09; 16]);
    let stream = FailingReadStream::default();
    let (rx_tx, rx_rx) = tokio::sync::mpsc::channel(4);
    drop(rx_rx);
    let (_tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
    let cancel = CancellationToken::new();
    let options = test_options(child, 1);
    let runtime_status = options.runtime_status.clone();

    tokio::time::timeout(
        Duration::from_secs(1),
        run_rnode_multi_stream(
            stream,
            options,
            cancel,
            CancellationToken::new(),
            rx_tx,
            Arc::new(tokio::sync::Mutex::new(tx_rx)),
        ),
    )
    .await
    .expect("rnode multi stream exits after read failure");

    let status = runtime_status.lock().expect("rnode multi runtime status mutex poisoned");
    assert_eq!(status.stream_state, "read_failed");
    assert_eq!(status.last_error.as_deref(), Some("synthetic rnode multi read failure"));
}

#[tokio::test]
async fn rnode_multi_stream_data_write_failure_preserves_failed_runtime_state() {
    let child = AddressHash::new([0x0A; 16]);
    let (rx_tx, rx_rx) = tokio::sync::mpsc::channel(4);
    drop(rx_rx);
    let (tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
    let cancel = CancellationToken::new();
    let options = test_options(child, 1);
    let runtime_status = options.runtime_status.clone();

    let task = tokio::spawn(run_rnode_multi_stream(
        FailingWriteStream,
        options,
        cancel,
        CancellationToken::new(),
        rx_tx,
        Arc::new(tokio::sync::Mutex::new(tx_rx)),
    ));

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let running = {
                let status =
                    runtime_status.lock().expect("rnode multi runtime status mutex poisoned");
                status.stream_state == "running"
            };
            if running {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("runtime reached running");

    tx_tx
        .send(TxMessage { tx_type: TxMessageType::Direct(child), packet: Packet::default() })
        .await
        .expect("queue tx");

    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("rnode multi stream write failure timeout")
        .expect("rnode multi stream task");

    let status = runtime_status.lock().expect("rnode multi runtime status mutex poisoned");
    assert_eq!(status.stream_state, "write_failed");
    assert_eq!(status.last_error.as_deref(), Some("data frame write failed"));
}

#[tokio::test]
async fn rnode_multi_stream_routes_direct_tx_to_selected_vport() {
    let child = AddressHash::new([0x01; 16]);
    let (stream, mut peer) = duplex(4096);
    let (_rx_tx, rx_rx) = tokio::sync::mpsc::channel(4);
    drop(rx_rx);
    let (tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(run_rnode_multi_stream(
        stream,
        test_options(child, 2),
        cancel.clone(),
        CancellationToken::new(),
        _rx_tx,
        Arc::new(tokio::sync::Mutex::new(tx_rx)),
    ));

    let packet = Packet::default();
    tx_tx
        .send(TxMessage { tx_type: TxMessageType::Direct(child), packet })
        .await
        .expect("queue tx");
    let mut bytes = vec![0_u8; 256];
    let n = peer.read(&mut bytes).await.expect("read mux frames");
    cancel.cancel();
    task.await.expect("stream task");

    let frames = decode_frames(&bytes[..n], 512).expect("decode frames");
    assert_eq!(frames[0], KissFrame::Command(KissCommand::Unknown(CMD_SEL_INT, vec![2])));
    assert!(matches!(frames[1], KissFrame::Data(_)));
}

#[tokio::test]
async fn rnode_multi_management_handle_writes_selected_vport_command_frame() {
    let child = AddressHash::new([0x12; 16]);
    let (stream, mut peer) = duplex(4096);
    let (_rx_tx, rx_rx) = tokio::sync::mpsc::channel(4);
    drop(rx_rx);
    let (_tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
    let cancel = CancellationToken::new();
    let (options, handle) = test_options_with_management(child, 2);
    let task = tokio::spawn(run_rnode_multi_stream(
        stream,
        options,
        cancel.clone(),
        CancellationToken::new(),
        _rx_tx,
        Arc::new(tokio::sync::Mutex::new(tx_rx)),
    ));

    handle
        .dispatch_frame(2, LoraConfig::blink_frame(0x03))
        .await
        .expect("queue vport management frame");
    let mut bytes = vec![0_u8; 256];
    let n = tokio::time::timeout(Duration::from_secs(1), peer.read(&mut bytes))
        .await
        .expect("management frame timeout")
        .expect("read management frames");
    cancel.cancel();
    task.await.expect("stream task");

    let frames = decode_frames(&bytes[..n], 512).expect("decode management frames");
    assert_eq!(
        frames,
        vec![
            KissFrame::Command(KissCommand::Unknown(CMD_SEL_INT, vec![2])),
            KissFrame::Command(KissCommand::Unknown(CMD_BLINK, vec![0x03])),
        ]
    );
}

#[tokio::test]
async fn rnode_multi_stream_transmits_id_beacon_on_outgoing_subinterfaces_after_first_tx() {
    let child = AddressHash::new([0x11; 16]);
    let mut options = test_options(child, 2);
    options.subinterfaces.push(RNodeMultiSubInterfaceConfig {
        name: "child-two".to_string(),
        vport: 3,
        config: LoraConfig::us915_default(),
        outgoing: true,
    });
    options.subinterfaces.push(RNodeMultiSubInterfaceConfig {
        name: "receive-only".to_string(),
        vport: 4,
        config: LoraConfig::us915_default(),
        outgoing: false,
    });
    options.id_beacon = Some(KissIdBeaconConfig {
        callsign: b"MYCALL-0".to_vec(),
        interval: Duration::from_millis(20),
        min_payload_len: 0,
    });
    let (stream, mut peer) = duplex(4096);
    let (_rx_tx, rx_rx) = tokio::sync::mpsc::channel(4);
    drop(rx_rx);
    let (tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(run_rnode_multi_stream(
        stream,
        options,
        cancel.clone(),
        CancellationToken::new(),
        _rx_tx,
        Arc::new(tokio::sync::Mutex::new(tx_rx)),
    ));

    tx_tx
        .send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet: Packet::default() })
        .await
        .expect("queue broadcast tx");
    let mut bytes = vec![0_u8; 512];
    let first_read = tokio::time::timeout(Duration::from_secs(1), peer.read(&mut bytes))
        .await
        .expect("first packet timeout")
        .expect("read first packet frames");
    assert!(
        decode_frames(&bytes[..first_read], 512)
            .expect("decode first packet")
            .iter()
            .any(|frame| matches!(frame, KissFrame::Data(payload) if payload != b"MYCALL-0")),
        "first RNodeMulti tx must carry the packet before any ID beacon"
    );

    let mut beacon_frames = Vec::new();
    for _ in 0..8 {
        let n = tokio::time::timeout(Duration::from_secs(1), peer.read(&mut bytes))
            .await
            .expect("beacon timeout")
            .expect("read beacon frames");
        beacon_frames.extend(decode_frames(&bytes[..n], 512).expect("decode beacon frames"));
        let beacon_count = beacon_frames
            .iter()
            .filter(|frame| matches!(frame, KissFrame::Data(payload) if payload == b"MYCALL-0"))
            .count();
        if beacon_count >= 2 {
            break;
        }
    }
    cancel.cancel();
    task.await.expect("stream task");

    assert!(beacon_frames.contains(&KissFrame::Command(KissCommand::Unknown(CMD_SEL_INT, vec![2]))));
    assert!(beacon_frames.contains(&KissFrame::Command(KissCommand::Unknown(CMD_SEL_INT, vec![3]))));
    assert!(
        !beacon_frames.contains(&KissFrame::Command(KissCommand::Unknown(CMD_SEL_INT, vec![4])))
    );
    assert_eq!(
        beacon_frames
            .iter()
            .filter(|frame| matches!(frame, KissFrame::Data(payload) if payload == b"MYCALL-0"))
            .count(),
        2
    );
}

#[tokio::test]
async fn rnode_multi_stream_routes_inbound_vport_data_to_child_iface() {
    let child = AddressHash::new([0x02; 16]);
    let (stream, mut peer) = duplex(4096);
    let (rx_tx, mut rx_rx) = tokio::sync::mpsc::channel(4);
    let (_tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(run_rnode_multi_stream(
        stream,
        test_options(child, 3),
        cancel.clone(),
        CancellationToken::new(),
        rx_tx,
        Arc::new(tokio::sync::Mutex::new(tx_rx)),
    ));

    let payload = packet_payload(&valid_test_packet());
    peer.write_all(&encode_command_frame(0x70, &payload)).await.expect("write inbound frame");
    let message = rx_rx.recv().await.expect("rx message");
    cancel.cancel();
    task.await.expect("stream task");

    assert_eq!(message.address, child);
}
