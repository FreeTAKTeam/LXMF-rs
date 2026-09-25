pub use driver::{InterfaceDriver, InterfaceDriverFactory};

/// Configured endpoint data for app boundaries that render a compatible
/// TCP-client path-table interface description. It describes only that
/// configured interface, not virtual interfaces hosted by it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpClientPathTableMetadata {
    pub name: String,
    pub target_host: String,
    pub target_port: u16,
}

pub type InterfaceTxSender = mpsc::Sender<TxMessage>;
pub type InterfaceTxReceiver = mpsc::Receiver<TxMessage>;

pub type InterfaceRxSender = mpsc::Sender<RxMessage>;
pub type InterfaceRxReceiver = mpsc::Receiver<RxMessage>;
