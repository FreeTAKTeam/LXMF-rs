use crate::hash::AddressHash;
use crate::iface::{IfacState, InterfaceManager, InterfaceSharedConfig, TxMessage, TxMessageType};
use crate::packet::{Packet, PacketDataBuffer};
use std::sync::atomic::{AtomicUsize, Ordering};
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
    assert_eq!(violations.load(Ordering::Relaxed), 1);
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

#[derive(Default)]
struct StartupRetryState {
    attempts: AtomicUsize,
    cleanups: AtomicUsize,
}

struct StartupRetryBackend {
    state: Arc<StartupRetryState>,
    fail_connect: bool,
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
        Ok(None)
    }

    async fn cleanup(&mut self) -> Result<(), String> {
        self.state.cleanups.fetch_add(1, Ordering::SeqCst);
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
            let attempt = worker_state.attempts.fetch_add(1, Ordering::SeqCst);
            StartupRetryBackend { state: worker_state.clone(), fail_connect: attempt == 0 }
        },
    ));

    timeout(std::time::Duration::from_secs(1), async {
        while state.attempts.load(Ordering::SeqCst) < 2 {
            sleep(std::time::Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("worker retries after a failed BLE startup");
    assert!(state.cleanups.load(Ordering::SeqCst) >= 1,
        "failed startup backend is cleaned before another backend is created");

    cancel.cancel();
    timeout(std::time::Duration::from_secs(1), task)
        .await
        .expect("worker stops after cancellation")
        .expect("worker task joins");
    assert_eq!(state.attempts.load(Ordering::SeqCst), 2);
    assert!(state.cleanups.load(Ordering::SeqCst) >= 2,
        "active retry backend is cleaned when the worker stops");
}
