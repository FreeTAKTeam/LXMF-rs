include!("inbound_control_parts/module_prelude.rs");

#[path = "inbound_control_peer.rs"]
mod peer_commands;

#[path = "inbound_control_propagation.rs"]
mod propagation_commands;

#[path = "inbound_control_response.rs"]
mod response;

#[path = "inbound_control_status.rs"]
mod status;

include!("inbound_control_parts/module_core.rs");

include!("inbound_control_parts/test_validated_peer_links.rs");
