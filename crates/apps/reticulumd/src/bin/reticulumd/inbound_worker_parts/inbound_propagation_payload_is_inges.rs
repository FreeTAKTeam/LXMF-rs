#[cfg(test)]
mod tests {
include!("inbound_propagation_payload_is_inges_sections/core_tests.rs");
include!("inbound_propagation_payload_is_inges_sections/resource_progress_status.rs");
include!("inbound_propagation_payload_is_inges_sections/outbound_resource_rejection.rs");
include!("inbound_propagation_payload_is_inges_sections/resource_terminal_consumer.rs");
mod partial_inbound_resource_teardown {
    use super::*;
    include!("inbound_propagation_payload_is_inges_sections/partial_inbound_resource_teardown.rs");
}
include!("inbound_propagation_payload_is_inges_sections/inbound_propagation_accepts_stamp_wi.rs");
include!("inbound_propagation_payload_is_inges_sections/local_propagated_delivery_processed_tr.rs");
include!("inbound_propagation_payload_is_inges_sections/propagated_signature_metadata.rs");
include!("inbound_propagation_payload_is_inges_sections/duplicate_direct_delivery_packet_event.rs");
include!("inbound_propagation_payload_is_inges_sections/duplicate_direct_delivery_packet_doe.rs");
include!("inbound_propagation_payload_is_inges_sections/duplicate_direct_resource_drop.rs");
include!("inbound_propagation_payload_is_inges_sections/malformed_direct_resource_drop.rs");
include!("inbound_propagation_payload_is_inges_sections/propagated_predecode_destination_mismatch.rs");
include!("inbound_propagation_payload_is_inges_sections/direct_delivery_success_sdk_events.rs");
}
