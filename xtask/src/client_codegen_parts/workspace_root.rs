#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::fs;

    fn workspace_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask crate should live under workspace root")
            .to_path_buf()
    }

    #[test]
    fn parse_direct_schema_methods() {
        let schema = json!({
            "$defs": {
                "request": {
                    "properties": {
                        "method": {"const": "sdk_send_v2"},
                        "params": {
                            "type": "object",
                            "properties": {"source": {"type": "string"}},
                            "required": ["source"],
                        },
                    },
                },
                "response_ok": {
                    "properties": {
                        "result": {
                            "type": "object",
                            "properties": {"message_id": {"type": "string"}},
                            "required": ["message_id"],
                        },
                    },
                },
            },
        });

        let methods = extract_methods_from_schema(
            &schema,
            Path::new("docs/schemas/sdk/v2/rpc/sdk_send_v2.schema.json"),
        )
        .expect("direct schema parse");

        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].method, "sdk_send_v2");
    }

    #[test]
    fn parse_grouped_schema_methods() {
        let schema = json!({
            "$defs": {
                "request": {
                    "properties": {
                        "method": {"enum": ["sdk_topic_list_v2", "sdk_topic_create_v2"]},
                        "params": {
                            "type": "object",
                            "properties": {"cursor": {"type": ["string", "null"]}},
                        },
                    },
                    "allOf": [
                        {
                            "if": {
                                "properties": {"method": {"const": "sdk_topic_create_v2"}},
                                "required": ["method"],
                            },
                            "then": {
                                "properties": {
                                    "params": {
                                        "type": "object",
                                        "properties": {"topic_path": {"type": "string"}},
                                    },
                                },
                            },
                        },
                    ],
                },
            },
        });

        let methods = extract_methods_from_schema(
            &schema,
            Path::new("docs/schemas/sdk/v2/rpc/sdk_release_b_methods.schema.json"),
        )
        .expect("grouped schema parse");

        let names = methods.iter().map(|m| m.method.as_str()).collect::<Vec<_>>();
        assert!(names.contains(&"sdk_topic_list_v2"));
        assert!(names.contains(&"sdk_topic_create_v2"));
        assert_eq!(methods.len(), 2);
    }

    #[test]
    fn reject_duplicate_methods_across_sources() {
        let schema = json!({
            "$defs": {
                "request": {
                    "properties": {
                        "method": {"const":"sdk_send_v2"},
                        "params": {"type": "object", "properties": {"source": {"type": "string"}}},
                    },
                },
                "response_ok": {"properties": {}},
            },
        });

        let source_a = SchemaSource {
            path: Path::new("docs/schemas/sdk/v2/rpc/sdk_send_v2.schema.json").to_path_buf(),
            schema: schema.clone(),
            def_component_prefix: "SdkSendV2".to_string(),
            kind: SchemaSourceKind::JsonSchemaDefs,
        };
        let source_b = SchemaSource {
            path: Path::new("docs/schemas/sdk/v2/rpc/sdk_send_v2_duplicate.schema.json")
                .to_path_buf(),
            schema,
            def_component_prefix: "SdkSendV2".to_string(),
            kind: SchemaSourceKind::JsonSchemaDefs,
        };

        let err = discover_methods(&[source_a, source_b]).unwrap_err();
        assert!(err.to_string().contains("duplicate RPC method 'sdk_send_v2'"));
    }

    #[test]
    fn recursive_ref_keeps_named_component() {
        let source = SchemaSource {
            path: Path::new("docs/schemas/sdk/v2/error.schema.json").to_path_buf(),
            schema: json!({
                "$defs": {
                    "json_value": {
                        "oneOf": [
                            {"type": "string"},
                            {
                                "type": "array",
                                "items": {"$ref": "#/$defs/json_value"},
                            },
                        ],
                    },
                },
            }),
            def_component_prefix: "Error".to_string(),
            kind: SchemaSourceKind::JsonSchemaDefs,
        };

        let mut components = BTreeMap::new();
        let mut in_progress = BTreeSet::new();

        let normalized = normalize_schema_with_refs(
            &source.schema["$defs"]["json_value"],
            &source,
            &mut components,
            &mut in_progress,
        )
        .expect("recursive refs should normalize");

        let rendered = serde_json::to_string(&normalized).expect("serialize");
        assert!(
            rendered.contains("#/components/schemas/ErrorJsonValue"),
            "recursive refs should reference named component"
        );
    }

    #[test]
    fn integer_validation_rejects_non_integer_numbers() {
        let err = validate_json_value("n", &json!(1.5), &json!({"type": "integer"})).unwrap_err();
        assert!(err.to_string().contains("integer"));
    }

    #[test]
    fn convert_openapi_spec_to_generator_compatible() {
        let input = json!({
            "openapi": "3.1.0",
            "paths": {},
            "components": {
                "schemas": {
                    "Request": {
                        "type": "object",
                        "properties": {
                            "method": {"const": "sdk_send_v2"},
                            "count": {"type": ["integer", "null"]},
                            "body": {"type": ["object", "string", "null"]},
                            "empty": {"type": "null"},
                            "json": {
                                "oneOf": [
                                    {"type": "string"},
                                    {"type": "null"}
                                ]
                            },
                        },
                        "required": ["method"]
                    },
                    "Payload": {
                        "$id": "urn:example",
                        "$schema": "https://json-schema.org/draft/2020-12/schema",
                        "type": ["string", "null"]
                    },
                    "ErrorJsonValue": {
                        "oneOf": [
                            {"type": "string"},
                            {"type": "number"},
                            {"type": "null"}
                        ]
                    },
                    "rpcId": {
                        "oneOf": [
                            {"type": "string"},
                            {"type": "integer", "minimum": 0}
                        ]
                    },
                    "ResponseMetaRpcEndpoint": {
                        "type": ["object", "string", "null"]
                    },
                    "MapWithPropertyNames": {
                        "type": "object",
                        "propertyNames": {
                            "type": "string",
                            "minLength": 1
                        },
                        "additionalProperties": {
                            "type": "string"
                        }
                    }
                }
            },
            "$schema": "https://json-schema.org/draft/2020-12/schema"
        });

        let converted = convert_openapi_spec_for_generator(&input).expect("convert openapi");
        assert_eq!(converted["openapi"], "3.0.3");
        assert!(converted.get("$schema").is_none());

        let request = &converted["components"]["schemas"]["Request"];
        assert_eq!(request["type"], "object");
        assert!(request.get("additionalProperties").is_none());
        assert_eq!(request["properties"]["method"]["enum"][0], "sdk_send_v2");

        let count = &request["properties"]["count"];
        assert_eq!(count["type"], "integer");
        assert_eq!(count["nullable"], true);
        assert!(count.get("additionalProperties").is_none());

        let body = &request["properties"]["body"];
        assert_eq!(body["anyOf"][0]["type"], "object");
        assert_eq!(body["anyOf"][1]["type"], "string");
        assert_eq!(body["nullable"], true);
        assert!(body.get("type").is_none());

        let empty = &request["properties"]["empty"];
        assert_eq!(empty["nullable"], true);
        assert!(empty.get("type").is_none());

        let json = &request["properties"]["json"];
        assert_eq!(json["oneOf"][0]["type"], "string");
        assert_eq!(json["oneOf"].as_array().expect("oneOf array").len(), 1);
        assert_eq!(json["nullable"], true);

        let payload = &converted["components"]["schemas"]["Payload"];
        assert!(payload.get("const").is_none());
        assert_eq!(payload["type"], "string");
        assert_eq!(payload["nullable"], true);

        let error_json_value = &converted["components"]["schemas"]["ErrorJsonValue"];
        assert_eq!(error_json_value["type"], "object");
        assert_eq!(error_json_value["additionalProperties"], true);
        assert!(error_json_value.get("oneOf").is_none());

        let rpc_id = &converted["components"]["schemas"]["rpcId"];
        assert_eq!(rpc_id["type"], "string");
        assert!(rpc_id.get("oneOf").is_none());

        let rpc_endpoint = &converted["components"]["schemas"]["ResponseMetaRpcEndpoint"];
        assert_eq!(rpc_endpoint["type"], "object");
        assert_eq!(rpc_endpoint["additionalProperties"], true);
        assert!(rpc_endpoint.get("anyOf").is_none());

        let map_schema = &converted["components"]["schemas"]["MapWithPropertyNames"];
        assert!(map_schema.get("propertyNames").is_none());
        assert_eq!(map_schema["type"], "object");
        assert_eq!(map_schema["additionalProperties"]["type"], "string");
    }

    #[test]
    fn conversion_keeps_unspecified_additional_properties() {
        let input = json!({
            "openapi": "3.1.0",
            "components": {
                "schemas": {
                    "Request": {
                        "type": "object",
                        "properties": {"id": {"type": "string"}}
                    }
                }
            }
        });

        let converted = convert_openapi_spec_for_generator(&input).expect("convert openapi");
        assert_eq!(
            converted["components"]["schemas"]["Request"].get("additionalProperties"),
            None,
            "unspecified additionalProperties should remain unspecified"
        );
    }

    #[test]
    fn discover_schema_glob_pattern_matches_nested_files() -> Result<()> {
        let temp = TempDir::new(Path::new("."))?;
        let rpc_root = temp.path.join("docs/schemas/sdk/v2/rpc");
        fs::create_dir_all(rpc_root.join("nested"))?;

        let root_schema = rpc_root.join("sdk_release_a_methods.schema.json");
        let nested_schema = rpc_root.join("nested").join("sdk_release_b_methods.schema.json");
        fs::write(&root_schema, "{}")?;
        fs::write(&nested_schema, "{}")?;

        let matches = discover_schema_glob(&temp.path, "**/*methods.schema.json")?;
        let mut paths = BTreeSet::new();
        for path in matches {
            paths.insert(
                path.file_name().and_then(|name| name.to_str()).unwrap_or_default().to_string(),
            );
        }

        assert!(paths.contains("sdk_release_a_methods.schema.json"));
        assert!(paths.contains("sdk_release_b_methods.schema.json"));
        Ok(())
    }

    #[test]
    fn projected_legacy_rpc_schemas_match_committed_files() -> Result<()> {
        let workspace = workspace_root();
        let openrpc_path = workspace.join("docs/openrpc/sdk-v2.openrpc.json");
        let openrpc = serde_json::from_str::<Value>(&fs::read_to_string(openrpc_path)?)?;
        let projected = project_legacy_rpc_schemas(&openrpc)?;

        assert_eq!(projected.len(), 11, "expected 9 core + release B + release C projections");

        for artifact in projected {
            let committed_path = workspace.join(&artifact.path);
            let committed = canonicalize_json(serde_json::from_str::<Value>(&fs::read_to_string(
                &committed_path,
            )?)?);
            assert_eq!(
                canonicalize_json(artifact.schema),
                committed,
                "projected legacy schema mismatch for {}",
                committed_path.display()
            );
        }

        Ok(())
    }

    #[test]
    fn projected_legacy_rpc_schemas_cover_each_method_once() -> Result<()> {
        let workspace = workspace_root();
        let openrpc = serde_json::from_str::<Value>(&fs::read_to_string(
            workspace.join("docs/openrpc/sdk-v2.openrpc.json"),
        )?)?;
        let methods =
            openrpc.get("methods").and_then(Value::as_array).context("OpenRPC methods missing")?;
        let components = openrpc
            .get("components")
            .and_then(|item| item.get("schemas"))
            .and_then(Value::as_object)
            .context("OpenRPC components missing")?;

        let release_b_methods =
            grouped_method_set(components, "SdkReleaseBRequestEnvelope", "release B request")?;
        let release_c_methods =
            grouped_method_set(components, "SdkReleaseCRequestEnvelope", "release C request")?;
        let core_methods = methods
            .iter()
            .filter_map(|method| {
                let method_name = method.get("name").and_then(Value::as_str)?;
                if release_b_methods.contains(method_name)
                    || release_c_methods.contains(method_name)
                {
                    None
                } else {
                    Some(method_name.to_string())
                }
            })
            .collect::<BTreeSet<_>>();

        assert!(release_b_methods.is_disjoint(&release_c_methods));

        let mut assigned = BTreeSet::new();
        assigned.extend(core_methods);
        assigned.extend(release_b_methods);
        assigned.extend(release_c_methods);

        let discovered = methods
            .iter()
            .filter_map(|method| method.get("name").and_then(Value::as_str).map(str::to_string))
            .collect::<BTreeSet<_>>();

        assert_eq!(assigned, discovered);
        Ok(())
    }

    #[test]
    fn release_c_projection_keeps_request_and_error_only() -> Result<()> {
        let workspace = workspace_root();
        let openrpc = serde_json::from_str::<Value>(&fs::read_to_string(
            workspace.join("docs/openrpc/sdk-v2.openrpc.json"),
        )?)?;
        let projected = project_legacy_rpc_schemas(&openrpc)?;
        let release_c = projected
            .into_iter()
            .find(|artifact| artifact.path.ends_with("sdk_release_c_methods.schema.json"))
            .context("missing release C projection")?;

        let one_of = release_c
            .schema
            .get("oneOf")
            .and_then(Value::as_array)
            .context("release C projection missing oneOf")?;
        let refs = one_of
            .iter()
            .filter_map(|item| item.get("$ref").and_then(Value::as_str))
            .collect::<Vec<_>>();

        assert_eq!(refs, vec!["#/$defs/request", "#/$defs/response_error"]);
        Ok(())
    }

    #[test]
    fn sync_legacy_rpc_schemas_check_mode_detects_drift() -> Result<()> {
        let temp = TempDir::new(Path::new("."))?;
        let workspace = workspace_root();
        let openrpc_path = temp.path.join("docs/openrpc/sdk-v2.openrpc.json");
        let openrpc_parent = openrpc_path.parent().context("missing temp openrpc parent")?;
        fs::create_dir_all(openrpc_parent)?;
        fs::write(
            &openrpc_path,
            fs::read_to_string(workspace.join("docs/openrpc/sdk-v2.openrpc.json"))?,
        )?;

        let openrpc = serde_json::from_str::<Value>(&fs::read_to_string(&openrpc_path)?)?;
        for artifact in project_legacy_rpc_schemas(&openrpc)? {
            let destination = temp.path.join(&artifact.path);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            let encoded = serde_json::to_string_pretty(&canonicalize_json(artifact.schema))?;
            fs::write(destination, format!("{encoded}\n"))?;
        }

        let drift_path = temp.path.join("docs/schemas/sdk/v2/rpc/sdk_send_v2.schema.json");
        fs::write(&drift_path, "{\n  \"drift\": true\n}\n")?;

        let schema_sources = vec![SchemaSource {
            path: openrpc_path,
            schema: openrpc,
            def_component_prefix: String::new(),
            kind: SchemaSourceKind::OpenRpc,
        }];

        let err = sync_legacy_rpc_schemas(&temp.path, &schema_sources, SchemaClientMode::Check)
            .unwrap_err();
        assert!(err.to_string().contains("out of date"));
        Ok(())
    }
}
