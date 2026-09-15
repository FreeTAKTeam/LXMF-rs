//! The AutoInterface driver: sockets, the peering announce loops, the
//! peer/transport bridge and NIC enumeration, over the discovery algorithm in
//! [`super::auto`].
//!
//! A plan ([`AutoRuntimePlan`]) is built from a typed config and the host's
//! link-local candidates, then spawned against an [`InterfaceChannel`] the
//! interface manager hands out with [`IfaceRole::Multicast`]; every peer that
//! authenticates becomes a virtual unicast interface under that host. The
//! returned [`AutoDiscoveryRuntime`] is the only way to stop it.

include!("auto_runtime_parts/module_prelude.rs");
include!("auto_runtime_parts/module_prelude_sections/transport_bridge.rs");

include!("auto_runtime_parts/autodiscoverysocketbindtarget.rs");

include!("auto_runtime_parts/autoruntimeplan.rs");

include!("auto_runtime_parts/plan_builder.rs");
include!("auto_runtime_parts/plan_builder_sections/peer_job_runtime_status.rs");
include!("auto_runtime_parts/plan_builder_sections/runtime_helpers.rs");

include!("auto_runtime_parts/tests.rs");
