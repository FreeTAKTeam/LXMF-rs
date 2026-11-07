#[cfg(test)]
#[path = "outbound_message_sections"]
mod tests {
include!("outbound_message_sections/core_tests.rs");
include!("outbound_message_sections/folded_peer_lookup.rs");
include!("outbound_message_sections/propagation_metadata.rs");
include!("outbound_message_sections/peer_queue_stats_merge_case_variant.rs");
include!("outbound_message_sections/received_report_does_not_downgrade_t.rs");
include!("outbound_message_sections/priority_pruning_preserves_prioritis.rs");
include!("outbound_message_sections/peer_queue_refill_is_bounded.rs");
include!("outbound_message_sections/peer_queue_refill_incremental_tests.rs");
include!("outbound_message_sections/peer_entry_prune_pages.rs");
#[path = "peer_entry_quota_tests.rs"]
mod peer_entry_quota_tests;
#[path = "peer_entry_budget_tests.rs"]
mod peer_entry_budget_tests;
include!("outbound_message_sections/wal_maintenance_tests.rs");
#[path = "sqlite_cache_release_tests.rs"]
mod sqlite_cache_release_tests;
include!("outbound_message_sections/broker_control_tests.rs");
}
