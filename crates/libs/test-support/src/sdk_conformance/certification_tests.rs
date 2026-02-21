use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("workspace root")
}

fn compatibility_matrix() -> String {
    let path = workspace_root().join("docs/contracts/compatibility-matrix.md");
    fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!("failed to read compatibility matrix at {}: {}", path.display(), err)
    })
}

#[test]
fn sdk_conformance_certification_matrix_defines_tiers_and_gate() {
    let matrix = compatibility_matrix();
    for marker in [
        "## Third-Party Conformance Certification",
        "| Bronze |",
        "| Silver |",
        "| Gold |",
        "cargo run -p xtask -- certification-report-check",
    ] {
        assert!(
            matrix.contains(marker),
            "compatibility matrix missing certification marker '{marker}'"
        );
    }
}

#[test]
fn sdk_conformance_certification_tiers_reference_required_gate_set() {
    let matrix = compatibility_matrix();
    for marker in [
        "interop-matrix-check",
        "interop-corpus-check",
        "sdk-conformance",
        "compat-kit-check",
        "schema-client-check",
        "plugin-negotiation-check",
        "e2e-compatibility",
        "security-review-check",
        "key-management-check",
    ] {
        assert!(
            matrix.contains(marker),
            "compatibility matrix missing certification gate marker '{marker}'"
        );
    }
}
