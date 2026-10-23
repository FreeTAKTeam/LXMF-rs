include!("tests_parts/module_prelude.rs");

include!("tests_parts/path_table_api.rs");

include!("tests_parts/reticulum_path_restore_bad_cache.rs");

include!("tests_parts/unchanged_path_cache.rs");

include!("tests_parts/blackholed_path_restore.rs");

include!("tests_parts/expired_route_restart_recovery.rs");

include!("tests_parts/path_request_duplicate_scoping.rs");

include!("tests_parts/unknown_path_request_answered_by_announce.rs");

include!("tests_parts/pending_out_link_rediscovery.rs");
include!("tests_parts/pending_out_link_establishment_timeout.rs");
include!("tests_parts/concurrent_out_link_acquisition.rs");
include!("tests_parts/receipt_handler_on_shared_transport.rs");

include!("tests_parts/tunnel_restore_freshness.rs");

include!("tests_parts/held_udp_announce_preserves_peer_sou.rs");

include!("tests_parts/roaming_path_response_suppression.rs");

include!("tests_parts/announce_identity_drift.rs");

include!("tests_parts/announce_broadcast_policy.rs");
include!("tests_parts/announce_table_retransmission_gate.rs");
include!("tests_parts/pinned_python_local_announce_schedule.rs");
include!("tests_parts/disabled_transport_shared_child_local_destination.rs");
include!("tests_parts/enabled_transport_shared_child_local_destination.rs");
include!("tests_parts/disabled_shared_child_local_announce.rs");
include!("tests_parts/enabled_shared_child_local_announce.rs");
include!("tests_parts/shared_child_remote_link_request.rs");
include!("tests_parts/shared_owner_unknown_link_request.rs");

include!("tests_parts/encrypted_resource_control_packet.rs");

include!("tests_parts/routed_link_packets.rs");

include!("tests_parts/transport_register_channel_handler_d.rs");

include!("tests_parts/inbound_proof_link_selection.rs");

include!("tests_parts/transport_register_channel_handler_e.rs");

include!("tests_parts/unicast_iface_for_source_returns_non.rs");

include!("tests_parts/inbound_link_request_registers_unicast.rs");

include!("tests_parts/blackhole_path_eviction.rs");

include!("tests_parts/rns_1_5_ingress_admission.rs");
include!("tests_parts/ordinary_proof_duplicate.rs");
include!("tests_parts/plain_group_packet_duplicates.rs");
include!("tests_parts/duplicate_filter_python_matrix.rs");
include!("tests_parts/packet_replay_restart.rs");
include!("tests_parts/hashlist_save_authority.rs");
include!("tests_parts/accepted_child_ifac_rotation.rs");

#[path = "tests_parts/shared_attachment_activation.rs"]
mod shared_attachment_activation;

include!("tests_parts/reticulum_runtime_management.rs");

include!("tests_parts/packet_proof_correlation.rs");
include!("tests_parts/prepared_packet_proof.rs");

include!("tests_parts/single_destination_delivery_proof.rs");

include!("tests_parts/link_broadcast_helpers_route_via_bound_iface.rs");

include!("tests_parts/resource_completion.rs");
include!("tests_parts/link_lifecycle.rs");
include!("tests_parts/cache_local_link_ingress.rs");
include!("tests_parts/cache_transit_link_proof.rs");
include!("tests_parts/cache_pending_link_proof.rs");
include!("tests_parts/link_packet_receipt.rs");

#[path = "tests/request_receipts.rs"]
mod request_receipts;

#[path = "tests/live_child_policy.rs"]
mod live_child_policy;

#[path = "tests_parts/interface_retirement_fence.rs"]
mod interface_retirement_fence;
