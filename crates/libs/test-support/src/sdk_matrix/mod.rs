use lxmf_sdk::{required_capabilities, supports_capability, Profile};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug)]
struct CapabilityRow {
    capability: String,
    desktop_full: String,
    desktop_local_runtime: String,
    embedded_alloc: String,
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("workspace root")
        .to_path_buf()
}

fn load_feature_matrix() -> String {
    let path = workspace_root().join("docs/contracts/sdk-v2-feature-matrix.md");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
}

fn parse_capability_rows(markdown: &str) -> Vec<CapabilityRow> {
    let mut rows = Vec::new();
    let mut in_table = false;

    for line in markdown.lines() {
        let trimmed = line.trim();
        if !in_table {
            if trimmed.starts_with("| Capability ID |")
                && trimmed.contains("| desktop-full |")
                && trimmed.contains("| embedded-alloc |")
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
        if cells.len() != 4 {
            continue;
        }
        let capability = cells[0].trim_matches('`').to_owned();
        if !capability.starts_with("sdk.capability.") {
            continue;
        }

        rows.push(CapabilityRow {
            capability,
            desktop_full: cells[1].to_ascii_lowercase(),
            desktop_local_runtime: cells[2].to_ascii_lowercase(),
            embedded_alloc: cells[3].to_ascii_lowercase(),
        });
    }

    rows
}

fn normalize_status(raw: &str) -> &str {
    if raw.starts_with("required") {
        return "required";
    }
    if raw.starts_with("optional") {
        return "optional";
    }
    if raw.starts_with("unsupported") {
        return "unsupported";
    }
    if raw.starts_with("experimental") {
        return "experimental";
    }
    raw
}

fn assert_profile_status(profile: Profile, capability: &str, status: &str) {
    let normalized = normalize_status(status);
    let required = required_capabilities(profile.clone()).contains(&capability);
    let supported = supports_capability(profile.clone(), capability);
    match normalized {
        "required" => {
            assert!(
                required && supported,
                "{capability} marked required in matrix but code does not require/support it for {:?}",
                profile
            );
        }
        "optional" => {
            assert!(
                !required && supported,
                "{capability} marked optional in matrix but code does not match optional support for {:?}",
                profile
            );
        }
        "unsupported" => {
            assert!(
                !supported,
                "{capability} marked unsupported in matrix but code supports it for {:?}",
                profile
            );
        }
        "experimental" => {}
        other => panic!("unknown matrix status '{other}' for capability {capability}"),
    }
}

#[test]
fn sdk_matrix_capability_table_matches_profile_capabilities() {
    let markdown = load_feature_matrix();
    let rows = parse_capability_rows(&markdown);
    assert!(!rows.is_empty(), "feature matrix capability table is empty");

    let mut seen = HashMap::new();
    for row in &rows {
        seen.insert(row.capability.clone(), ());
        assert_profile_status(
            Profile::DesktopFull,
            row.capability.as_str(),
            row.desktop_full.as_str(),
        );
        assert_profile_status(
            Profile::DesktopLocalRuntime,
            row.capability.as_str(),
            row.desktop_local_runtime.as_str(),
        );
        assert_profile_status(
            Profile::EmbeddedAlloc,
            row.capability.as_str(),
            row.embedded_alloc.as_str(),
        );
    }

    const KNOWN_CAPABILITIES: &[&str] = &[
        "sdk.capability.cursor_replay",
        "sdk.capability.async_events",
        "sdk.capability.manual_tick",
        "sdk.capability.token_auth",
        "sdk.capability.mtls_auth",
        "sdk.capability.receipt_terminality",
        "sdk.capability.config_revision_cas",
        "sdk.capability.idempotency_ttl",
    ];
    for capability in KNOWN_CAPABILITIES {
        assert!(seen.contains_key(*capability), "matrix missing known capability row {capability}");
    }

    for capability in required_capabilities(Profile::DesktopFull) {
        assert!(
            seen.contains_key(*capability),
            "matrix missing desktop-full required capability {capability}"
        );
    }
    for capability in required_capabilities(Profile::DesktopLocalRuntime) {
        assert!(
            seen.contains_key(*capability),
            "matrix missing desktop-local-runtime required capability {capability}"
        );
    }
    for capability in required_capabilities(Profile::EmbeddedAlloc) {
        assert!(
            seen.contains_key(*capability),
            "matrix missing embedded-alloc required capability {capability}"
        );
    }
}
