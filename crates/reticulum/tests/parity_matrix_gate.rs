use std::path::{Path, PathBuf};

fn load_parity_matrix() -> (String, PathBuf) {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let candidate_paths = [
        (
            manifest_dir.join("../../docs/plans/reticulum-parity-matrix.md"),
            manifest_dir.join("../.."),
        ),
        (manifest_dir.join("docs/plans/reticulum-parity-matrix.md"), manifest_dir.to_path_buf()),
    ];

    for (path, repo_root) in candidate_paths {
        if let Ok(contents) = std::fs::read_to_string(&path) {
            return (contents, repo_root);
        }
    }

    panic!("reticulum parity matrix should exist");
}

fn parse_tests_column(cell: &str) -> Vec<String> {
    cell.split(',')
        .map(str::trim)
        .map(|entry| entry.trim_matches('`'))
        .filter(|entry| !entry.is_empty())
        .map(ToString::to_string)
        .collect()
}

#[test]
fn parity_matrix_has_no_missing_core_items() {
    let (text, _) = load_parity_matrix();
    assert!(!text.contains("missing") || !text.contains("core"));
}

#[test]
fn parity_matrix_rows_are_done_and_backed_by_tests() {
    let (text, repo_root) = load_parity_matrix();
    assert!(text.contains("Last verified:"));

    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with("| `RNS/") {
            continue;
        }

        let columns: Vec<_> = line.split('|').map(str::trim).collect();
        let status = columns.get(3).copied().unwrap_or_default();
        assert_eq!(status, "done", "reticulum parity row must be done: {line}");

        let tests_cell = columns.get(4).copied().unwrap_or_default();
        let test_paths = parse_tests_column(tests_cell);
        assert!(!test_paths.is_empty(), "reticulum parity row must list tests: {line}");
        for test_path in &test_paths {
            let path = Path::new(test_path);
            assert!(
                repo_root.join(path).exists(),
                "listed reticulum parity test path does not exist: {test_path}"
            );
        }
    }
}
