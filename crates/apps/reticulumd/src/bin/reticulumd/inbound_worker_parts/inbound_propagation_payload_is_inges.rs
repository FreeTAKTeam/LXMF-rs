#[cfg(test)]
mod tests {
include!("inbound_propagation_payload_is_inges_sections/core_tests.rs");
include!("inbound_propagation_payload_is_inges_sections/inbound_propagation_accepts_stamp_wi.rs");
include!("inbound_propagation_payload_is_inges_sections/local_propagated_delivery_processed_tr.rs");
include!("inbound_propagation_payload_is_inges_sections/propagated_signature_metadata.rs");
include!("inbound_propagation_payload_is_inges_sections/duplicate_direct_delivery_packet_doe.rs");
include!("inbound_propagation_payload_is_inges_sections/direct_delivery_success_sdk_events.rs");
}
