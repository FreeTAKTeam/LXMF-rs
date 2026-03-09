mod capabilities;
mod config;
mod delivery;
mod discovery;
mod envelope;
mod errors;
mod events;
mod node;
mod operations;
mod profiles;

pub use capabilities::CapabilitySummary;
pub use delivery::{
    AttemptDisposition, BackoffSchedule, DeliveryAttempt, DeliveryOptions, DeliveryPlan,
    QueuePressurePolicy, QueuePressureStrategy, ReconnectPolicy, RetryPolicy, SendReport,
    TimeoutPolicy,
};
pub use discovery::{
    BootstrapRequest, Contact, ContactPage, ContactUpdate, Identity, PeerDirectoryEntry, Presence,
    PresencePage,
};
pub use envelope::{Envelope, EnvelopeKind, EnvelopeResponse};
pub use errors::{Error, ErrorCategory, ErrorCode};
pub use events::{
    Event, EventBatch, EventKind, EventMetadata, Severity, StreamGapDetails, SubscriptionStart,
};
#[cfg(feature = "sdk-async")]
pub use node::EventStream;
pub use node::{
    Client, Config, DeliveryState, DeliveryStatus, Handle, Profile, RunState, RuntimeStatus,
    SendReceipt, SendRequest,
};
pub use operations::{
    OperationEntry, OperationId, OperationKind, OperationRegistry, RegistryError, TransportVariant,
};
