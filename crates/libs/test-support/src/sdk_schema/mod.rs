use jsonschema::{Draft, JSONSchema};
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::fs;
use std::path::{Path, PathBuf};

mod cookbook_tests;
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
    let sdk_status_v2 = read_json(&schema_dir.join("sdk_status_v2.schema.json"));
    let sdk_configure_v2 = read_json(&schema_dir.join("sdk_configure_v2.schema.json"));
    let sdk_poll_events_v2 = read_json(&schema_dir.join("sdk_poll_events_v2.schema.json"));
    let sdk_cancel_message_v2 = read_json(&schema_dir.join("sdk_cancel_message_v2.schema.json"));
    let sdk_snapshot_v2 = read_json(&schema_dir.join("sdk_snapshot_v2.schema.json"));
    let sdk_shutdown_v2 = read_json(&schema_dir.join("sdk_shutdown_v2.schema.json"));

    RpcCoreSchemaSet {
        sdk_negotiate_v2: compile_schema(&sdk_negotiate_v2, "rpc/sdk_negotiate_v2"),
        sdk_send_v2: compile_schema(&sdk_send_v2, "rpc/sdk_send_v2"),
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
    let _rpc_domain_schemas = load_rpc_domain_schemas();

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
