#[path = "support/python_compat_cases.rs"]
mod python_compat_cases;

use python_compat_cases::{
    assert_cases_are_dispatchable_by_harness_and_smoke_script,
    assert_local_evidence_cases_are_dispatchable_by_harness, assert_required_modes_covered,
    assert_smoke_rpc_call_retries_transient_connection_refusals, run_case,
};

#[test]
fn compatibility_matrix_covers_required_modes() {
    assert_required_modes_covered();
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_direct_rust_to_python() {
    run_case("direct_rust_to_python");
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_direct_python_to_rust() {
    run_case("direct_python_to_rust");
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_opportunistic_rust_to_python() {
    run_case("opportunistic_rust_to_python");
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_opportunistic_python_to_rust() {
    run_case("opportunistic_python_to_rust");
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_propagated_rust_to_python() {
    run_case("propagated_rust_to_python");
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_propagated_python_to_rust() {
    run_case("propagated_python_to_rust");
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_a_propagation_remote_status_bidir() {
    run_case("propagation_remote_status_bidir");
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_propagation_remote_fetch_rust_to_python() {
    run_case("propagation_remote_fetch_rust_to_python");
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_propagation_remote_download_rust_to_python() {
    run_case("propagation_remote_download_rust_to_python");
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_propagation_remote_sync_rust_to_python() {
    run_case("propagation_remote_sync_rust_to_python");
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_propagation_get_haves_python_to_rust() {
    run_case("propagation_get_haves_python_to_rust");
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_propagation_offer_python_to_rust() {
    run_case("propagation_offer_python_to_rust");
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_propagation_offer_queue_python_to_rust() {
    run_case("propagation_offer_queue_python_to_rust");
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_propagation_offer_duplicate_wanted_source_completed_python_to_rust() {
    run_case("propagation_offer_duplicate_wanted_source_completed_python_to_rust");
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_link_liveness_rust_to_python() {
    run_case("link_liveness_rust_to_python");
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_link_liveness_python_to_rust() {
    run_case("link_liveness_python_to_rust");
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_link_teardown_rust_to_python() {
    run_case("link_teardown_rust_to_python");
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_link_teardown_python_to_rust() {
    run_case("link_teardown_python_to_rust");
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_resource_transfer() {
    run_case("resource_transfer");
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_lxm_interchange() {
    run_case("lxm_interchange");
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_rns_path_request_rust_to_python() {
    run_case("rns_path_request_rust_to_python");
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_rns_path_request_rust_to_python_scoped_refresh() {
    run_case("rns_path_request_rust_to_python_scoped_refresh");
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_rns_path_request_python_to_rust() {
    run_case("rns_path_request_python_to_rust");
}

#[test]
#[ignore = "runs deterministic local transport-policy evidence through the compatibility harness"]
fn python_compat_rns_path_request_transport_policy() {
    run_case("rns_path_request_transport_policy");
}

#[test]
#[ignore = "runs deterministic local roaming path-response policy evidence through the compatibility harness"]
fn python_compat_rns_path_request_roaming_transport_policy() {
    run_case("rns_path_request_roaming_transport_policy");
}

#[test]
#[ignore = "runs deterministic local roaming path-response grace evidence through the compatibility harness"]
fn python_compat_rns_path_request_roaming_grace_transport_policy() {
    run_case("rns_path_request_roaming_grace_transport_policy");
}

#[test]
#[ignore = "runs deterministic local announce rebroadcast policy evidence through the compatibility harness"]
fn python_compat_rns_announce_rebroadcast_transport_policy() {
    run_case("rns_announce_rebroadcast_transport_policy");
}

#[test]
#[ignore = "runs deterministic local unknown-announce ingress policy evidence through the compatibility harness"]
fn python_compat_rns_unknown_announce_ingress_policy() {
    run_case("rns_unknown_announce_ingress_policy");
}

#[test]
#[ignore = "runs deterministic local LINKREQUEST MTU signalling policy evidence through the compatibility harness"]
fn python_compat_rns_link_request_mtu_transport_policy() {
    run_case("rns_link_request_mtu_transport_policy");
}

#[test]
fn compatibility_cases_are_dispatchable_by_harness_and_smoke_script() {
    assert_cases_are_dispatchable_by_harness_and_smoke_script();
}

#[test]
fn local_evidence_cases_are_dispatchable_by_harness() {
    assert_local_evidence_cases_are_dispatchable_by_harness();
}

#[test]
fn smoke_rpc_call_retries_transient_connection_refusals() {
    assert_smoke_rpc_call_retries_transient_connection_refusals();
}
