use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().expect("repo root")
}

#[test]
fn auto_interface_prepared_host_smoke_preserves_evidence_contract() {
    let root = repo_root();
    let script_path = root.join("tools/scripts/auto-interface-prepared-host-smoke.sh");
    let script =
        fs::read_to_string(&script_path).expect("read AutoInterface prepared-host smoke script");

    for required in [
        "target/auto-interface-hil",
        "AUTO_CHURN_NETNS",
        "AUTO_CHURN_DEVICE",
        "AUTO_CHURN_INITIAL_ADDR",
        "AUTO_CHURN_REPLACEMENT_ADDR",
        "[[interfaces]]",
        "AutoInterface",
        "auto-churn-prepared-host",
        "devices = [\"${AUTO_CHURN_DEVICE}\"]",
        "ip netns",
        "type dummy",
        "addrgenmode none",
        "--strict-interface-startup",
        "rnstatus-rs",
        "evidence_scope",
        "linux_namespace_dummy_churn",
        "product_boundary",
        "broader prepared-host parity",
        "zero_initial",
        "added",
        "replaced",
        "removed",
        "adopted_device_count",
        "adopted_add_count",
        "adopted_remove_count",
        "link_local_replacement_count",
        "last_adopted_change",
        "adopted_devices",
        "phase_snapshots",
        "report.json",
    ] {
        assert!(
            script.contains(required),
            "AutoInterface prepared-host smoke should include required token {required:?}"
        );
    }
}

#[test]
fn nightly_hil_workflow_exposes_auto_interface_prepared_host_job() {
    let root = repo_root();
    let workflow_path = root.join(".github/workflows/nightly-embedded-hil.yml");
    let workflow = fs::read_to_string(&workflow_path).expect("read nightly HIL workflow");

    for required in [
        "auto-interface-prepared-host",
        "HIL_AUTO_INTERFACE_ENABLED",
        "HIL_AUTO_INTERFACE_NETNS",
        "HIL_AUTO_INTERFACE_DEVICE",
        "HIL_AUTO_INTERFACE_INITIAL_ADDR",
        "HIL_AUTO_INTERFACE_REPLACEMENT_ADDR",
        "HIL_AUTO_INTERFACE_TIMEOUT_SECS",
        "./tools/scripts/auto-interface-prepared-host-smoke.sh",
        "auto-interface-prepared-host-artifacts",
        "target/auto-interface-hil/report.json",
        "target/auto-interface-hil/run.*",
    ] {
        assert!(
            workflow.contains(required),
            "nightly HIL workflow should include required token {required:?}"
        );
    }
}

#[test]
fn auto_interface_runbook_documents_prepared_host_churn_artifacts() {
    let root = repo_root();
    let runbook_path = root.join("docs/runbooks/reticulumd-auto-interface.md");
    let runbook = fs::read_to_string(&runbook_path).expect("read AutoInterface runbook");

    for required in [
        "Prepared-Host Churn Smoke",
        "./tools/scripts/auto-interface-prepared-host-smoke.sh",
        "AUTO_CHURN_DEVICE=lxauto0",
        "ip netns",
        "_runtime.auto.carrier_runtime.adopted_add_count",
        "_runtime.auto.carrier_runtime.adopted_remove_count",
        "_runtime.auto.carrier_runtime.link_local_replacement_count",
        "target/auto-interface-hil/",
        "report.json",
        "evidence_scope",
        "linux_namespace_dummy_churn",
        "product_boundary",
        "broader prepared-host parity",
        "phase snapshots",
        "HIL_AUTO_INTERFACE_ENABLED",
    ] {
        assert!(
            runbook.contains(required),
            "AutoInterface runbook should document required token {required:?}"
        );
    }
}
