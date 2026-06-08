//! RPC boundary crate for protocol and daemon contracts.

pub mod e2e_harness;
pub mod rpc;
mod storage;
mod transport;

pub use lxmf_reference::{
    PYTHON_LXMF_REFERENCE_REF, PYTHON_LXMF_REFERENCE_VERSION, PYTHON_RETICULUM_REFERENCE_REF,
    PYTHON_RETICULUM_REFERENCE_VERSION, RETICULUM_CONFORMANCE_REFERENCE_REF,
};
pub use rpc::http;
pub use rpc::{
    AnnounceBridge, DeliveryPolicy, DeliveryTraceEntry, EventSinkBridge, InterfaceMutationBridge,
    InterfaceRecord, OutboundBridge, OutboundDeliveryOptions, PaperDecodeOutcome,
    PaperEncodeEnvelope, PeerRecord, PropagationState, RemoteControlBridge, RpcDaemon, RpcError,
    RpcEvent, RpcEventSinkEnvelope, RpcRequest, RpcResponse, SdkCustomOperationSpec, StampPolicy,
    TicketRecord,
};
pub use storage::messages::{AnnounceRecord, MessageRecord, MessagesStore};
