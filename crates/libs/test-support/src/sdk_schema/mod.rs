use jsonschema::{Draft, JSONSchema};
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::fs;
use std::path::{Path, PathBuf};

mod cookbook_tests;
mod failure_matrix_tests;
mod fixtures_contract_tests;
mod interop_corpus_tests;
mod rpc_core_tests;
mod rpc_domain_tests;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("workspace root")
        .to_path_buf()
}

fn read_json(path: &Path) -> JsonValue {
    let data = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    serde_json::from_str(&data)
        .unwrap_or_else(|err| panic!("failed to parse {}: {err}", path.display()))
}

fn collect_json_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("failed to read directory {}: {err}", dir.display()));
    for entry in entries {
        let path = entry
            .unwrap_or_else(|err| {
                panic!("failed to read directory entry in {}: {err}", dir.display())
            })
            .path();
        if path.is_dir() {
            collect_json_files(&path, files);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            files.push(path);
        }
    }
}

fn rewrite_ref(value: &mut JsonValue, from: &str, to: &str) {
    match value {
        JsonValue::Object(object) => {
            if let Some(JsonValue::String(current)) = object.get_mut("$ref") {
                if current == from {
                    *current = to.to_owned();
                }
            }
            for nested in object.values_mut() {
                rewrite_ref(nested, from, to);
            }
        }
        JsonValue::Array(items) => {
            for item in items {
                rewrite_ref(item, from, to);
            }
        }
        JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) => {}
    }
}

fn command_schema_with_embedded_config(
    config_schema: &JsonValue,
    mut command_schema: JsonValue,
) -> JsonValue {
    let config_defs = config_schema
        .get("$defs")
        .and_then(JsonValue::as_object)
        .expect("config schema missing $defs");
    let config_root =
        command_schema.as_object_mut().expect("command schema root must be an object");
    let defs = config_root
        .entry("$defs")
        .or_insert_with(|| JsonValue::Object(JsonMap::new()))
        .as_object_mut()
        .expect("command schema $defs must be object");
    for (key, value) in config_defs {
        defs.entry(key.clone()).or_insert_with(|| value.clone());
    }
    rewrite_ref(&mut command_schema, "config.schema.json#/$defs/sdk_config", "#/$defs/sdk_config");
    rewrite_ref(
        &mut command_schema,
        "config.schema.json#/$defs/config_patch",
        "#/$defs/config_patch",
    );
    command_schema
}

fn compile_schema(schema: &JsonValue, name: &str) -> JSONSchema {
    JSONSchema::options()
        .with_draft(Draft::Draft202012)
        .compile(schema)
        .unwrap_or_else(|err| panic!("failed to compile {name} schema: {err}"))
}

fn assert_schema_valid(schema: &JSONSchema, fixture_path: &str, fixture: &JsonValue) {
    if let Err(errors) = schema.validate(fixture) {
        let details = errors.map(|err| err.to_string()).collect::<Vec<_>>().join("; ");
        panic!("fixture {fixture_path} did not validate: {details}");
    }
}

fn assert_schema_invalid(schema: &JSONSchema, fixture_path: &str, fixture: &JsonValue) {
    if schema.validate(fixture).is_ok() {
        panic!("fixture {fixture_path} was expected to fail schema validation");
    }
}

struct SchemaSet {
    config: JSONSchema,
    command: JSONSchema,
    event: JSONSchema,
    error: JSONSchema,
    topic: JSONSchema,
    telemetry: JSONSchema,
    attachment: JSONSchema,
    marker: JSONSchema,
    identity: JSONSchema,
    paper: JSONSchema,
    command_plugin: JSONSchema,
    voice_signaling: JSONSchema,
}

struct RpcCoreSchemaSet {
    sdk_negotiate_v2: JSONSchema,
    sdk_send_v2: JSONSchema,
    sdk_send_batch_v2: JSONSchema,
    sdk_status_v2: JSONSchema,
    sdk_configure_v2: JSONSchema,
    sdk_poll_events_v2: JSONSchema,
    sdk_cancel_message_v2: JSONSchema,
    sdk_snapshot_v2: JSONSchema,
    sdk_shutdown_v2: JSONSchema,
}

struct OpenRpcCoreSchemaSet {
    sdk_negotiate_v2: JSONSchema,
    sdk_send_v2: JSONSchema,
    sdk_send_batch_v2: JSONSchema,
    sdk_status_v2: JSONSchema,
    sdk_configure_v2: JSONSchema,
    sdk_poll_events_v2: JSONSchema,
    sdk_cancel_message_v2: JSONSchema,
    sdk_snapshot_v2: JSONSchema,
    sdk_shutdown_v2: JSONSchema,
}

struct RpcDomainSchemaSet {
    release_b_methods: JSONSchema,
    release_c_methods: JSONSchema,
}

struct OpenRpcDomainSchemaSet {
    release_b_methods: JSONSchema,
    release_c_methods: JSONSchema,
}

fn load_schemas() -> SchemaSet {
    let root = workspace_root();
    let schema_dir = root.join("docs/schemas/sdk/v2");
    let config_schema = read_json(&schema_dir.join("config.schema.json"));
    let command_schema = read_json(&schema_dir.join("command.schema.json"));
    let event_schema = read_json(&schema_dir.join("event.schema.json"));
    let error_schema = read_json(&schema_dir.join("error.schema.json"));
    let topic_schema = read_json(&schema_dir.join("topic.schema.json"));
    let telemetry_schema = read_json(&schema_dir.join("telemetry.schema.json"));
    let attachment_schema = read_json(&schema_dir.join("attachment.schema.json"));
    let marker_schema = read_json(&schema_dir.join("marker.schema.json"));
    let identity_schema = read_json(&schema_dir.join("identity.schema.json"));
    let paper_schema = read_json(&schema_dir.join("paper.schema.json"));
    let command_plugin_schema = read_json(&schema_dir.join("command-plugin.schema.json"));
    let voice_signaling_schema = read_json(&schema_dir.join("voice-signaling.schema.json"));
    let command_schema = command_schema_with_embedded_config(&config_schema, command_schema);

    SchemaSet {
        config: compile_schema(&config_schema, "config"),
        command: compile_schema(&command_schema, "command"),
        event: compile_schema(&event_schema, "event"),
        error: compile_schema(&error_schema, "error"),
        topic: compile_schema(&topic_schema, "topic"),
        telemetry: compile_schema(&telemetry_schema, "telemetry"),
        attachment: compile_schema(&attachment_schema, "attachment"),
        marker: compile_schema(&marker_schema, "marker"),
        identity: compile_schema(&identity_schema, "identity"),
        paper: compile_schema(&paper_schema, "paper"),
        command_plugin: compile_schema(&command_plugin_schema, "command-plugin"),
        voice_signaling: compile_schema(&voice_signaling_schema, "voice-signaling"),
    }
}

fn load_rpc_core_schemas() -> RpcCoreSchemaSet {
    let root = workspace_root();
    let schema_dir = root.join("docs/schemas/sdk/v2/rpc");

    let sdk_negotiate_v2 = read_json(&schema_dir.join("sdk_negotiate_v2.schema.json"));
    let sdk_send_v2 = read_json(&schema_dir.join("sdk_send_v2.schema.json"));
    let sdk_send_batch_v2 = read_json(&schema_dir.join("sdk_send_batch_v2.schema.json"));
    let sdk_status_v2 = read_json(&schema_dir.join("sdk_status_v2.schema.json"));
    let sdk_configure_v2 = read_json(&schema_dir.join("sdk_configure_v2.schema.json"));
    let sdk_poll_events_v2 = read_json(&schema_dir.join("sdk_poll_events_v2.schema.json"));
    let sdk_cancel_message_v2 = read_json(&schema_dir.join("sdk_cancel_message_v2.schema.json"));
    let sdk_snapshot_v2 = read_json(&schema_dir.join("sdk_snapshot_v2.schema.json"));
    let sdk_shutdown_v2 = read_json(&schema_dir.join("sdk_shutdown_v2.schema.json"));

    RpcCoreSchemaSet {
        sdk_negotiate_v2: compile_schema(&sdk_negotiate_v2, "rpc/sdk_negotiate_v2"),
        sdk_send_v2: compile_schema(&sdk_send_v2, "rpc/sdk_send_v2"),
        sdk_send_batch_v2: compile_schema(&sdk_send_batch_v2, "rpc/sdk_send_batch_v2"),
        sdk_status_v2: compile_schema(&sdk_status_v2, "rpc/sdk_status_v2"),
        sdk_configure_v2: compile_schema(&sdk_configure_v2, "rpc/sdk_configure_v2"),
        sdk_poll_events_v2: compile_schema(&sdk_poll_events_v2, "rpc/sdk_poll_events_v2"),
        sdk_cancel_message_v2: compile_schema(&sdk_cancel_message_v2, "rpc/sdk_cancel_message_v2"),
        sdk_snapshot_v2: compile_schema(&sdk_snapshot_v2, "rpc/sdk_snapshot_v2"),
        sdk_shutdown_v2: compile_schema(&sdk_shutdown_v2, "rpc/sdk_shutdown_v2"),
    }
}

fn load_rpc_domain_schemas() -> RpcDomainSchemaSet {
    let root = workspace_root();
    let schema_dir = root.join("docs/schemas/sdk/v2/rpc");

    let release_b_methods = read_json(&schema_dir.join("sdk_release_b_methods.schema.json"));
    let release_c_methods = read_json(&schema_dir.join("sdk_release_c_methods.schema.json"));

    RpcDomainSchemaSet {
        release_b_methods: compile_schema(&release_b_methods, "rpc/sdk_release_b_methods"),
        release_c_methods: compile_schema(&release_c_methods, "rpc/sdk_release_c_methods"),
    }
}

fn rewrite_ref_prefix(value: &mut JsonValue, from: &str, to: &str) {
    match value {
        JsonValue::Object(object) => {
            if let Some(JsonValue::String(current)) = object.get_mut("$ref") {
                if current.starts_with(from) {
                    *current = format!("{to}{}", &current[from.len()..]);
                }
            }
            for nested in object.values_mut() {
                rewrite_ref_prefix(nested, from, to);
            }
        }
        JsonValue::Array(items) => {
            for item in items {
                rewrite_ref_prefix(item, from, to);
            }
        }
        JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) => {}
    }
}

fn openrpc_path() -> PathBuf {
    workspace_root().join("docs/openrpc/sdk-v2.openrpc.json")
}

fn load_openrpc_document() -> JsonValue {
    read_json(&openrpc_path())
}

fn openrpc_component_schema(document: &JsonValue, component_name: &str) -> JsonValue {
    let schemas = document
        .get("components")
        .and_then(|item| item.get("schemas"))
        .and_then(JsonValue::as_object)
        .expect("openrpc components.schemas object");
    let component = schemas
        .get(component_name)
        .unwrap_or_else(|| panic!("missing OpenRPC component schema {component_name}"));

    let mut defs = JsonMap::new();
    for (name, schema) in schemas {
        let mut rewritten = schema.clone();
        rewrite_ref_prefix(&mut rewritten, "#/components/schemas/", "#/$defs/");
        defs.insert(name.clone(), rewritten);
    }

    let mut root = component.clone();
    rewrite_ref_prefix(&mut root, "#/components/schemas/", "#/$defs/");
    let root_object = root.as_object_mut().expect("OpenRPC component schema root must be object");
    root_object.insert(
        "$schema".to_string(),
        JsonValue::String("https://json-schema.org/draft/2020-12/schema".to_string()),
    );
    root_object.insert(
        "$id".to_string(),
        JsonValue::String(format!(
            "https://weft.tak/contracts/sdk/v2/openrpc/components/{component_name}.schema.json"
        )),
    );
    root_object.insert("$defs".to_string(), JsonValue::Object(defs));
    JsonValue::Object(root_object.clone())
}

fn load_openrpc_core_schemas() -> OpenRpcCoreSchemaSet {
    let document = load_openrpc_document();

    OpenRpcCoreSchemaSet {
        sdk_negotiate_v2: compile_schema(
            &openrpc_component_schema(&document, "SdkNegotiateV2Envelope"),
            "openrpc/SdkNegotiateV2Envelope",
        ),
        sdk_send_v2: compile_schema(
            &openrpc_component_schema(&document, "SdkSendV2Envelope"),
            "openrpc/SdkSendV2Envelope",
        ),
        sdk_send_batch_v2: compile_schema(
            &openrpc_component_schema(&document, "SdkSendBatchV2Envelope"),
            "openrpc/SdkSendBatchV2Envelope",
        ),
        sdk_status_v2: compile_schema(
            &openrpc_component_schema(&document, "SdkStatusV2Envelope"),
            "openrpc/SdkStatusV2Envelope",
        ),
        sdk_configure_v2: compile_schema(
            &openrpc_component_schema(&document, "SdkConfigureV2Envelope"),
            "openrpc/SdkConfigureV2Envelope",
        ),
        sdk_poll_events_v2: compile_schema(
            &openrpc_component_schema(&document, "SdkPollEventsV2Envelope"),
            "openrpc/SdkPollEventsV2Envelope",
        ),
        sdk_cancel_message_v2: compile_schema(
            &openrpc_component_schema(&document, "SdkCancelMessageV2Envelope"),
            "openrpc/SdkCancelMessageV2Envelope",
        ),
        sdk_snapshot_v2: compile_schema(
            &openrpc_component_schema(&document, "SdkSnapshotV2Envelope"),
            "openrpc/SdkSnapshotV2Envelope",
        ),
        sdk_shutdown_v2: compile_schema(
            &openrpc_component_schema(&document, "SdkShutdownV2Envelope"),
            "openrpc/SdkShutdownV2Envelope",
        ),
    }
}

fn load_openrpc_domain_schemas() -> OpenRpcDomainSchemaSet {
    let document = load_openrpc_document();

    OpenRpcDomainSchemaSet {
        release_b_methods: compile_schema(
            &openrpc_component_schema(&document, "SdkReleaseBEnvelope"),
            "openrpc/SdkReleaseBEnvelope",
        ),
        release_c_methods: compile_schema(
            &openrpc_component_schema(&document, "SdkReleaseCEnvelope"),
            "openrpc/SdkReleaseCEnvelope",
        ),
    }
}

fn assert_openrpc_schema_ref_exists(document: &JsonValue, schema: &JsonValue) {
    match schema {
        JsonValue::Object(object) => {
            if let Some(JsonValue::String(reference)) = object.get("$ref") {
                let prefix = "#/components/schemas/";
                if let Some(name) = reference.strip_prefix(prefix) {
                    let exists = document
                        .get("components")
                        .and_then(|item| item.get("schemas"))
                        .and_then(|item| item.get(name))
                        .is_some();
                    assert!(exists, "missing OpenRPC schema ref target {reference}");
                } else {
                    panic!("unsupported OpenRPC schema ref {reference}");
                }
            }
            for nested in object.values() {
                assert_openrpc_schema_ref_exists(document, nested);
            }
        }
        JsonValue::Array(items) => {
            for item in items {
                assert_openrpc_schema_ref_exists(document, item);
            }
        }
        JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) => {}
    }
}

fn validate_openrpc_document(document: &JsonValue) {
    let object = document.as_object().expect("OpenRPC document root object");
    assert_eq!(
        object.get("openrpc").and_then(JsonValue::as_str).expect("OpenRPC version"),
        "1.3.2"
    );
    assert!(
        object.get("info").and_then(|item| item.get("title")).and_then(JsonValue::as_str).is_some(),
        "OpenRPC info.title missing"
    );
    assert!(
        object
            .get("info")
            .and_then(|item| item.get("version"))
            .and_then(JsonValue::as_str)
            .is_some(),
        "OpenRPC info.version missing"
    );

    let methods =
        object.get("methods").and_then(JsonValue::as_array).expect("OpenRPC methods array");
    let expected_core_methods = [
        "sdk_negotiate_v2",
        "sdk_send_v2",
        "sdk_send_batch_v2",
        "sdk_status_v2",
        "sdk_configure_v2",
        "sdk_poll_events_v2",
        "sdk_cancel_message_v2",
        "sdk_snapshot_v2",
        "sdk_shutdown_v2",
    ];
    let schemas = object
        .get("components")
        .and_then(|item| item.get("schemas"))
        .and_then(JsonValue::as_object)
        .expect("OpenRPC components.schemas object");
    assert!(
        schemas.contains_key("RpcId") && schemas.contains_key("RpcError"),
        "OpenRPC common component schemas missing"
    );

    for method_name in expected_core_methods {
        let method = methods
            .iter()
            .find(|item| item.get("name").and_then(JsonValue::as_str) == Some(method_name))
            .unwrap_or_else(|| panic!("OpenRPC method missing: {method_name}"));
        let params = method
            .get("params")
            .and_then(JsonValue::as_array)
            .unwrap_or_else(|| panic!("OpenRPC method params missing: {method_name}"));
        assert_eq!(params.len(), 1, "OpenRPC method params drift: {method_name}");
        assert_openrpc_schema_ref_exists(
            document,
            params[0]
                .get("schema")
                .unwrap_or_else(|| panic!("OpenRPC param schema missing: {method_name}")),
        );
        assert_openrpc_schema_ref_exists(
            document,
            method
                .get("result")
                .and_then(|item| item.get("schema"))
                .unwrap_or_else(|| panic!("OpenRPC result schema missing: {method_name}")),
        );
    }

    for name in [
        "SdkNegotiateV2Envelope",
        "SdkSendV2Envelope",
        "SdkSendBatchV2Envelope",
        "SdkStatusV2Envelope",
        "SdkConfigureV2Envelope",
        "SdkPollEventsV2Envelope",
        "SdkCancelMessageV2Envelope",
        "SdkSnapshotV2Envelope",
        "SdkShutdownV2Envelope",
    ] {
        let compiled = openrpc_component_schema(document, name);
        compile_schema(&compiled, &format!("openrpc/{name}"));
    }

    if schemas.contains_key("SdkReleaseBEnvelope") {
        let compiled = openrpc_component_schema(document, "SdkReleaseBEnvelope");
        compile_schema(&compiled, "openrpc/SdkReleaseBEnvelope");
        for method_name in [
            "sdk_topic_create_v2",
            "sdk_topic_get_v2",
            "sdk_topic_list_v2",
            "sdk_topic_subscribe_v2",
            "sdk_topic_unsubscribe_v2",
            "sdk_topic_publish_v2",
            "sdk_telemetry_query_v2",
            "sdk_telemetry_subscribe_v2",
            "sdk_attachment_store_v2",
            "sdk_attachment_upload_start_v2",
            "sdk_attachment_upload_chunk_v2",
            "sdk_attachment_upload_commit_v2",
            "sdk_attachment_get_v2",
            "sdk_attachment_list_v2",
            "sdk_attachment_delete_v2",
            "sdk_attachment_download_v2",
            "sdk_attachment_download_chunk_v2",
            "sdk_attachment_associate_topic_v2",
            "sdk_marker_create_v2",
            "sdk_marker_list_v2",
            "sdk_marker_update_position_v2",
            "sdk_marker_delete_v2",
        ] {
            let method = methods
                .iter()
                .find(|item| item.get("name").and_then(JsonValue::as_str) == Some(method_name))
                .unwrap_or_else(|| panic!("OpenRPC method missing: {method_name}"));
            let params = method
                .get("params")
                .and_then(JsonValue::as_array)
                .unwrap_or_else(|| panic!("OpenRPC method params missing: {method_name}"));
            assert_eq!(params.len(), 1, "OpenRPC method params drift: {method_name}");
            assert_openrpc_schema_ref_exists(
                document,
                method
                    .get("result")
                    .and_then(|item| item.get("schema"))
                    .unwrap_or_else(|| panic!("OpenRPC result schema missing: {method_name}")),
            );
        }
    }

    if schemas.contains_key("SdkReleaseCEnvelope") {
        let compiled = openrpc_component_schema(document, "SdkReleaseCEnvelope");
        compile_schema(&compiled, "openrpc/SdkReleaseCEnvelope");
        for method_name in [
            "sdk_identity_list_v2",
            "sdk_identity_announce_now_v2",
            "sdk_identity_presence_list_v2",
            "sdk_identity_activate_v2",
            "sdk_identity_import_v2",
            "sdk_identity_export_v2",
            "sdk_identity_resolve_v2",
            "sdk_identity_contact_update_v2",
            "sdk_identity_contact_list_v2",
            "sdk_identity_bootstrap_v2",
            "sdk_paper_encode_v2",
            "sdk_paper_decode_v2",
            "sdk_command_invoke_v2",
            "sdk_command_reply_v2",
            "sdk_voice_session_open_v2",
            "sdk_voice_session_update_v2",
            "sdk_voice_session_close_v2",
        ] {
            let method = methods
                .iter()
                .find(|item| item.get("name").and_then(JsonValue::as_str) == Some(method_name))
                .unwrap_or_else(|| panic!("OpenRPC method missing: {method_name}"));
            let params = method
                .get("params")
                .and_then(JsonValue::as_array)
                .unwrap_or_else(|| panic!("OpenRPC method params missing: {method_name}"));
            assert_eq!(params.len(), 1, "OpenRPC method params drift: {method_name}");
        }
    }
}

fn fixture(path: &str) -> JsonValue {
    let root = workspace_root();
    read_json(&root.join(path))
}

fn fixture_paths(dir: &str) -> Vec<PathBuf> {
    let root = workspace_root().join(dir);
    let mut paths = Vec::new();
    collect_json_files(&root, &mut paths);
    paths.sort();
    paths
}

#[test]
fn sdk_schema_documents_parse_and_compile() {
    let _schemas = load_schemas();
    let _rpc_schemas = load_rpc_core_schemas();
    let _openrpc_schemas = load_openrpc_core_schemas();
    let _rpc_domain_schemas = load_rpc_domain_schemas();
    let _openrpc_domain_schemas = load_openrpc_domain_schemas();
    let openrpc = load_openrpc_document();
    validate_openrpc_document(&openrpc);

    let root = workspace_root();
    let schema_root = root.join("docs/schemas/sdk/v2");
    let mut schema_paths = Vec::new();
    collect_json_files(&schema_root, &mut schema_paths);
    let mut schema_files = 0_usize;
    for path in schema_paths {
        let schema = read_json(&path);
        let object = schema.as_object().expect("schema root object");
        assert!(object.contains_key("$schema"), "{} missing $schema", path.display());
        assert!(object.contains_key("$id"), "{} missing $id", path.display());
        assert!(object.contains_key("title"), "{} missing title", path.display());
        compile_schema(&schema, path.to_string_lossy().as_ref());
        schema_files += 1;
    }
    assert!(schema_files >= 12, "expected at least 12 sdk schema files");
}
