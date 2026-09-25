use std::collections::VecDeque;
use std::future::pending;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::hash::AddressHash;
use crate::iface::{InterfaceManager, InterfaceSharedConfig, TxMessage, TxMessageType};
use crate::kiss::{decode_frames, encode_command_frame, encode_data_frame, KissFrame};
use crate::packet::{Packet, PacketDataBuffer};

use super::*;

enum OpenBehavior {
    Fail,
    Block,
}

struct FailingCloseBackend {
    open_behavior: OpenBehavior,
}

impl RnodeBearerBackend for FailingCloseBackend {
    async fn open(&mut self) -> Result<RnodeBearerInfo, String> {
        match self.open_behavior {
            OpenBehavior::Fail => Err("open failed".to_string()),
            OpenBehavior::Block => pending().await,
        }
    }

    async fn read(&mut self) -> Result<Option<Vec<u8>>, String> {
        Ok(None)
    }

    async fn write(&mut self, _payload: Vec<u8>) -> Result<(), String> {
        Ok(())
    }

    async fn close(&mut self) -> Result<(), String> {
        Err("close failed".to_string())
    }
}

fn test_interface(
    open_behavior: OpenBehavior,
) -> (InterfaceContext<RnodeBearerKissInterface<FailingCloseBackend>>, RnodeBearerRuntimeStatusHandle)
{
    let interface = RnodeBearerKissInterface::new(
        "test-rnode",
        "test-endpoint",
        FailingCloseBackend { open_behavior },
        RnodeBleKissConfig::default(),
        LoraConfig::us915_default(),
    );
    let status = interface.runtime_status_handle();
    let mut manager = InterfaceManager::new(4);
    (manager.new_context(interface), status)
}

#[test]
fn runtime_status_exposes_packet_and_kiss_boundary_counters() {
    let status = Arc::new(Mutex::new(serde_json::Value::Null));
    let monitor = RnodeBleCommandMonitor::new(LoraConfig::us915_default(), Duration::from_secs(1));
    let traffic = RnodeBearerTraffic { tx_packets: 2, tx_bytes: 270, rx_packets: 1, rx_bytes: 48 };
    let io = super::super::rnode_ble::RnodeBleKissIoStats {
        read_chunks: 3,
        read_bytes: 60,
        write_chunks: 15,
        write_bytes: 300,
    };

    publish_monitor_status(&status, &monitor, "ble://test", "ble", Some(517), traffic, io);

    let snapshot = status.lock().expect("status mutex").clone();
    assert_eq!(snapshot["negotiated_mtu"], 517);
    assert_eq!(snapshot["traffic"]["tx_packets"], 2);
    assert_eq!(snapshot["traffic"]["kiss_write_chunks"], 15);
    assert_eq!(snapshot["traffic"]["rx_packets"], 1);
    assert_eq!(snapshot["traffic"]["kiss_read_bytes"], 60);
}

#[tokio::test]
async fn failed_startup_records_close_failure_without_losing_open_error() {
    let (context, status) = test_interface(OpenBehavior::Fail);

    RnodeBearerKissInterface::spawn(context).await;

    let snapshot = status.to_json();
    let error = snapshot["last_command_error"].as_str().expect("startup failure status");
    assert!(error.contains("open failed"));
    assert!(error.contains("iface=test-rnode"));
    assert!(error.contains("phase=startup_failure"));
    assert!(error.contains("close failed"));
}

#[tokio::test]
async fn cancelled_startup_records_close_failure() {
    let (context, status) = test_interface(OpenBehavior::Block);
    let cancel = context.cancel.clone();
    let task = tokio::spawn(RnodeBearerKissInterface::spawn(context));
    tokio::task::yield_now().await;

    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("cancelled startup timeout")
        .expect("cancelled startup task");

    let snapshot = status.to_json();
    let error = snapshot["last_command_error"].as_str().expect("cancelled startup failure status");
    assert!(error.contains("iface=test-rnode"));
    assert!(error.contains("phase=startup_cancellation"));
    assert!(error.contains("close failed"));
}

#[derive(Default)]
struct NativeReadState {
    active_reads: AtomicUsize,
    max_active_reads: AtomicUsize,
    started_reads: AtomicUsize,
    consumed_notifications: AtomicUsize,
    notifications: Mutex<VecDeque<Vec<u8>>>,
}

struct DelayedNativeReadBackend {
    state: Arc<NativeReadState>,
    read_started: tokio::sync::mpsc::UnboundedSender<usize>,
}

impl RnodeBearerBackend for DelayedNativeReadBackend {
    async fn open(&mut self) -> Result<RnodeBearerInfo, String> {
        Ok(RnodeBearerInfo { kind: RnodeBearerKind::Ble, negotiated_mtu: Some(517) })
    }

    async fn read(&mut self) -> Result<Option<Vec<u8>>, String> {
        let state = Arc::clone(&self.state);
        let read_started = self.read_started.clone();
        let (sender, receiver) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let active = state.active_reads.fetch_add(1, Ordering::SeqCst) + 1;
            state.max_active_reads.fetch_max(active, Ordering::SeqCst);
            let read_number = state.started_reads.fetch_add(1, Ordering::SeqCst) + 1;
            read_started.send(read_number).expect("read-start observer should remain available");
            tokio::time::sleep(Duration::from_millis(150)).await;
            let notification = state
                .notifications
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .pop_front();
            if notification.is_some() {
                state.consumed_notifications.fetch_add(1, Ordering::SeqCst);
            }
            state.active_reads.fetch_sub(1, Ordering::SeqCst);
            sender
                .send(Ok(notification))
                .expect("bearer read future must remain owned until the native worker completes");
        });
        receiver.await.map_err(|_| "synthetic read worker stopped".to_string())?
    }

    async fn write(&mut self, _payload: Vec<u8>) -> Result<(), String> {
        Ok(())
    }

    async fn close(&mut self) -> Result<(), String> {
        Ok(())
    }
}

#[tokio::test]
async fn bearer_keeps_delayed_native_reads_single_owner_and_delivers_once() {
    let state = Arc::new(NativeReadState::default());
    let (read_started_tx, mut read_started_rx) = tokio::sync::mpsc::unbounded_channel();
    state
        .notifications
        .lock()
        .expect("notification queue")
        .push_back(crate::kiss::encode_data_frame(&[0x42]));
    let interface = RnodeBearerKissInterface::new(
        "test-rnode",
        "ble://test-rnode",
        DelayedNativeReadBackend { state: Arc::clone(&state), read_started: read_started_tx },
        RnodeBleKissConfig::default(),
        LoraConfig::us915_default(),
    );
    let status = interface.runtime_status_handle();
    let mut manager = InterfaceManager::new(4);
    let context = manager.new_context(interface);
    let cancel = context.cancel.clone();
    let task = tokio::spawn(RnodeBearerKissInterface::spawn(context));

    assert_eq!(read_started_rx.recv().await, Some(1));
    // The first notification arrives after the removed 100 ms outer timeout.
    // Wait for it to complete and for the next single-owner read to start,
    // then cancel while that bounded worker is active.
    assert_eq!(
        tokio::time::timeout(Duration::from_millis(400), read_started_rx.recv())
            .await
            .expect("second read should start after the delayed notification"),
        Some(2)
    );
    cancel.cancel();
    tokio::time::timeout(Duration::from_millis(400), task)
        .await
        .expect("cancellation must finish within the backend read bound")
        .expect("bearer task should not panic");

    let snapshot = status.to_json();
    assert_eq!(snapshot["traffic"]["kiss_read_chunks"], 1);
    assert_eq!(snapshot["traffic"]["rx_packets"], 1);
    assert_eq!(state.consumed_notifications.load(Ordering::SeqCst), 1);
    assert_eq!(state.active_reads.load(Ordering::SeqCst), 0);
    assert_eq!(
        state.max_active_reads.load(Ordering::SeqCst),
        1,
        "a native read worker must finish before the next poll starts"
    );

    // Once cancellation closes this generation, there must be no abandoned
    // worker left behind to consume a notification meant for a later one.
    state
        .notifications
        .lock()
        .expect("notification queue")
        .push_back(crate::kiss::encode_data_frame(&[0x99]));
    assert_eq!(state.consumed_notifications.load(Ordering::SeqCst), 1);
    assert_eq!(state.active_reads.load(Ordering::SeqCst), 0);
    assert_eq!(state.started_reads.load(Ordering::SeqCst), 2);
}

#[derive(Default)]
struct IfacBearerState {
    reads: Mutex<VecDeque<Vec<u8>>>,
    writes: Mutex<Vec<Vec<u8>>>,
}

struct IfacTestBearer {
    state: Arc<IfacBearerState>,
    authenticated_read: Option<tokio::sync::oneshot::Receiver<Vec<u8>>>,
}

impl RnodeBearerBackend for IfacTestBearer {
    async fn open(&mut self) -> Result<RnodeBearerInfo, String> {
        Ok(RnodeBearerInfo { kind: RnodeBearerKind::Ble, negotiated_mtu: Some(517) })
    }

    async fn read(&mut self) -> Result<Option<Vec<u8>>, String> {
        if let Some(payload) = self.state.reads.lock().expect("fake bearer reads").pop_front() {
            return Ok(Some(payload));
        }
        match self.authenticated_read.take() {
            Some(receiver) => receiver
                .await
                .map(Some)
                .map_err(|_| "test released authenticated input without a payload".to_string()),
            None => Ok(None),
        }
    }

    async fn write(&mut self, payload: Vec<u8>) -> Result<(), String> {
        self.state.writes.lock().expect("fake bearer writes").push(payload);
        Ok(())
    }

    async fn close(&mut self) -> Result<(), String> {
        Ok(())
    }
}

fn rnode_ifac_state(config: InterfaceSharedConfig) -> crate::iface::IfacState {
    Arc::new(std::sync::RwLock::new(
        config.ifac_context_with_default_size(8).expect("derive RNode IFAC state"),
    ))
}

fn rnode_packet_frame(state: &crate::iface::IfacState, packet: &Packet) -> Vec<u8> {
    let wire = encode_packet_ifac(state, packet).expect("encode IFAC packet");
    encode_data_frame(&wire)
}

#[tokio::test]
async fn rnode_bearer_rejects_wrong_ifac_and_authenticates_ingress_egress() {
    use crate::iface::lora::{
        CMD_BANDWIDTH, CMD_CR, CMD_DETECT, CMD_FREQUENCY, CMD_FW_VERSION, CMD_MCU, CMD_PLATFORM,
        CMD_SF, CMD_TXPOWER, DETECT_RESP, PLATFORM_ESP32,
    };

    let shared = InterfaceSharedConfig {
        network_name: Some("rnode-bearer-ifac-test".to_string()),
        passphrase: Some("rnode-bearer-ifac-secret".to_string()),
        ..InterfaceSharedConfig::default()
    };
    let expected_state = rnode_ifac_state(shared.clone());
    let wrong_state = rnode_ifac_state(InterfaceSharedConfig {
        network_name: shared.network_name.clone(),
        passphrase: Some("incorrect-rnode-bearer-secret".to_string()),
        ..InterfaceSharedConfig::default()
    });
    let rejected_packet = Packet {
        destination: AddressHash::new_from_slice(&[0xa1; 16]),
        data: PacketDataBuffer::new_from_slice(b"wrong RNode bearer IFAC key"),
        ..Packet::default()
    };
    let inbound_packet = Packet {
        destination: AddressHash::new_from_slice(&[0xa2; 16]),
        data: PacketDataBuffer::new_from_slice(b"authenticated RNode bearer ingress"),
        ..Packet::default()
    };
    let outbound_packet = Packet {
        destination: AddressHash::new_from_slice(&[0xa3; 16]),
        data: PacketDataBuffer::new_from_slice(b"authenticated RNode bearer egress"),
        ..Packet::default()
    };

    let lora = LoraConfig::us915_default();
    let mut startup = Vec::new();
    for (command, payload) in [
        (CMD_DETECT, vec![DETECT_RESP]),
        (CMD_FW_VERSION, vec![1, 83]),
        (CMD_PLATFORM, vec![PLATFORM_ESP32]),
        (CMD_MCU, vec![1]),
        (
            CMD_FREQUENCY,
            u32::try_from(lora.frequency_hz)
                .expect("LoRa frequency fits RNode response")
                .to_be_bytes()
                .to_vec(),
        ),
        (CMD_BANDWIDTH, lora.bandwidth_hz.to_be_bytes().to_vec()),
        (CMD_TXPOWER, lora.tx_power_dbm.to_be_bytes().to_vec()),
        (CMD_SF, vec![lora.spreading_factor]),
        (CMD_CR, vec![lora.coding_rate]),
    ] {
        startup.extend(encode_command_frame(command, &payload));
    }

    let bearer_state = Arc::new(IfacBearerState::default());
    let (authenticated_read_tx, authenticated_read_rx) = tokio::sync::oneshot::channel();
    {
        let mut reads = bearer_state.reads.lock().expect("fake bearer reads");
        reads.push_back(startup.clone());
        reads.push_back(rnode_packet_frame(&wrong_state, &rejected_packet));
    }

    let mut manager = InterfaceManager::new(8);
    let interface = RnodeBearerKissInterface::new(
        "test-rnode-ifac",
        "fake://rnode-bearer",
        IfacTestBearer {
            state: Arc::clone(&bearer_state),
            authenticated_read: Some(authenticated_read_rx),
        },
        RnodeBleKissConfig::default(),
        lora,
    );
    let status = interface.runtime_status_handle();
    let context = manager.new_context(interface);
    let address = *context.channel.address();
    let receiver = manager.receiver();
    let violations = Arc::clone(&context.channel.ifac_violations);
    assert!(manager.set_shared_config(address, shared));
    let outbound_trace = manager
        .send(TxMessage {
            tx_type: TxMessageType::Direct(address),
            packet: outbound_packet.clone(),
        })
        .await;
    assert_eq!(outbound_trace.sent_ifaces, 1);

    let cancel = context.cancel.clone();
    let worker = tokio::spawn(RnodeBearerKissInterface::spawn(context));

    tokio::time::timeout(Duration::from_secs(1), async {
        while violations.load(Ordering::Relaxed) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("wrong-key IFAC violation is recorded");
    assert!(
        receiver.lock().await.try_recv().is_err(),
        "wrong-key packet must be rejected before interface admission"
    );

    let mut authenticated_notification = startup;
    authenticated_notification.extend(rnode_packet_frame(&expected_state, &inbound_packet));
    authenticated_read_tx
        .send(authenticated_notification)
        .expect("interface is waiting for authenticated fake-bearer input");

    let admitted = tokio::time::timeout(Duration::from_secs(1), receiver.lock().await.recv())
        .await
        .expect("authenticated ingress admission timeout")
        .expect("interface receive queue remains connected");
    assert_eq!(admitted.address, address);
    assert_eq!(admitted.packet.destination, inbound_packet.destination);
    assert_eq!(admitted.packet.data.as_slice(), b"authenticated RNode bearer ingress");
    assert_eq!(admitted.packet.ifac.map(|ifac| ifac.length), Some(0));

    tokio::time::timeout(Duration::from_secs(7), async {
        while status.to_json()["startup_validated"].as_bool() != Some(true) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("startup radio configuration must validate before payload egress");

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let writes = bearer_state.writes.lock().expect("fake bearer writes").clone();
            let frames = decode_frames(&writes.into_iter().flatten().collect::<Vec<_>>(), 508)
                .expect("decode fake bearer writes");
            if frames.iter().any(|frame| matches!(frame, KissFrame::Data(_))) {
                break frames;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("RNode bearer egress timeout");
    let writes = bearer_state.writes.lock().expect("fake bearer writes").clone();
    let frames = decode_frames(&writes.into_iter().flatten().collect::<Vec<_>>(), 508)
        .expect("decode captured fake bearer writes");
    let encoded_outbound = frames
        .into_iter()
        .find_map(|frame| match frame {
            KissFrame::Data(payload) => Some(payload),
            KissFrame::Command(_) => None,
        })
        .expect("outbound packet is written as a KISS data frame");
    let decoded_outbound = decode_packet_ifac(&expected_state, &encoded_outbound)
        .expect("outbound bearer payload authenticates under configured IFAC");
    assert_eq!(decoded_outbound.destination, outbound_packet.destination);
    assert_eq!(decoded_outbound.data.as_slice(), b"authenticated RNode bearer egress");

    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(1), worker)
        .await
        .expect("cancelled RNode interface stops promptly")
        .expect("RNode interface task does not panic");
}
