//! Extension interfaces for out-of-tree hardware/runtime adapters.
//!
//! Proprietary or platform-specific drivers should implement these traits in
//! external crates and integrate through the public interface manager API.

use super::AddressHash;

/// Minimal metadata contract for an interface driver.
pub trait InterfaceDriver: Send + Sync {
    /// Stable driver identifier for metrics/config mapping.
    fn driver_id(&self) -> &'static str;

    /// Link MTU supported by this driver.
    fn mtu(&self) -> usize;
}

/// Factory contract used by host runtimes to register external drivers.
pub trait InterfaceDriverFactory: Send + Sync {
    type Driver: InterfaceDriver;

    fn create(&self, local_address: AddressHash) -> Self::Driver;
}
