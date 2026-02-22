use std::fs;
use std::path::PathBuf;

#[derive(Debug)]
struct CutoverRow {
    owner: String,
    classification: String,
    replacement: String,
    removal_version: String,
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("workspace root")
        .to_path_buf()
}

fn load_cutover_map() -> String {
    let path = workspace_root().join("docs/migrations/sdk-v2.5-cutover-map.md");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
}

fn parse_cutover_rows(markdown: &str) -> Result<Vec<CutoverRow>, String> {
    let mut rows = Vec::new();
    let mut in_table = false;

    for line in markdown.lines() {
        let trimmed = line.trim();
        if !in_table {
            if trimmed.starts_with("| Surface |")
                && trimmed.contains("| Classification |")
                && trimmed.contains("| Removal version |")
            {
                in_table = true;
            }
            continue;
        }

        if !trimmed.starts_with('|') {
            if !rows.is_empty() {
                break;
            }
            continue;
        }
        if trimmed.contains("---") {
            continue;
        }

        let cells = trimmed.trim_matches('|').split('|').map(str::trim).collect::<Vec<_>>();
        if cells.len() != 7 {
            return Err(format!(
                "malformed cutover row '{trimmed}' (expected 7 columns, found {})",
                cells.len()
            ));
        }
        rows.push(CutoverRow {
            owner: cells[2].to_owned(),
            classification: cells[3].to_ascii_lowercase(),
            replacement: cells[4].to_owned(),
            removal_version: cells[5].to_owned(),
        });
    }

    Ok(rows)
}

#[test]
fn sdk_migration_cutover_map_is_complete() {
    let markdown = load_cutover_map();
    let rows = parse_cutover_rows(&markdown).expect("parse cutover rows");
    assert!(!rows.is_empty(), "cutover map must contain at least one consumer row");

    for (idx, row) in rows.iter().enumerate() {
        assert!(!row.owner.is_empty(), "row {idx} missing owner");
        assert!(!row.classification.is_empty(), "row {idx} missing classification");
        assert!(!row.replacement.is_empty(), "row {idx} missing replacement");
        assert!(!row.removal_version.is_empty(), "row {idx} missing removal version");
        assert!(
            matches!(row.classification.as_str(), "keep" | "wrap" | "deprecate"),
            "row {idx} has invalid classification '{}'",
            row.classification
        );
        if row.classification == "wrap" {
            assert!(
                !row.removal_version.eq_ignore_ascii_case("n/a"),
                "row {idx} with classification=wrap requires explicit removal version"
            );
        }
    }
}
