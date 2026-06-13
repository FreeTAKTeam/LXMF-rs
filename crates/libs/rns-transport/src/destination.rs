pub mod link;

pub mod link_map;

include!("destination_parts/module_prelude.rs");

#[path = "destination/primitives.rs"]
mod primitives;

#[path = "destination/ratchet.rs"]
mod ratchet;

#[cfg(test)]
#[path = "destination/tests.rs"]
mod tests;

include!("destination_parts/module_core.rs");

include!("destination_parts/destination.rs");
