include!("inbound_control_parts/part_001_part_001.rs");

#[path = "inbound_control_peer.rs"]
mod peer_commands;

#[path = "inbound_control_propagation.rs"]
mod propagation_commands;

#[path = "inbound_control_response.rs"]
mod response;

#[path = "inbound_control_status.rs"]
mod status;

include!("inbound_control_parts/part_002_part_002.rs");

include!("inbound_control_parts/part_003_test_validated_peer_links.rs");
