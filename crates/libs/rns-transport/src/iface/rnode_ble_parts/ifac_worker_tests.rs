use crate::hash::AddressHash;
use crate::iface::{IfacState, InterfaceManager, InterfaceSharedConfig, TxMessage, TxMessageType};
use crate::packet::{Packet, PacketDataBuffer};
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use tokio::sync::Mutex as AsyncMutex;

#[derive(Clone, Default)]
struct IfacWorkerBackendState {
    incoming: Arc<AsyncMutex<VecDeque<Vec<u8>>>>,
    writes: Arc<AsyncMutex<Vec<Vec<u8>>>>,
}

struct IfacWorkerBackend(IfacWorkerBackendState);

impl RnodeBleBackend for IfacWorkerBackend {
    async fn connect(&mut self) -> Result<(), String> {
        Ok(())
    }
    async fn subscribe_notifications(&mut self) -> Result<(), String> {
        Ok(())
    }
    async fn write(&mut self, write: RnodeBleWrite) -> Result<(), String> {
        self.0.writes.lock().await.push(write.payload);
        Ok(())
    }
    async fn next_notification(&mut self) -> Result<Option<Vec<u8>>, String> {
        Ok(self.0.incoming.lock().await.pop_front())
    }
}

fn worker_ifac_state(config: InterfaceSharedConfig) -> IfacState {
    Arc::new(std::sync::RwLock::new(
        config.ifac_context_with_default_size(8).expect("derive worker IFAC context"),
    ))
}

fn worker_wire_packet(state: &IfacState, packet: &Packet) -> Vec<u8> {
    encode_data_frame(&encode_packet_ifac(state, packet).expect("encode worker IFAC packet"))
}

#[tokio::test]
async fn rnode_ble_kiss_worker_authenticates_ifac_egress_and_admission() {
    let shared = InterfaceSharedConfig {
        network_name: Some("rnode-ble-ifac".into()),
        passphrase: Some("worker-test-key".into()),
        ..InterfaceSharedConfig::default()
    };
    let wrong = InterfaceSharedConfig {
        network_name: shared.network_name.clone(),
        passphrase: Some("wrong-worker-key".into()),
        ..InterfaceSharedConfig::default()
    };
    let mut manager = InterfaceManager::new(8);
    let interface = NativeRnodeBleKissInterface::new(
        "software-rnode-ble-ifac",
        NativeRnodeBleSettings::for_peripheral("fake-peripheral"),
        RnodeBleKissConfig { max_write_len: 508, ..RnodeBleKissConfig::default() },
    );
    let context = manager.new_context(interface);
    let address = *context.channel.address();
    let state = context.channel.ifac_state.clone();
    assert!(manager.set_shared_config(address, shared));
    let wrong_state = worker_ifac_state(wrong);
    let rejected = Packet {
        destination: AddressHash::new_from_slice(&[0x81; 16]),
        data: PacketDataBuffer::new_from_slice(b"wrong key"),
        ..Packet::default()
    };
    let accepted = Packet {
        destination: AddressHash::new_from_slice(&[0x82; 16]),
        data: PacketDataBuffer::new_from_slice(b"matching key"),
        ..Packet::default()
    };
    let state_for_backend = IfacWorkerBackendState::default();
    {
        let mut incoming = state_for_backend.incoming.lock().await;
        incoming.push_back(worker_wire_packet(&wrong_state, &rejected));
        incoming.push_back(worker_wire_packet(&state, &accepted));
    }
    let receiver = manager.receiver();
    let violations = context.channel.ifac_violations.clone();
    let cancel = context.cancel.clone();
    let backend = state_for_backend.clone();
    let task =
        tokio::spawn(NativeRnodeBleKissInterface::spawn_with_backend_factory(context, move |_| {
            IfacWorkerBackend(backend.clone())
        }));

    let ingress = timeout(std::time::Duration::from_secs(1), async {
        loop {
            let mut rx = receiver.lock().await;
            if let Ok(message) = rx.try_recv() {
                break message;
            }
            drop(rx);
            sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("matching-key BLE KISS ingress is admitted");
    assert_eq!(ingress.packet.destination, accepted.destination);
    assert_eq!(ingress.packet.data.as_slice(), b"matching key");
    assert_eq!(violations.load(AtomicOrdering::Relaxed), 1);
    assert!(receiver.lock().await.try_recv().is_err(), "wrong-key packet never reaches routing");

    let outbound = Packet {
        destination: AddressHash::new_from_slice(&[0x83; 16]),
        data: PacketDataBuffer::new_from_slice(b"authenticated BLE egress"),
        ..Packet::default()
    };
    manager
        .send(TxMessage { packet: outbound.clone(), tx_type: TxMessageType::Direct(address) })
        .await;
    let wire = timeout(std::time::Duration::from_secs(1), async {
        loop {
            for write in state_for_backend.writes.lock().await.iter() {
                if let Ok(frames) = decode_frames(write, 508) {
                    for frame in frames {
                        if let KissFrame::Data(bytes) = frame {
                            if crate::iface::decode_packet_ifac(&state, &bytes).is_ok() {
                                return bytes;
                            }
                        }
                    }
                }
            }
            sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("worker emits authenticated KISS egress");
    let decoded =
        crate::iface::decode_packet_ifac(&state, &wire).expect("egress IFAC authenticates");
    assert_eq!(decoded.destination, outbound.destination);
    assert_eq!(decoded.data.as_slice(), b"authenticated BLE egress");

    cancel.cancel();
    timeout(std::time::Duration::from_secs(1), task)
        .await
        .expect("worker stops on cancellation")
        .expect("worker task joins");
}

#[tokio::test]
async fn rnode_ble_virtual_child_uses_inherited_ifac_for_ingress_and_egress() {
    let inherited = InterfaceSharedConfig {
        network_name: Some("rnode-ble-child-ifac".into()),
        passphrase: Some("inherited-child-key".into()),
        ..InterfaceSharedConfig::default()
    };
    let wrong = worker_ifac_state(InterfaceSharedConfig {
        network_name: inherited.network_name.clone(),
        passphrase: Some("wrong-child-key".into()),
        ..InterfaceSharedConfig::default()
    });
    let mut manager = InterfaceManager::new(8);
    let interface = NativeRnodeBleKissInterface::new(
        "software-rnode-ble-child-ifac",
        NativeRnodeBleSettings::for_peripheral("fake-peripheral"),
        RnodeBleKissConfig { max_write_len: 508, ..RnodeBleKissConfig::default() },
    );
    let context = manager.new_context(interface);
    let host = *context.channel.address();
    assert!(manager.set_shared_config(host, inherited.clone()));
    let host_ifac_state = context.channel.ifac_state.clone();
    let child = manager
        .register_virtual_iface(host, crate::iface::IfaceRole::VirtualUnicast)
        .expect("virtual peer is registered on BLE host");
    let child_runtime = manager
        .ifaces
        .iter()
        .find(|iface| iface.address == child)
        .expect("virtual child runtime");
    assert_eq!(manager.shared_config(&child), Some(&inherited));
    assert!(Arc::ptr_eq(&host_ifac_state, &child_runtime.ifac_state));
    let child_ifac_state = child_runtime.ifac_state.clone();

    let rejected = Packet {
        destination: AddressHash::new_from_slice(&[0x91; 16]),
        data: PacketDataBuffer::new_from_slice(b"wrong inherited key"),
        ..Packet::default()
    };
    let accepted = Packet {
        destination: AddressHash::new_from_slice(&[0x92; 16]),
        data: PacketDataBuffer::new_from_slice(b"inherited key"),
        ..Packet::default()
    };
    let backend_state = IfacWorkerBackendState::default();
    {
        let mut incoming = backend_state.incoming.lock().await;
        incoming.push_back(worker_wire_packet(&wrong, &rejected));
        incoming.push_back(worker_wire_packet(&host_ifac_state, &accepted));
    }
    let receiver = manager.receiver();
    let violations = context.channel.ifac_violations.clone();
    let cancel = context.cancel.clone();
    let backend = backend_state.clone();
    let task = tokio::spawn(NativeRnodeBleKissInterface::spawn_with_backend_factory(
        context,
        move |_| IfacWorkerBackend(backend.clone()),
    ));

    let ingress = timeout(std::time::Duration::from_secs(1), async {
        loop {
            let mut rx = receiver.lock().await;
            if let Ok(message) = rx.try_recv() {
                break message;
            }
            drop(rx);
            sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("matching inherited-key ingress is admitted");
    assert_eq!(ingress.packet.destination, accepted.destination);
    assert_eq!(ingress.packet.data.as_slice(), b"inherited key");
    assert_eq!(violations.load(AtomicOrdering::Relaxed), 1);
    assert!(receiver.lock().await.try_recv().is_err(), "wrong-key child traffic is never delivered");

    let outbound = Packet {
        destination: AddressHash::new_from_slice(&[0x93; 16]),
        data: PacketDataBuffer::new_from_slice(b"virtual child egress"),
        ..Packet::default()
    };
    let trace = manager
        .send(TxMessage { packet: outbound.clone(), tx_type: TxMessageType::Direct(child) })
        .await;
    assert_eq!(trace.sent_ifaces, 1, "virtual-child route uses the BLE host transmit channel");
    let wire = timeout(std::time::Duration::from_secs(1), async {
        loop {
            for write in backend_state.writes.lock().await.iter() {
                if let Ok(frames) = decode_frames(write, 508) {
                    for frame in frames {
                        if let KissFrame::Data(bytes) = frame {
                            if let Ok(packet) = decode_packet_ifac(&child_ifac_state, &bytes) {
                                if packet.destination == outbound.destination {
                                    return bytes;
                                }
                            }
                        }
                    }
                }
            }
            sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("BLE worker emits egress authenticated by the inherited child policy");
    let decoded = decode_packet_ifac(&child_ifac_state, &wire)
        .expect("virtual-child IFAC policy authenticates egress");
    assert_eq!(decoded.destination, outbound.destination);
    assert_eq!(decoded.data.as_slice(), b"virtual child egress");

    cancel.cancel();
    timeout(std::time::Duration::from_secs(1), task)
        .await
        .expect("worker stops on cancellation")
        .expect("worker task joins");
}

#[derive(Default)]
struct StartupRetryState {
    attempts: AtomicUsize,
    cleanups: AtomicUsize,
    io: IfacWorkerBackendState,
}

struct StartupRetryBackend {
    state: Arc<StartupRetryState>,
    fail_connect: bool,
    fail_notification: bool,
}

impl RnodeBleBackend for StartupRetryBackend {
    async fn connect(&mut self) -> Result<(), String> {
        if self.fail_connect {
            Err("injected BLE connect failure".into())
        } else {
            Ok(())
        }
    }

    async fn subscribe_notifications(&mut self) -> Result<(), String> {
        Ok(())
    }

    async fn write(&mut self, _write: RnodeBleWrite) -> Result<(), String> {
        Ok(())
    }

    async fn next_notification(&mut self) -> Result<Option<Vec<u8>>, String> {
        if self.fail_notification {
            return Err("injected BLE notification failure".into());
        }
        Ok(self.state.io.incoming.lock().await.pop_front())
    }

    async fn cleanup(&mut self) -> Result<(), String> {
        self.state.cleanups.fetch_add(1, AtomicOrdering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn rnode_ble_worker_cleans_up_failed_startup_before_retry_and_stop() {
    let mut manager = InterfaceManager::new(8);
    let interface = NativeRnodeBleKissInterface::new(
        "software-rnode-ble-startup-retry",
        NativeRnodeBleSettings::for_peripheral("fake-peripheral"),
        RnodeBleKissConfig { max_write_len: 508, ..RnodeBleKissConfig::default() },
    )
    .with_reconnect_backoff(std::time::Duration::from_millis(1));
    let context = manager.new_context(interface);
    let cancel = context.cancel.clone();
    let state = Arc::new(StartupRetryState::default());
    let worker_state = state.clone();
    let task = tokio::spawn(NativeRnodeBleKissInterface::spawn_with_backend_factory(
        context,
        move |_| {
            let attempt = worker_state.attempts.fetch_add(1, AtomicOrdering::SeqCst);
            StartupRetryBackend { state: worker_state.clone(), fail_connect: attempt == 0, fail_notification: false }
        },
    ));

    timeout(std::time::Duration::from_secs(1), async {
        while state.attempts.load(AtomicOrdering::SeqCst) < 2 {
            sleep(std::time::Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("worker retries after a failed BLE startup");
    assert!(state.cleanups.load(AtomicOrdering::SeqCst) >= 1,
        "failed startup backend is cleaned before another backend is created");

    cancel.cancel();
    timeout(std::time::Duration::from_secs(1), task)
        .await
        .expect("worker stops after cancellation")
        .expect("worker task joins");
    assert_eq!(state.attempts.load(AtomicOrdering::SeqCst), 2);
    assert!(state.cleanups.load(AtomicOrdering::SeqCst) >= 2,
        "active retry backend is cleaned when the worker stops");
}

#[tokio::test]
async fn rnode_ble_startup_retry_keeps_ifac_required_and_counts_plaintext_rejection() {
    let shared = InterfaceSharedConfig {
        network_name: Some("rnode-ble-retry-ifac".into()),
        passphrase: Some("retry-test-key".into()),
        ..InterfaceSharedConfig::default()
    };
    let mut manager = InterfaceManager::new(8);
    let interface = NativeRnodeBleKissInterface::new(
        "software-rnode-ble-ifac-restart",
        NativeRnodeBleSettings::for_peripheral("fake-peripheral"),
        RnodeBleKissConfig { max_write_len: 508, ..RnodeBleKissConfig::default() },
    )
    .with_reconnect_backoff(std::time::Duration::from_millis(1));
    let context = manager.new_context(interface);
    let address = *context.channel.address();
    let ifac_state = context.channel.ifac_state.clone();
    let violations = context.channel.ifac_violations.clone();
    assert!(manager.set_shared_config(address, shared));

    let plaintext = Packet {
        destination: AddressHash::new_from_slice(&[0x94; 16]),
        data: PacketDataBuffer::new_from_slice(b"plaintext after failed startup"),
        ..Packet::default()
    };
    let authenticated = Packet {
        destination: AddressHash::new_from_slice(&[0x95; 16]),
        data: PacketDataBuffer::new_from_slice(b"authenticated after retry"),
        ..Packet::default()
    };
    let state = Arc::new(StartupRetryState::default());
    {
        let mut incoming = state.io.incoming.lock().await;
        incoming.push_back(
            encode_data_frame(&plaintext.to_bytes().expect("serialize plaintext test packet")),
        );
        incoming.push_back(worker_wire_packet(&ifac_state, &authenticated));
    }
    let receiver = manager.receiver();
    let cancel = context.cancel.clone();
    let worker_state = state.clone();
    let task = tokio::spawn(NativeRnodeBleKissInterface::spawn_with_backend_factory(
        context,
        move |_| {
            let attempt = worker_state.attempts.fetch_add(1, AtomicOrdering::SeqCst);
            StartupRetryBackend { state: worker_state.clone(), fail_connect: attempt == 0, fail_notification: false }
        },
    ));

    let ingress = timeout(std::time::Duration::from_secs(1), async {
        loop {
            let mut rx = receiver.lock().await;
            if let Ok(message) = rx.try_recv() {
                break message;
            }
            drop(rx);
            sleep(std::time::Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("authenticated ingress is admitted after the failed startup retry");
    assert_eq!(ingress.packet.destination, authenticated.destination);
    assert_eq!(ingress.packet.data.as_slice(), b"authenticated after retry");
    assert_eq!(violations.load(AtomicOrdering::Relaxed), 1);
    assert!(receiver.lock().await.try_recv().is_err(), "plaintext never reaches transport routing");
    assert!(
        state.cleanups.load(AtomicOrdering::SeqCst) >= 1,
        "failed startup backend is cleaned"
    );

    cancel.cancel();
    timeout(std::time::Duration::from_secs(1), task)
        .await
        .expect("worker stops after cancellation")
        .expect("worker task joins");
}

include!("ifac_runtime_failure_test.rs");
