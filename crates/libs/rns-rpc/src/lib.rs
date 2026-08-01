//! RPC boundary crate for protocol and daemon contracts.

pub mod e2e_harness;
pub mod rpc;
mod storage;
mod transport;

pub use lxmf_reference::{
    current_software_parity_orientation, ParityCheckpoint, ParityInventory, ParityLevel,
    ParityRatio, ReferenceRevision, SoftwareParityOrientation, SoftwareParityReferences,
    PYTHON_LXMF_REFERENCE_REF, PYTHON_LXMF_REFERENCE_VERSION, PYTHON_RETICULUM_REFERENCE_REF,
    PYTHON_RETICULUM_REFERENCE_VERSION, PYTHON_SOFTWARE_PARITY_COMPLETE,
    PYTHON_SOFTWARE_PARITY_LEVEL, PYTHON_SOFTWARE_PARITY_NOT_APPLICABLE,
    PYTHON_SOFTWARE_PARITY_PARTIAL, PYTHON_SOFTWARE_PARITY_TOTAL,
    RETICULUM_CONFORMANCE_REFERENCE_REF,
};
pub use rpc::http;
pub use rpc::{
    AnnounceBridge, DeliveryPolicy, DeliveryTraceEntry, EventSinkBridge, InterfaceMutationBridge,
    InterfaceRecord, OutboundBridge, OutboundDeliveryOptions, PaperDecodeOutcome,
    PaperEncodeEnvelope, PathLookupBridge, PeerRecord, PropagationState, RNodeManagementBridge,
    RemoteControlBridge, RpcDaemon, RpcError, RpcEvent, RpcEventSinkEnvelope, RpcRequest,
    RpcResponse, SdkCustomOperationSpec, ServiceIdentityBridge, ServiceIdentityRecord,
    ServiceIdentitySpec, StampPolicy, TicketRecord, WeaveDisplayControlBridge,
};
pub use storage::messages::{AnnounceRecord, MessageRecord, MessagesStore};
