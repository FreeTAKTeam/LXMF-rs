#[path = "support/python_compat_cases.rs"]
mod python_compat_cases;

use python_compat_cases::{
    assert_cases_are_dispatchable_by_harness_and_smoke_script, assert_required_modes_covered,
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
fn compatibility_cases_are_dispatchable_by_harness_and_smoke_script() {
    assert_cases_are_dispatchable_by_harness_and_smoke_script();
}

#[test]
fn smoke_rpc_call_retries_transient_connection_refusals() {
    assert_smoke_rpc_call_retries_transient_connection_refusals();
}
