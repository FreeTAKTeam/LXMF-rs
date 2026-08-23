//! Pinned reference implementation metadata used by LXMF-rs compatibility gates.

mod parity;

pub use parity::{
    current_software_parity_orientation, ParityCheckpoint, ParityInventory, ParityLevel,
    ParityRatio, ReferenceRevision, SoftwareParityOrientation, SoftwareParityReferences,
};

include!("python_software_parity.rs");

pub const RETICULUM_CONFORMANCE_REFERENCE_REF: &str = "0319444b20e0815f26c6b9ceeba8fa44de037c9b";
pub const PYTHON_RETICULUM_REFERENCE_VERSION: &str = "1.5.0";
pub const PYTHON_RETICULUM_REFERENCE_REF: &str = "e32d4df754a7b87b1bf1bb0d08675d12ff505ae6";
pub const PYTHON_LXMF_REFERENCE_VERSION: &str = "0.9.6";
pub const PYTHON_LXMF_REFERENCE_REF: &str = "727830cefda83d9c6e3982b48675425f3f988f9c";
