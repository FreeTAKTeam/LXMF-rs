use std::collections::VecDeque;
use std::future::pending;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::iface::InterfaceManager;

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
