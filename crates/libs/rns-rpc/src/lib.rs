//! RPC boundary crate for protocol and daemon contracts.

pub mod e2e_harness;
pub mod rpc;
mod storage;
mod transport;

pub use rpc::http;
pub use rpc::{
    AnnounceBridge, ControlEnvelope, ControlMessage, ControlRole, ControlRouterProcessStatus,
    DeliveryPolicy, DeliveryTraceEntry, EventSinkBridge, InterfaceMutationBridge, InterfaceRecord,
    InterfaceWorkerProcessStatus, OutboundBridge, OutboundDeliveryOptions, PaperDecodeOutcome,
    PaperEncodeEnvelope, PeerRecord, PropagationState, RemoteControlBridge, RpcDaemon, RpcError,
    RpcEvent, RpcEventSinkEnvelope, RpcRequest, RpcResponse, StampPolicy, TicketRecord,
    WorkerProcessStatus,
};
pub use storage::messages::{AnnounceRecord, MessageRecord, MessagesStore};
