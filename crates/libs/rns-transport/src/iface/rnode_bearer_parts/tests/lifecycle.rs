use super::*;
use crate::iface::lora::{
    CMD_BANDWIDTH, CMD_CR, CMD_DETECT, CMD_FREQUENCY, CMD_FW_VERSION, CMD_MCU, CMD_PLATFORM,
    CMD_RADIO_STATE, CMD_SF, CMD_TXPOWER, DETECT_RESP, PLATFORM_ESP32, RADIO_STATE_ON,
};
use crate::iface::RxMessage;
use std::sync::atomic::AtomicU64;
use tokio_util::sync::CancellationToken;

const BOUND: Duration = Duration::from_secs(2);
const INITIAL_KEY: &str = "bearer-initial-key";
const ROTATED_KEY: &str = "bearer-rotated-key";
const FINAL_KEY: &str = "bearer-restarted-key";

#[derive(Default)]
struct BearerState {
    reads: Mutex<VecDeque<Result<Vec<u8>, String>>>,
    writes: Mutex<Vec<Vec<u8>>>,
    read_actions: AtomicUsize,
    closes: AtomicUsize,
}

impl BearerState {
    fn queue(&self, bytes: Vec<u8>) {
        self.reads.lock().expect("software bearer reads").push_back(Ok(bytes));
    }

    fn wire(&self) -> Vec<u8> {
        self.writes.lock().expect("software bearer writes").iter().flatten().copied().collect()
    }
}

struct SoftwareBearer {
    state: Arc<BearerState>,
    kind: RnodeBearerKind,
    fail_open: bool,
    fail_close: bool,
}

impl RnodeBearerBackend for SoftwareBearer {
    async fn open(&mut self) -> Result<RnodeBearerInfo, String> {
        if self.fail_open {
            return Err("injected open failure".to_string());
        }
        Ok(RnodeBearerInfo {
            kind: self.kind,
            negotiated_mtu: (self.kind == RnodeBearerKind::Ble).then_some(517),
        })
    }

    async fn read(&mut self) -> Result<Option<Vec<u8>>, String> {
        let read = self.state.reads.lock().expect("software bearer reads").pop_front();
        if read.is_some() {
            self.state.read_actions.fetch_add(1, Ordering::SeqCst);
        }
        read.transpose()
    }

    async fn write(&mut self, payload: Vec<u8>) -> Result<(), String> {
        self.state.writes.lock().expect("software bearer writes").push(payload);
        Ok(())
    }

    async fn close(&mut self) -> Result<(), String> {
        self.state.closes.fetch_add(1, Ordering::SeqCst);
        if self.fail_close {
            Err("injected close failure".to_string())
        } else {
            Ok(())
        }
    }
}

struct Attempt {
    address: AddressHash,
    status: RnodeBearerRuntimeStatusHandle,
    state: Arc<BearerState>,
    violations: Arc<AtomicU64>,
    receive: tokio::sync::mpsc::Sender<RxMessage>,
    global: CancellationToken,
    stop: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

fn policy(key: &str) -> InterfaceSharedConfig {
    InterfaceSharedConfig {
        network_name: Some("software-rnode-bearer-lifecycle".to_string()),
        passphrase: Some(key.to_string()),
        ..InterfaceSharedConfig::default()
    }
}

fn packet(marker: u8) -> Packet {
    Packet {
        destination: AddressHash::new_from_slice(&[marker; 16]),
        data: PacketDataBuffer::new_from_slice(&[marker, 0xc0, 0xdb, marker]),
        ..Packet::default()
    }
}

fn startup(lora: LoraConfig) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (command, payload) in [
        (CMD_DETECT, vec![DETECT_RESP]),
        (CMD_FW_VERSION, vec![1, 83]),
        (CMD_PLATFORM, vec![PLATFORM_ESP32]),
        (CMD_MCU, vec![1]),
        (
            CMD_FREQUENCY,
            u32::try_from(lora.frequency_hz).expect("RNode frequency").to_be_bytes().to_vec(),
        ),
        (CMD_BANDWIDTH, lora.bandwidth_hz.to_be_bytes().to_vec()),
        (CMD_TXPOWER, lora.tx_power_dbm.to_be_bytes().to_vec()),
        (CMD_SF, vec![lora.spreading_factor]),
        (CMD_CR, vec![lora.coding_rate]),
        (CMD_RADIO_STATE, vec![RADIO_STATE_ON]),
    ] {
        bytes.extend(encode_command_frame(command, &payload));
    }
    bytes
}

fn attempt(
    manager: &mut InterfaceManager,
    kind: RnodeBearerKind,
    key: &str,
    fail_open: bool,
    fail_close: bool,
) -> Attempt {
    let lora = LoraConfig { max_payload_bytes: 508, ..LoraConfig::us915_default() };
    lora.validate_rnode().expect("software RNode configuration");
    let state = Arc::new(BearerState::default());
    if !fail_open {
        state.queue(startup(lora));
    }
    let config = RnodeBleKissConfig {
        initial_frames: lora.probe_frames(),
        deferred_frames: lora.radio_config_frames(),
        shutdown_frames: lora.shutdown_frames(),
        ..RnodeBleKissConfig::default()
    };
    let interface = RnodeBearerKissInterface::new(
        "software-rnode-lifecycle",
        "software://rnode-lifecycle",
        SoftwareBearer { state: state.clone(), kind, fail_open, fail_close },
        config,
        lora,
    );
    let status = interface.runtime_status_handle();
    let mut context = manager.new_context(interface);
    let address = context.channel.address;
    assert_eq!(context.channel.ifac_default_size_bytes, 8);
    assert_eq!(manager.mtu(&address), Some(508));
    assert!(manager.try_set_shared_config(address, policy(key)).expect("prepared IFAC policy"));
    let global = CancellationToken::new();
    context.cancel = global.clone();
    Attempt {
        address,
        status,
        state,
        violations: context.channel.ifac_violations.clone(),
        receive: context.channel.rx_channel.clone(),
        global,
        stop: context.channel.stop.clone(),
        task: tokio::spawn(RnodeBearerKissInterface::spawn(context)),
    }
}

async fn wait_for(bound: Duration, message: &str, ready: impl Fn() -> bool) {
    tokio::time::timeout(bound, async {
        while !ready() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect(message);
}

impl Attempt {
    async fn ready(&self, kind: RnodeBearerKind) {
        wait_for(Duration::from_secs(7), "unchanged five-second startup validation", || {
            let value = self.status.to_json();
            value["startup_validated"] == true && value["online"] == true
        })
        .await;
        let value = self.status.to_json();
        assert_eq!(value["bearer"], bearer_label(RnodeBearerInfo { kind, negotiated_mtu: None }));
        assert_eq!(
            value["negotiated_mtu"],
            if kind == RnodeBearerKind::Ble {
                serde_json::json!(517)
            } else {
                serde_json::Value::Null
            }
        );
        assert_eq!(value["traffic"]["rx_packets"], 0);
        assert_eq!(value["traffic"]["tx_packets"], 0);
    }

    async fn joined(&mut self, closes: usize) {
        if tokio::time::timeout(BOUND, &mut self.task)
            .await
            .expect("bearer must join within bound")
            .is_err()
        {
            panic!("software bearer task panicked");
        }
        assert_eq!(self.state.closes.load(Ordering::SeqCst), closes);
    }
}

async fn full_receive_queue(kind: RnodeBearerKind) {
    let mut manager = InterfaceManager::new(1);
    let mut run = attempt(&mut manager, kind, INITIAL_KEY, false, false);
    run.ready(kind).await;
    let sentinel =
        RxMessage { address: run.address, packet: packet(0x71), source: IfaceSource::None };
    run.receive.send(sentinel.clone()).await.expect("prefill the actual receive queue");
    run.state.queue(rnode_packet_frame(&rnode_ifac_state(policy(INITIAL_KEY)), &packet(0x72)));
    // On this current-thread executor the ready read runs through decoding to the full send.
    wait_for(BOUND, "authenticated notification reaches full receive queue", || {
        run.state.read_actions.load(Ordering::SeqCst) == 2
    })
    .await;
    assert_eq!(run.receive.capacity(), 0);
    assert_eq!(run.violations.load(Ordering::SeqCst), 0);
    if kind == RnodeBearerKind::Ble {
        run.global.cancel();
    } else {
        run.stop.cancel();
    }
    let joined = tokio::time::timeout(BOUND, &mut run.task).await;
    if joined.is_err() {
        run.task.abort();
        let _ = run.task.await;
        panic!("full receive queue prevents cancelled bearer from joining");
    }
    joined.expect("join was observed").expect("bearer task did not panic");
    assert_eq!(run.state.closes.load(Ordering::SeqCst), 1);
    assert_eq!(run.status.to_json()["online"], false);
    let receiver = manager.receiver();
    let mut receiver = receiver.lock().await;
    assert_eq!(receiver.try_recv().expect("held sentinel"), sentinel);
    assert!(receiver.try_recv().is_err(), "cancelled handoff must not admit its packet");
    drop(receiver);
    manager.cleanup();
    assert!(manager.interface_hashes().is_empty());
}

async fn terminal_offline(kind: RnodeBearerKind) {
    let mut manager = InterfaceManager::new(1);
    let mut run = attempt(&mut manager, kind, INITIAL_KEY, false, false);
    run.ready(kind).await;
    let before = run.status.to_json();
    if kind == RnodeBearerKind::Ble {
        run.stop.cancel();
    } else {
        run.global.cancel();
    }
    run.joined(1).await;
    let after = run.status.to_json();
    assert_eq!(after["online"], false, "a retired healthy attempt is offline");
    for field in ["startup_validated", "bearer", "negotiated_mtu", "traffic", "last_command_error"]
    {
        assert_eq!(after[field], before[field], "terminal publication preserves {field}");
    }
    manager.cleanup();
    assert!(manager.interface_hashes().is_empty());
}

async fn authenticated_roundtrip(
    manager: &mut InterfaceManager,
    run: &Attempt,
    key: &str,
    wrong: &str,
    marker: u8,
) {
    let expected = rnode_ifac_state(policy(key));
    let bad = rnode_ifac_state(policy(wrong));
    let inbound = packet(marker);
    let previous = run.violations.load(Ordering::SeqCst);
    run.state.queue(rnode_packet_frame(&bad, &inbound));
    run.state.queue(encode_data_frame(&inbound.to_bytes().expect("plaintext control packet")));
    wait_for(BOUND, "wrong key and plaintext rejected", || {
        run.violations.load(Ordering::SeqCst) == previous + 2
    })
    .await;
    let receiver = manager.receiver();
    assert!(receiver.lock().await.try_recv().is_err());
    let raw = encode_packet_ifac(&expected, &inbound).expect("authenticated ingress");
    assert_eq!(raw.len(), inbound.to_bytes().expect("plain packet").len() + 8);
    run.state.queue(encode_data_frame(&raw));
    let admitted = tokio::time::timeout(BOUND, receiver.lock().await.recv())
        .await
        .expect("ingress bound")
        .expect("admitted packet");
    assert_eq!(admitted.address, run.address);
    assert_eq!(admitted.packet.destination, inbound.destination);
    assert_eq!(admitted.packet.data, inbound.data);
    assert_eq!(admitted.packet.ifac.map(|value| value.length), Some(0));
    let outbound = packet(marker + 1);
    let expected_wire = rnode_packet_frame(&expected, &outbound);
    let trace = manager
        .send(TxMessage { tx_type: TxMessageType::Direct(run.address), packet: outbound })
        .await;
    assert_eq!(trace.sent_ifaces, 1);
    wait_for(BOUND, "exact authenticated KISS egress at software bearer", || {
        run.state.wire().windows(expected_wire.len()).any(|bytes| bytes == expected_wire)
    })
    .await;
}

async fn caller_owned_retry(kind: RnodeBearerKind) {
    let mut manager = InterfaceManager::new(4);
    let mut failed = attempt(&mut manager, kind, INITIAL_KEY, true, true);
    failed.joined(2).await;
    let error =
        failed.status.to_json()["last_command_error"].as_str().expect("open failure").to_string();
    assert!(error.contains("injected open failure") && error.contains("injected close failure"));
    assert!(failed.state.wire().is_empty());
    assert!(manager.receiver().lock().await.try_recv().is_err());
    manager.cleanup();
    assert!(manager.interface_hashes().is_empty());

    let mut active = attempt(&mut manager, kind, INITIAL_KEY, false, true);
    assert_ne!(active.address, failed.address);
    active.ready(kind).await;
    authenticated_roundtrip(&mut manager, &active, INITIAL_KEY, ROTATED_KEY, 0x81).await;
    assert!(manager
        .try_set_shared_config(active.address, policy(ROTATED_KEY))
        .expect("live key rotation"));
    authenticated_roundtrip(&mut manager, &active, ROTATED_KEY, INITIAL_KEY, 0x83).await;
    wait_for(BOUND, "both output counters published", || {
        active.status.to_json()["traffic"]["tx_packets"] == 2
    })
    .await;
    let traffic = active.status.to_json()["traffic"].clone();
    active
        .state
        .reads
        .lock()
        .expect("software bearer reads")
        .push_back(Err("injected read failure".to_string()));
    active.joined(1).await;
    let terminal = active.status.to_json();
    assert_eq!(terminal["online"], false);
    assert_eq!(terminal["traffic"], traffic);
    let error = terminal["last_command_error"].as_str().expect("read failure");
    assert!(error.contains("injected read failure"));
    assert!(error.contains("injected close failure"), "cleanup error must not disappear");
    manager.cleanup();
    assert!(manager.interface_hashes().is_empty());

    let mut fresh = attempt(&mut manager, kind, FINAL_KEY, false, false);
    assert_ne!(fresh.address, active.address);
    fresh.ready(kind).await;
    authenticated_roundtrip(&mut manager, &fresh, FINAL_KEY, ROTATED_KEY, 0x85).await;
    fresh.global.cancel();
    fresh.joined(1).await;
    assert_eq!(fresh.status.to_json()["online"], false);
    assert_eq!(active.status.to_json(), terminal, "fresh generation cannot rewrite retired status");
    manager.cleanup();
    assert!(manager.interface_hashes().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn ble_full_receive_queue_cancellation_joins() {
    full_receive_queue(RnodeBearerKind::Ble).await;
}

#[tokio::test(flavor = "current_thread")]
async fn classic_full_receive_queue_cancellation_joins() {
    full_receive_queue(RnodeBearerKind::BluetoothClassic).await;
}

#[tokio::test(flavor = "current_thread")]
async fn ble_healthy_cancellation_publishes_offline() {
    terminal_offline(RnodeBearerKind::Ble).await;
}

#[tokio::test(flavor = "current_thread")]
async fn classic_healthy_cancellation_publishes_offline() {
    terminal_offline(RnodeBearerKind::BluetoothClassic).await;
}

#[tokio::test(flavor = "current_thread")]
async fn ble_caller_owns_failure_rotation_and_fresh_retry() {
    caller_owned_retry(RnodeBearerKind::Ble).await;
}

#[tokio::test(flavor = "current_thread")]
async fn classic_caller_owns_failure_rotation_and_fresh_retry() {
    caller_owned_retry(RnodeBearerKind::BluetoothClassic).await;
}
