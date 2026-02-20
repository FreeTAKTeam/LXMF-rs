use jsonschema::{Draft, JSONSchema};
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::fs;
use std::path::{Path, PathBuf};

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
    }
}

fn fixture(path: &str) -> JsonValue {
    let root = workspace_root();
    read_json(&root.join(path))
}

#[test]
fn sdk_schema_documents_parse_and_compile() {
    let _schemas = load_schemas();

    let root = workspace_root();
    let schema_root = root.join("docs/schemas/sdk/v2");
    let entries = fs::read_dir(&schema_root).expect("read schema directory");
    let mut schema_files = 0_usize;
    for entry in entries {
        let path = entry.expect("schema directory entry").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let schema = read_json(&path);
        let object = schema.as_object().expect("schema root object");
        assert!(object.contains_key("$schema"), "{} missing $schema", path.display());
        assert!(object.contains_key("$id"), "{} missing $id", path.display());
        assert!(object.contains_key("title"), "{} missing title", path.display());
        schema_files += 1;
    }
    assert!(schema_files >= 4, "expected at least 4 sdk schema files");
}

#[test]
fn sdk_schema_valid_fixtures_pass_contract_checks() {
    let schemas = load_schemas();
    assert_schema_valid(
        &schemas.event,
        "docs/fixtures/sdk-v2/event.runtime_state.valid.json",
        &fixture("docs/fixtures/sdk-v2/event.runtime_state.valid.json"),
    );
    assert_schema_valid(
        &schemas.event,
        "docs/fixtures/sdk-v2/event.stream_gap.valid.json",
        &fixture("docs/fixtures/sdk-v2/event.stream_gap.valid.json"),
    );
    assert_schema_valid(
        &schemas.config,
        "docs/fixtures/sdk-v2/config.desktop_local.valid.json",
        &fixture("docs/fixtures/sdk-v2/config.desktop_local.valid.json"),
    );
    assert_schema_valid(
        &schemas.config,
        "docs/fixtures/sdk-v2/config.remote_token.valid.json",
        &fixture("docs/fixtures/sdk-v2/config.remote_token.valid.json"),
    );
    assert_schema_valid(
        &schemas.command,
        "docs/fixtures/sdk-v2/command.start.valid.json",
        &fixture("docs/fixtures/sdk-v2/command.start.valid.json"),
    );
    assert_schema_valid(
        &schemas.command,
        "docs/fixtures/sdk-v2/command.topic_get.valid.json",
        &fixture("docs/fixtures/sdk-v2/command.topic_get.valid.json"),
    );
    assert_schema_valid(
        &schemas.command,
        "docs/fixtures/sdk-v2/command.attachment_store.valid.json",
        &fixture("docs/fixtures/sdk-v2/command.attachment_store.valid.json"),
    );
    assert_schema_valid(
        &schemas.command,
        "docs/fixtures/sdk-v2/command.attachment_associate_topic.valid.json",
        &fixture("docs/fixtures/sdk-v2/command.attachment_associate_topic.valid.json"),
    );
    assert_schema_valid(
        &schemas.error,
        "docs/fixtures/sdk-v2/error.validation.valid.json",
        &fixture("docs/fixtures/sdk-v2/error.validation.valid.json"),
    );
    assert_schema_valid(
        &schemas.topic,
        "docs/fixtures/sdk-v2/topic.record.valid.json",
        &fixture("docs/fixtures/sdk-v2/topic.record.valid.json"),
    );
    assert_schema_valid(
        &schemas.telemetry,
        "docs/fixtures/sdk-v2/telemetry.point.valid.json",
        &fixture("docs/fixtures/sdk-v2/telemetry.point.valid.json"),
    );
    assert_schema_valid(
        &schemas.attachment,
        "docs/fixtures/sdk-v2/attachment.meta.valid.json",
        &fixture("docs/fixtures/sdk-v2/attachment.meta.valid.json"),
    );
    assert_schema_valid(
        &schemas.marker,
        "docs/fixtures/sdk-v2/marker.record.valid.json",
        &fixture("docs/fixtures/sdk-v2/marker.record.valid.json"),
    );
}

#[test]
fn sdk_schema_invalid_fixtures_are_rejected() {
    let schemas = load_schemas();
    assert_schema_invalid(
        &schemas.event,
        "docs/fixtures/sdk-v2/event.extension_reserved.invalid.json",
        &fixture("docs/fixtures/sdk-v2/event.extension_reserved.invalid.json"),
    );
    assert_schema_invalid(
        &schemas.config,
        "docs/fixtures/sdk-v2/config.remote_local_trusted.invalid.json",
        &fixture("docs/fixtures/sdk-v2/config.remote_local_trusted.invalid.json"),
    );
    assert_schema_invalid(
        &schemas.command,
        "docs/fixtures/sdk-v2/command.send_unknown_field.invalid.json",
        &fixture("docs/fixtures/sdk-v2/command.send_unknown_field.invalid.json"),
    );
    assert_schema_invalid(
        &schemas.command,
        "docs/fixtures/sdk-v2/command.topic_get.invalid.json",
        &fixture("docs/fixtures/sdk-v2/command.topic_get.invalid.json"),
    );
    assert_schema_invalid(
        &schemas.command,
        "docs/fixtures/sdk-v2/command.attachment_associate_topic.invalid.json",
        &fixture("docs/fixtures/sdk-v2/command.attachment_associate_topic.invalid.json"),
    );
    assert_schema_invalid(
        &schemas.error,
        "docs/fixtures/sdk-v2/error.invalid_machine_code.invalid.json",
        &fixture("docs/fixtures/sdk-v2/error.invalid_machine_code.invalid.json"),
    );
}
