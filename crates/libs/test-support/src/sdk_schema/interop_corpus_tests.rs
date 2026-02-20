use super::*;
use std::collections::HashSet;

fn load_contract_schema(path: &str, name: &str) -> jsonschema::JSONSchema {
    let root = workspace_root();
    let schema = read_json(&root.join(path));
    compile_schema(&schema, name)
}

#[test]
fn sdk_interop_corpus_schema_validates_manifest() {
    let corpus_schema = load_contract_schema(
        "docs/schemas/contract-v2/interop-golden-corpus.schema.json",
        "contract-v2/interop-golden-corpus",
    );
    let corpus = fixture("docs/fixtures/interop/v1/golden-corpus.json");
    assert_schema_valid(&corpus_schema, "docs/fixtures/interop/v1/golden-corpus.json", &corpus);

    let entries = corpus
        .get("entries")
        .and_then(serde_json::Value::as_array)
        .expect("golden corpus entries must be an array");
    let mut unique_ids = HashSet::new();
    for entry in entries {
        let id = entry
            .get("id")
            .and_then(serde_json::Value::as_str)
            .expect("golden corpus entry id must be string");
        assert!(
            unique_ids.insert(id.to_string()),
            "golden corpus contains duplicate entry id '{id}'"
        );
    }
}

#[test]
fn sdk_interop_corpus_entries_replay_against_contract_schemas() {
    let payload_schema =
        load_contract_schema("docs/schemas/contract-v2/payload-envelope.schema.json", "payload");
    let event_schema =
        load_contract_schema("docs/schemas/contract-v2/event-payload.schema.json", "event");
    let rpc_schemas = load_rpc_core_schemas();
    let corpus = fixture("docs/fixtures/interop/v1/golden-corpus.json");
    let entries = corpus
        .get("entries")
        .and_then(serde_json::Value::as_array)
        .expect("golden corpus entries must be an array");

    for entry in entries {
        let id = entry
            .get("id")
            .and_then(serde_json::Value::as_str)
            .expect("golden corpus entry id must be string");
        let payload =
            entry.get("payload_envelope").expect("golden corpus entry missing payload_envelope");
        let event = entry.get("event_payload").expect("golden corpus entry missing event_payload");
        let rpc_request =
            entry.get("rpc_send_request").expect("golden corpus entry missing rpc_send_request");
        let rpc_response =
            entry.get("rpc_send_response").expect("golden corpus entry missing rpc_send_response");
        let slices = entry
            .get("slices")
            .and_then(serde_json::Value::as_array)
            .expect("golden corpus entry slices must be array");

        let payload_path = format!("golden-corpus:{id}:payload_envelope");
        assert_schema_valid(&payload_schema, payload_path.as_str(), payload);

        let event_path = format!("golden-corpus:{id}:event_payload");
        assert_schema_valid(&event_schema, event_path.as_str(), event);

        let request_path = format!("golden-corpus:{id}:rpc_send_request");
        assert_schema_valid(&rpc_schemas.sdk_send_v2, request_path.as_str(), rpc_request);

        let response_path = format!("golden-corpus:{id}:rpc_send_response");
        assert_schema_valid(&rpc_schemas.sdk_send_v2, response_path.as_str(), rpc_response);

        let slice_values =
            slices.iter().filter_map(serde_json::Value::as_str).collect::<HashSet<_>>();
        for required in ["rpc_v2", "payload_v2", "event_cursor_v2"] {
            assert!(
                slice_values.contains(required),
                "golden corpus entry '{id}' is missing required slice '{required}'"
            );
        }
    }
}
