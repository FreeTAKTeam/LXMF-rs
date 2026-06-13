include!("inbound_worker_parts/module_prelude.rs");

#[path = "inbound_control.rs"]
mod control;

#[path = "inbound_delivery_events.rs"]
mod delivery_events;

#[path = "inbound_propagation.rs"]
mod propagation;

#[path = "inbound_routing.rs"]
mod routing;

include!("inbound_worker_parts/module_core.rs");

include!("inbound_worker_parts/inbound_propagation_payload_is_inges.rs");
