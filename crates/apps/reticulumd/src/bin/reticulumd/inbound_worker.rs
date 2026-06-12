include!("inbound_worker_parts/part_001_part_001.rs");

#[path = "inbound_control.rs"]
mod control;

#[path = "inbound_delivery_events.rs"]
mod delivery_events;

#[path = "inbound_propagation.rs"]
mod propagation;

#[path = "inbound_routing.rs"]
mod routing;

include!("inbound_worker_parts/part_002_part_002.rs");

include!("inbound_worker_parts/part_003_inbound_propagation_payload_is_inges.rs");
