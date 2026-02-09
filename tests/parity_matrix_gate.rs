use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug)]
struct ParityItem {
    status: String,
    tests: Vec<String>,
}

fn parse_parity_items(text: &str) -> BTreeMap<String, ParityItem> {
    let mut items = BTreeMap::new();

    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with("- PARITY_ITEM ") {
            continue;
        }

        let mut id: Option<String> = None;
        let mut status: Option<String> = None;
        let mut tests: Option<Vec<String>> = None;

        for token in line.split_whitespace().skip(2) {
            let Some((key, value)) = token.split_once('=') else {
                continue;
            };

            match key {
                "id" => id = Some(value.to_string()),
                "status" => status = Some(value.to_string()),
                "tests" => {
                    let parsed = if value == "none" {
                        Vec::new()
                    } else {
                        value
                            .split(',')
                            .filter(|entry| !entry.is_empty())
                            .map(|entry| entry.to_string())
                            .collect()
                    };
                    tests = Some(parsed);
                }
                _ => {}
            }
        }

        let id = id.expect("PARITY_ITEM must include id=...");
        let status = status.expect("PARITY_ITEM must include status=...");
        let tests = tests.expect("PARITY_ITEM must include tests=...");
        items.insert(id, ParityItem { status, tests });
    }

    items
}

#[test]
fn parity_matrix_has_required_method_items() {
    let text = std::fs::read_to_string("docs/plans/lxmf-parity-matrix.md").unwrap();
    let items = parse_parity_items(&text);

    let required = [
        "message.pack_wire",
        "message.unpack_wire",
        "message.storage_roundtrip",
        "message.propagation_pack_unpack",
        "message.paper_pack",
        "message.signature_verify",
        "stamper.generate_stamp",
        "stamper.cancel_work",
        "ticket.validity_with_grace",
        "peer.serialize_roundtrip",
        "peer.queue_accounting",
        "peer.acceptance_rate",
        "router.outbound_queue",
        "router.handle_outbound_policy",
        "router.cancel_outbound",
        "router.propagation_ingest_fetch",
        "router.transfer_state_lifecycle",
        "handlers.delivery_callback",
        "handlers.propagation_app_data",
        "lxmd.config_load",
        "lxmd.serve_sync_unpeer_status",
    ];

    for id in required {
        assert!(items.contains_key(id), "missing PARITY_ITEM id={id}");
    }
}

#[test]
fn done_parity_items_require_behavior_tests() {
    let text = std::fs::read_to_string("docs/plans/lxmf-parity-matrix.md").unwrap();
    let items = parse_parity_items(&text);

    for (id, item) in &items {
        match item.status.as_str() {
            "done" => {
                assert!(
                    !item.tests.is_empty(),
                    "done item must list tests for id={id}"
                );
                for test in &item.tests {
                    assert!(
                        Path::new(test).exists(),
                        "listed test path does not exist for id={id}: {test}"
                    );
                }
            }
            "partial" | "not-started" => {}
            other => panic!("invalid status for id={id}: {other}"),
        }
    }
}

#[test]
fn reticulum_matrix_is_current_and_actionable() {
    let text = std::fs::read_to_string("docs/plans/reticulum-parity-matrix.md").unwrap();
    assert!(text.contains("Last verified:"));
    assert!(text.contains("RNS/Transport.py"));
    assert!(!text.contains("| not-started |"));

    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with("| `RNS/") {
            continue;
        }
        let columns: Vec<_> = line.split('|').map(str::trim).collect();
        let status = columns.get(3).copied().unwrap_or_default();
        assert_eq!(status, "done", "reticulum module row must be done: {line}");
    }
}

#[test]
fn lxmf_module_map_and_compatibility_matrix_are_fully_done() {
    let module_text = std::fs::read_to_string("docs/plans/lxmf-parity-matrix.md").unwrap();
    for line in module_text.lines() {
        let line = line.trim();
        if !line.starts_with("| `LXMF/") {
            continue;
        }
        let columns: Vec<_> = line.split('|').map(str::trim).collect();
        let status = columns.get(3).copied().unwrap_or_default();
        assert_eq!(status, "done", "lxmf module map row must be done: {line}");
    }

    let compatibility_text = std::fs::read_to_string("docs/compatibility-matrix.md").unwrap();
    for line in compatibility_text.lines() {
        let line = line.trim();
        if !line.starts_with("| `LXMF/") {
            continue;
        }
        let columns: Vec<_> = line.split('|').map(str::trim).collect();
        let status = columns.get(3).copied().unwrap_or_default();
        assert_eq!(
            status, "done",
            "compatibility matrix row must be done: {line}"
        );
    }
}
