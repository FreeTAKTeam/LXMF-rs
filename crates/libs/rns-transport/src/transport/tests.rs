include!("tests_parts/module_prelude.rs");

include!("tests_parts/reticulum_path_restore_bad_cache.rs");

include!("tests_parts/path_request_duplicate_scoping.rs");

include!("tests_parts/unknown_path_request_answered_by_announce.rs");

include!("tests_parts/pending_out_link_rediscovery.rs");
include!("tests_parts/pending_out_link_establishment_timeout.rs");

include!("tests_parts/tunnel_restore_freshness.rs");

include!("tests_parts/held_udp_announce_preserves_peer_sou.rs");

include!("tests_parts/roaming_path_response_suppression.rs");

include!("tests_parts/announce_identity_drift.rs");

include!("tests_parts/announce_broadcast_policy.rs");
include!("tests_parts/announce_table_retransmission_gate.rs");

include!("tests_parts/encrypted_resource_control_packet.rs");

include!("tests_parts/transport_register_channel_handler_d.rs");

include!("tests_parts/inbound_proof_link_selection.rs");

include!("tests_parts/transport_register_channel_handler_e.rs");

include!("tests_parts/unicast_iface_for_source_returns_non.rs");

include!("tests_parts/inbound_link_request_registers_unicast.rs");

include!("tests_parts/blackhole_path_eviction.rs");

include!("tests_parts/rns_1_5_ingress_admission.rs");

include!("tests_parts/reticulum_runtime_management.rs");

include!("tests_parts/packet_proof_correlation.rs");

include!("tests_parts/single_destination_delivery_proof.rs");

include!("tests_parts/link_broadcast_helpers_route_via_bound_iface.rs");
