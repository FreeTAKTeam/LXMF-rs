pub mod link;

pub mod link_map;

include!("destination_parts/part_001_part_001.rs");

#[path = "destination/primitives.rs"]
mod primitives;

#[path = "destination/ratchet.rs"]
mod ratchet;

#[cfg(test)]
#[path = "destination/tests.rs"]
mod tests;

include!("destination_parts/part_002_part_002.rs");

include!("destination_parts/part_003_destination.rs");
