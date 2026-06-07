use lxmf_sdk::app::{OperationEntry, OperationRegistry};
use rns_rpc::{RpcDaemon, RpcRequest};
use serde_json::json;
use serde_json::Value as JsonValue;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("workspace root")
        .to_path_buf()
}

fn read_workspace_text(path: &str) -> String {
    let full_path = workspace_root().join(path);
    std::fs::read_to_string(&full_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", full_path.display()))
}

fn read_json(path: &str) -> JsonValue {
    serde_json::from_str(&read_workspace_text(path))
        .unwrap_or_else(|err| panic!("failed to parse {path}: {err}"))
}

fn rpc_request(id: u64, method: &str, params: JsonValue) -> RpcRequest {
    RpcRequest { id, method: method.to_owned(), params: Some(params) }
}

#[test]
fn sdk_operation_runtime_r3akt_catalog_fixture_is_shared_runtime_ready() {
    let catalog = read_json("docs/fixtures/sdk-operation-runtime/r3akt.catalog.json");
    assert_eq!(catalog["catalog_family"].as_str(), Some("sdk-operation-runtime"));
    assert_eq!(catalog["catalog_release"].as_str(), Some("v1"));
    assert_eq!(catalog["product"].as_str(), Some("r3akt"));
    assert_eq!(catalog["runtime_owner"].as_str(), Some("LXMF-rs"));

    let operations_json = catalog["custom_operations"].clone();
    let operations: Vec<OperationEntry> =
        serde_json::from_value(operations_json).expect("catalog custom operations");
    let registry = OperationRegistry::built_in()
        .merged(operations)
        .expect("catalog should merge with shared runtime");

    for (alias, canonical) in [
        ("R3AKT;EMergencyMessages.send", "r3akt.message.send"),
        ("GET /api/markers", "r3akt.marker.list"),
        ("R3AKT;Presence.announce", "r3akt.presence.announce"),
    ] {
        assert_eq!(registry.canonicalize(alias).expect("canonical operation").as_str(), canonical);
    }

    let flows = catalog["parity_flows"].as_array().expect("parity flows");
    for required in ["message_send", "marker_list", "presence_announce"] {
        assert!(
            flows.iter().any(|flow| flow["id"].as_str() == Some(required)),
            "missing parity flow {required}"
        );
    }
}

#[test]
fn sdk_operation_runtime_extraction_docs_define_product_boundary() {
    let guide = read_workspace_text("docs/sdk/operation-runtime-extraction.md");
    for required in [
        "docs/fixtures/sdk-operation-runtime/r3akt.catalog.json",
        "LXMF-rs owns",
        "Product repos own",
        "custom_operations",
        "sdk_envelope_execute_v2",
        "R3AKTClient",
    ] {
        assert!(guide.contains(required), "operation extraction guide missing {required}");
    }
}

#[test]
fn sdk_operation_runtime_r3akt_catalog_executes_aliases_through_daemon_envelopes() {
    let catalog = read_json("docs/fixtures/sdk-operation-runtime/r3akt.catalog.json");
    let operations = catalog["custom_operations"].as_array().expect("custom operations").clone();
    let flows = catalog["parity_flows"].as_array().expect("parity flows").clone();
    let daemon = RpcDaemon::test_instance();

    let negotiated = daemon
        .handle_rpc(rpc_request(
            55_000,
            "sdk_negotiate_v2",
            json!({
                "supported_contract_versions": [2],
                "requested_capabilities": ["sdk.capability.remote_commands"],
                "config": {
                    "profile": "desktop-full",
                    "extensions": {
                        "custom_operations": operations
                    }
                }
            }),
        ))
        .expect("negotiate catalog");
    assert!(negotiated.error.is_none());

    for (index, flow) in flows.iter().enumerate() {
        let alias = flow["legacy_alias"].as_str().expect("legacy alias");
        let expected = flow["operation_id"].as_str().expect("operation id");
        let response = daemon
            .handle_rpc(rpc_request(
                55_010 + index as u64,
                "sdk_envelope_execute_v2",
                json!({
                    "operation_id": alias,
                    "kind": catalog_kind_for(&catalog, expected),
                    "target": "r3akt-hub",
                    "payload": {
                        "fixture_flow": flow["id"].as_str().expect("flow id")
                    },
                    "extensions": {
                        "product": "r3akt",
                        "source": "sdk-operation-runtime-fixture"
                    }
                }),
            ))
            .expect("execute catalog alias");
        assert!(response.error.is_none(), "{alias} should execute without envelope error");
        let result = response.result.expect("envelope result");
        assert_eq!(result["response"]["operation_id"].as_str(), Some(expected));
        assert_eq!(
            result["response"]["payload"]["command"].as_str(),
            Some(expected),
            "custom catalog operations should delegate to shared command runtime"
        );
        assert_eq!(result["response"]["extensions"]["product"].as_str(), Some("r3akt"));
    }
}

fn catalog_kind_for(catalog: &JsonValue, operation_id: &str) -> String {
    catalog["custom_operations"]
        .as_array()
        .expect("custom operations")
        .iter()
        .find(|operation| operation["id"].as_str() == Some(operation_id))
        .and_then(|operation| operation["kind"].as_str())
        .unwrap_or_else(|| panic!("missing operation kind for {operation_id}"))
        .to_owned()
}
