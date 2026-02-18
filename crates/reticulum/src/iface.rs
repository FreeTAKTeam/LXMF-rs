pub mod driver;
pub mod hdlc;
pub mod tcp_client;
pub mod tcp_server;
pub mod udp;

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task;
use tokio_util::sync::CancellationToken;

use crate::hash::AddressHash;
use crate::hash::Hash;
use crate::packet::Packet;

pub use driver::{InterfaceDriver, InterfaceDriverFactory};

pub type InterfaceTxSender = mpsc::Sender<TxMessage>;
pub type InterfaceTxReceiver = mpsc::Receiver<TxMessage>;

pub type InterfaceRxSender = mpsc::Sender<RxMessage>;
pub type InterfaceRxReceiver = mpsc::Receiver<RxMessage>;

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum TxMessageType {
    Broadcast(Option<AddressHash>),
    Direct(AddressHash),
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub struct TxMessage {
    pub tx_type: TxMessageType,
    pub packet: Packet,
}

#[derive(Debug, Default, PartialEq, Eq, Copy, Clone)]
pub struct TxDispatchTrace {
    pub matched_ifaces: usize,
    pub sent_ifaces: usize,
    pub failed_ifaces: usize,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub struct RxMessage {
    pub address: AddressHash, // Address of source interface
    pub packet: Packet,       // Received packet
}

pub struct InterfaceChannel {
    pub address: AddressHash,
    pub rx_channel: InterfaceRxSender,
    pub tx_channel: InterfaceTxReceiver,
    pub stop: CancellationToken,
}

impl InterfaceChannel {
    pub fn make_rx_channel(cap: usize) -> (InterfaceRxSender, InterfaceRxReceiver) {
        mpsc::channel(cap)
    }

    pub fn make_tx_channel(cap: usize) -> (InterfaceTxSender, InterfaceTxReceiver) {
        mpsc::channel(cap)
    }

    pub fn new(
        rx_channel: InterfaceRxSender,
        tx_channel: InterfaceTxReceiver,
        address: AddressHash,
        stop: CancellationToken,
    ) -> Self {
        Self { address, rx_channel, tx_channel, stop }
    }

    pub fn address(&self) -> &AddressHash {
        &self.address
    }

    pub fn split(self) -> (InterfaceRxSender, InterfaceTxReceiver) {
        (self.rx_channel, self.tx_channel)
    }
}

pub trait Interface {
    fn mtu() -> usize;
}

struct LocalInterface {
    address: AddressHash,
    tx_send: InterfaceTxSender,
    stop: CancellationToken,
}

pub struct InterfaceContext<T: Interface> {
    pub inner: Arc<Mutex<T>>,
    pub channel: InterfaceChannel,
    pub cancel: CancellationToken,
}

pub struct InterfaceManager {
    counter: usize,
    rx_recv: Arc<tokio::sync::Mutex<InterfaceRxReceiver>>,
    rx_send: InterfaceRxSender,
    cancel: CancellationToken,
    ifaces: Vec<LocalInterface>,
}

const DEFAULT_IFACE_TX_QUEUE_CAPACITY: usize = 128;
const IFACE_TX_ENQUEUE_TIMEOUT_MS: u64 = 200;

fn tx_diag_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("RETICULUMD_DIAGNOSTICS")
            .or_else(|_| std::env::var("RETICULUM_TRANSPORT_DIAGNOSTICS"))
            .ok()
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on" | "debug"
                )
            })
            .unwrap_or(false)
    })
}

impl InterfaceManager {
    pub fn new(rx_cap: usize) -> Self {
        let (rx_send, rx_recv) = InterfaceChannel::make_rx_channel(rx_cap);
        let rx_recv = Arc::new(tokio::sync::Mutex::new(rx_recv));

        Self { counter: 0, rx_recv, rx_send, cancel: CancellationToken::new(), ifaces: Vec::new() }
    }

    pub fn new_channel(&mut self, tx_cap: usize) -> InterfaceChannel {
        self.counter += 1;

        let counter_bytes = self.counter.to_le_bytes();
        let address = AddressHash::new_from_hash(&Hash::new_from_slice(&counter_bytes[..]));

        let (tx_send, tx_recv) = InterfaceChannel::make_tx_channel(tx_cap);

        log::debug!("iface: create channel {}", address);

        let stop = CancellationToken::new();

        self.ifaces.push(LocalInterface { address, tx_send, stop: stop.clone() });

        InterfaceChannel { rx_channel: self.rx_send.clone(), tx_channel: tx_recv, address, stop }
    }

    pub fn new_context<T: Interface>(&mut self, inner: T) -> InterfaceContext<T> {
        let channel = self.new_channel(DEFAULT_IFACE_TX_QUEUE_CAPACITY);

        let inner = Arc::new(Mutex::new(inner));

        InterfaceContext::<T> { inner: inner.clone(), channel, cancel: self.cancel.clone() }
    }

    pub fn spawn<T: Interface, F, R>(&mut self, inner: T, worker: F) -> AddressHash
    where
        F: FnOnce(InterfaceContext<T>) -> R,
        R: std::future::Future<Output = ()> + Send + 'static,
        R::Output: Send + 'static,
    {
        let context = self.new_context(inner);
        let address = *context.channel.address();

        task::spawn(worker(context));

        address
    }

    pub fn receiver(&self) -> Arc<tokio::sync::Mutex<InterfaceRxReceiver>> {
        self.rx_recv.clone()
    }

    pub fn cleanup(&mut self) {
        self.ifaces.retain(|iface| !iface.stop.is_cancelled());
    }

    pub async fn send(&self, message: TxMessage) -> TxDispatchTrace {
        let mut trace = TxDispatchTrace::default();
        for iface in &self.ifaces {
            let should_send = match message.tx_type {
                TxMessageType::Broadcast(address) => {
                    let mut should_send = true;
                    if let Some(address) = address {
                        should_send = address != iface.address;
                    }

                    should_send
                }
                TxMessageType::Direct(address) => address == iface.address,
            };

            if should_send && !iface.stop.is_cancelled() {
                trace.matched_ifaces += 1;
                match iface.tx_send.try_send(message) {
                    Ok(()) => {
                        trace.sent_ifaces += 1;
                    }
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        // Fall back to a short async wait before dropping. This avoids
                        // dropping critical packets (link proofs, receipts) under bursts.
                        match tokio::time::timeout(
                            Duration::from_millis(IFACE_TX_ENQUEUE_TIMEOUT_MS),
                            iface.tx_send.send(message),
                        )
                        .await
                        {
                            Ok(Ok(())) => {
                                trace.sent_ifaces += 1;
                                log::warn!(
                                    "iface: recovered from full tx queue on {} for {:?}",
                                    iface.address,
                                    message.tx_type
                                );
                                eprintln!(
                                    "[tp-diag] iface enqueue recovered iface={} tx_type={:?}",
                                    iface.address, message.tx_type
                                );
                            }
                            Ok(Err(_)) => {
                                trace.failed_ifaces += 1;
                                log::warn!(
                                    "iface: tx queue closed on {} for {:?}",
                                    iface.address,
                                    message.tx_type
                                );
                                eprintln!(
                                    "[tp-diag] iface enqueue closed iface={} tx_type={:?}",
                                    iface.address, message.tx_type
                                );
                            }
                            Err(_) => {
                                trace.failed_ifaces += 1;
                                log::warn!(
                                    "iface: tx queue full timeout on {} for {:?}",
                                    iface.address,
                                    message.tx_type
                                );
                                eprintln!(
                                    "[tp-diag] iface enqueue timeout iface={} tx_type={:?}",
                                    iface.address, message.tx_type
                                );
                            }
                        }
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        trace.failed_ifaces += 1;
                        log::warn!(
                            "iface: tx queue closed on {} for {:?}",
                            iface.address,
                            message.tx_type
                        );
                        eprintln!(
                            "[tp-diag] iface enqueue closed iface={} tx_type={:?}",
                            iface.address, message.tx_type
                        );
                    }
                }
            }
        }

        if tx_diag_enabled() {
            eprintln!(
                "[tp-diag] iface_dispatch tx_type={:?} dst={} matched={} sent={} failed={}",
                message.tx_type,
                message.packet.destination,
                trace.matched_ifaces,
                trace.sent_ifaces,
                trace.failed_ifaces
            );
            log::info!(
                "[tp-diag] iface_dispatch tx_type={:?} dst={} matched={} sent={} failed={}",
                message.tx_type,
                message.packet.destination,
                trace.matched_ifaces,
                trace.sent_ifaces,
                trace.failed_ifaces
            );
        }

        trace
    }
}

impl Drop for InterfaceManager {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
