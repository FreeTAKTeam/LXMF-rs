use super::workspace_root;
use std::fs;

#[test]
fn failure_injection_matrix_lists_required_rows() {
    let path = workspace_root().join("docs/contracts/failure-injection-matrix.md");
    let body = fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));

    let required = [
        "offset_mismatch",
        "unknown_upload_id",
        "commit_incomplete",
        "checksum_mismatch",
        "duplicate_chunk_same_bytes",
        "duplicate_chunk_conflict",
        "seq_gap",
        "forced_disconnect_mid_transfer",
        "queue_timeout_exhausted",
    ];

    for row in required {
        assert!(body.contains(row), "failure matrix missing required scenario row: {row}");
    }
}
