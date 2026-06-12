#[cfg(test)]
mod tests {
include!("part_003_outbound_message_sections/tests_001_section_001.rs");
include!("part_003_outbound_message_sections/tests_002_peer_queue_stats_merge_case_variant_.rs");
include!("part_003_outbound_message_sections/tests_003_received_report_does_not_downgrade_t.rs");
include!("part_003_outbound_message_sections/tests_004_priority_pruning_preserves_prioritis.rs");
}
