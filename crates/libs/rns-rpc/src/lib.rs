//! RPC boundary crate for protocol and daemon contracts.

pub mod e2e_harness;
pub mod rpc;
mod storage;
mod transport;

pub use rpc::http;
pub use rpc::{
    AnnounceBridge, DeliveryPolicy, DeliveryTraceEntry, InterfaceMutationBridge, InterfaceRecord,
    OutboundBridge, OutboundDeliveryOptions, PaperDecodeOutcome, PaperEncodeEnvelope, PeerRecord,
    PropagationState, RemoteControlBridge, RpcDaemon, RpcError, RpcEvent, RpcRequest, RpcResponse,
    StampPolicy, TicketRecord,
};
pub use storage::messages::{AnnounceRecord, MessageRecord, MessagesStore};
