#[cfg(test)]
mod tests {
include!("test_validated_peer_links_sections/core_tests.rs");
include!("test_validated_peer_links_sections/offer_request_invalid_peering_key_st.rs");
include!("test_validated_peer_links_sections/offer_request_defers_capacity_limite.rs");
include!("test_validated_peer_links_sections/message_get_lists_fetches_and_purges.rs");
include!("test_validated_peer_links_sections/message_get_haves_clear_stale_unhand.rs");
include!("test_validated_peer_links_sections/message_get_purges_haves_before_reje.rs");
include!("test_validated_peer_links_sections/message_get_false_transfer_limit_ski.rs");
}
