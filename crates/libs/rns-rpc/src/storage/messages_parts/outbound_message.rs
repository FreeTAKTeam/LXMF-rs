#[cfg(test)]
mod tests {
include!("outbound_message_sections/core_tests.rs");
include!("outbound_message_sections/peer_queue_stats_merge_case_variant.rs");
include!("outbound_message_sections/received_report_does_not_downgrade_t.rs");
include!("outbound_message_sections/priority_pruning_preserves_prioritis.rs");
}
