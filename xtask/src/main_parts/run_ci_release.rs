use super::*;

pub(super) fn run_release_check() -> Result<()> {
    run_pr_core_ci()?;
    run_correctness_check()?;
    run("cargo", &["doc", "--workspace", "--no-deps", "--lib"])?;
    // v0.9.7 explicitly ships with documented partial Python/RNS parity. The
    // inventory remains a required consistency/evidence check, but release
    // promotion must not require every mapped surface row to be complete.
    run_python_surface_parity_check(false)?;
    run_sdk_zmq_parity_check()?;
    run_performance_docs_check()?;
    run_sdk_docs_check()?;
    run_sdk_cookbook_check()?;
    run_sdk_ergonomics_check()?;
    run_lxmf_cli_check()?;
    run_reference_integration_check()?;
    run_dx_bootstrap_check()?;
    run_sdk_incident_runbook_check()?;
    run_sdk_drill_check()?;
    run_sdk_soak_check()?;
    run_interop_artifacts(false)?;
    run_interop_matrix_check()?;
    run_interop_corpus_check()?;
    run_interop_drift_check(false)?;
    run_schema_client_check()?;
    run_compat_kit_check()?;
    run_certification_report_check()?;
    run_e2e_compatibility(None)?;
    run_sdk_conformance()?;
    run_sdk_profile_build()?;
    run_sdk_examples_check()?;
    run_governance_check()?;
    run_interfaces_required()?;
    run_compliance_profile_check()?;
    run_support_policy_check()?;
    run_unsafe_audit_check()?;
    run_supply_chain_check()?;
    run_release_scorecard_check()?;
    run_canary_criteria_check()?;
    run_extension_registry_check()?;
    run_plugin_negotiation_check()?;
    run_security_review_check()?;
    run_sdk_security_check()?;
    run_sdk_api_break()?;
    run_changelog_migration_check()?;
    run_crypto_agility_check()?;
    run_key_management_check()?;
    run_sdk_fuzz_check()?;
    run_sdk_property_check()?;
    run_sdk_model_check()?;
    run_sdk_race_check()?;
    run_sdk_replay_check()?;
    run_sdk_metrics_check()?;
    run_sdk_memory_budget_check()?;
    run_sdk_queue_pressure_check()?;
    run_reproducible_build_check()?;
    run_sdk_matrix_check()?;
    run_embedded_link_check()?;
    run_embedded_native_lock_check()?;
    run_embedded_core_check()?;
    run_embedded_node_build()?;
    run_embedded_node_contract()?;
    run_embedded_node_failure_matrix()?;
    run_embedded_footprint_check()?;
    run_migration_checks()?;
    run_architecture_checks()?;
    Ok(())
}
