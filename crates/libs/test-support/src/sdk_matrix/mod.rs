use lxmf_sdk::{default_memory_budget, required_capabilities, supports_capability, Profile};
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

#[derive(Debug)]
struct MemoryBudgetRow {
    profile: String,
    max_heap_bytes: usize,
    max_event_queue_bytes: usize,
    max_attachment_spool_bytes: usize,
}

#[derive(Debug)]
struct NoStdAuditRow {
    crate_name: String,
    status: String,
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

fn load_compatibility_matrix() -> String {
    let path = workspace_root().join("docs/contracts/compatibility-matrix.md");
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

fn parse_memory_budget_rows(markdown: &str) -> Vec<MemoryBudgetRow> {
    let mut rows = Vec::new();
    let mut in_table = false;

    for line in markdown.lines() {
        let trimmed = line.trim();
        if !in_table {
            if trimmed.starts_with("| Profile |")
                && trimmed.contains("| max_heap_bytes |")
                && trimmed.contains("| max_event_queue_bytes |")
                && trimmed.contains("| max_attachment_spool_bytes |")
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
        rows.push(MemoryBudgetRow {
            profile: cells[0].trim_matches('`').to_ascii_lowercase(),
            max_heap_bytes: parse_budget_cell(cells[1]),
            max_event_queue_bytes: parse_budget_cell(cells[2]),
            max_attachment_spool_bytes: parse_budget_cell(cells[3]),
        });
    }

    rows
}

fn parse_nostd_audit_rows(markdown: &str) -> Vec<NoStdAuditRow> {
    let mut rows = Vec::new();
    let mut in_table = false;

    for line in markdown.lines() {
        let trimmed = line.trim();
        if !in_table {
            if trimmed.starts_with("| Crate |")
                && trimmed.contains("| std_required |")
                && trimmed.contains("| alloc_target |")
                && trimmed.contains("| status |")
                && trimmed.contains("| removal_plan |")
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
        if cells.len() != 5 {
            continue;
        }
        rows.push(NoStdAuditRow {
            crate_name: cells[0].trim_matches('`').to_ascii_lowercase(),
            status: cells[3].trim_matches('`').to_ascii_lowercase(),
        });
    }

    rows
}

fn parse_budget_cell(raw: &str) -> usize {
    raw.replace(',', "")
        .parse::<usize>()
        .unwrap_or_else(|err| panic!("invalid memory budget value '{raw}': {err}"))
}

fn parse_table_first_column(markdown: &str, header_pattern: &str) -> Vec<String> {
    let mut rows = Vec::new();
    let mut in_table = false;

    for line in markdown.lines() {
        let trimmed = line.trim();
        if !in_table {
            if trimmed.starts_with(header_pattern) {
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
        if let Some(first_cell) = cells.first() {
            rows.push(first_cell.trim_matches('`').to_ascii_lowercase());
        }
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
        "sdk.capability.topics",
        "sdk.capability.topic_subscriptions",
        "sdk.capability.topic_fanout",
        "sdk.capability.telemetry_query",
        "sdk.capability.telemetry_stream",
        "sdk.capability.attachments",
        "sdk.capability.attachment_delete",
        "sdk.capability.attachment_streaming",
        "sdk.capability.markers",
        "sdk.capability.identity_multi",
        "sdk.capability.identity_discovery",
        "sdk.capability.identity_import_export",
        "sdk.capability.identity_hash_resolution",
        "sdk.capability.contact_management",
        "sdk.capability.paper_messages",
        "sdk.capability.remote_commands",
        "sdk.capability.voice_signaling",
        "sdk.capability.group_delivery",
        "sdk.capability.shared_instance_rpc_auth",
        "sdk.capability.key_management",
        "sdk.capability.plugin_host",
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

#[test]
fn sdk_matrix_release_windows_and_clients_cover_n_n1_n2_contracts() {
    let markdown = load_compatibility_matrix();

    let window_rows = parse_table_first_column(&markdown, "| Window |");
    assert!(window_rows.contains(&"n".to_string()), "compatibility matrix missing window N row");
    assert!(
        window_rows.contains(&"n+1".to_string()),
        "compatibility matrix missing window N+1 row"
    );
    assert!(
        window_rows.contains(&"n+2".to_string()),
        "compatibility matrix missing window N+2 row"
    );

    let client_rows = parse_table_first_column(&markdown, "| Client |");
    for client in ["lxmf-sdk", "reticulumd", "sideband", "rch", "columba"] {
        assert!(
            client_rows.iter().any(|row| row.contains(client)),
            "compatibility matrix missing required client row containing '{client}'"
        );
    }
}

#[test]
fn sdk_memory_budget_table_matches_profile_budgets() {
    let markdown = load_feature_matrix();
    let rows = parse_memory_budget_rows(&markdown);
    assert!(!rows.is_empty(), "feature matrix memory budget table is empty");

    let mut by_profile = HashMap::new();
    for row in rows {
        by_profile.insert(row.profile.clone(), row);
    }

    for (profile_key, profile) in [
        ("desktop-full", Profile::DesktopFull),
        ("desktop-local-runtime", Profile::DesktopLocalRuntime),
        ("embedded-alloc", Profile::EmbeddedAlloc),
    ] {
        let row = by_profile
            .get(profile_key)
            .unwrap_or_else(|| panic!("missing memory budget row for profile '{profile_key}'"));
        let budget = default_memory_budget(profile);
        assert_eq!(
            row.max_heap_bytes, budget.max_heap_bytes,
            "memory budget drift for {profile_key}: max_heap_bytes"
        );
        assert_eq!(
            row.max_event_queue_bytes, budget.max_event_queue_bytes,
            "memory budget drift for {profile_key}: max_event_queue_bytes"
        );
        assert_eq!(
            row.max_attachment_spool_bytes, budget.max_attachment_spool_bytes,
            "memory budget drift for {profile_key}: max_attachment_spool_bytes"
        );
    }
}

#[test]
fn sdk_nostd_capability_table_has_required_rows() {
    let markdown = load_feature_matrix();
    let rows = parse_nostd_audit_rows(&markdown);
    assert!(!rows.is_empty(), "feature matrix no_std audit table is empty");

    let mut by_crate = HashMap::new();
    for row in rows {
        assert!(
            matches!(row.status.as_str(), "std-first" | "alloc-ready" | "planned"),
            "invalid no_std audit status '{}'",
            row.status
        );
        by_crate.insert(row.crate_name.clone(), row);
    }

    for required in ["lxmf-core", "rns-core"] {
        assert!(by_crate.contains_key(required), "no_std audit table missing crate '{required}'");
    }

    let lxmf_core_manifest =
        fs::read_to_string(workspace_root().join("crates/libs/lxmf-core/Cargo.toml"))
            .expect("read lxmf-core Cargo.toml");
    assert!(
        lxmf_core_manifest.contains("default = [\"std\"]")
            && lxmf_core_manifest.contains("alloc = []"),
        "lxmf-core Cargo.toml must declare std default and alloc feature for no_std audit tracking"
    );

    let rns_core_manifest =
        fs::read_to_string(workspace_root().join("crates/libs/rns-core/Cargo.toml"))
            .expect("read rns-core Cargo.toml");
    assert!(
        rns_core_manifest.contains("default = [\"std\"]")
            && rns_core_manifest.contains("alloc = []"),
        "rns-core Cargo.toml must declare std default and alloc feature for no_std audit tracking"
    );
}
