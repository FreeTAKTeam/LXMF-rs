#[cfg(test)]
mod tests {
use crate::iface::auto::{AutoDiscoveryScope, MulticastAddressType};
include!("tests_sections/core_tests.rs");
include!("tests_sections/auto_multicast_discovery_bind_resolv.rs");
include!("tests_sections/auto_peer_data_transport_bridge_regi.rs");
}
