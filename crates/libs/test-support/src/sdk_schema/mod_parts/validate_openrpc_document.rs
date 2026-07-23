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
            "sdk_identity_create_v2",
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
