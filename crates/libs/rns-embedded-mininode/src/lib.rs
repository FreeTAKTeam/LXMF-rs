#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod adapters;
pub mod config;
pub mod error;
pub mod event;
pub mod node;
pub mod store;
pub mod telemetry;

pub use adapters::{FrameLink, MemoryLink};
pub use config::MiniNodeConfig;
pub use error::MiniNodeError;
pub use event::NodeEvent;
pub use node::{MessageEnvelope, MiniNode, NeighborRecord};
pub use store::{MemoryStore, MiniNodeStore, NeighborSnapshot, NodeSnapshot};
pub use telemetry::{
    BatteryStatus, DeviceHealth, LinkStats, PositionFix, TelemetryPoint, TelemetryQuery,
    TelemetrySample, TelemetryValue,
};
