fn generate_openapi_spec(
    manifest: &ClientGenerationManifest,
    schema_sources: &[SchemaSource],
    methods: &[MethodDescriptor],
    spec_path: &Path,
) -> Result<String> {
    let default_backend = GeneratorBackendConfig {
        name: "openapi".to_string(),
        openapi_version: DEFAULT_OPENAPI_VERSION.to_string(),
    };
    let backend = manifest.generator_backend.as_ref().unwrap_or(&default_backend);

    let mut components = BTreeMap::new();
    let rpc_id_schema = resolve_rpc_id_schema(schema_sources);
    components.insert("rpcId".to_string(), rpc_id_schema.clone());

    for source in schema_sources {
        if source.kind == SchemaSourceKind::OpenRpc {
            continue;
        }
        let defs = source
            .schema
            .get("$defs")
            .and_then(Value::as_object)
            .context("schema missing $defs")?;

        for (def_name, def_schema) in defs {
            let component_name = source_to_component_name(&source.def_component_prefix, def_name);
            if components.contains_key(&component_name) {
                continue;
            }
            let normalized = normalize_schema_with_refs(
                def_schema,
                source,
                &mut components,
                &mut BTreeSet::new(),
            )?;
            components.insert(component_name, normalized);
        }
    }

    let rpc_error_source = select_error_schema_source(schema_sources)
        .context("schema missing error.schema.json in required schemas")?;
    let rpc_error_payload = normalize_schema_with_refs(
        &select_error_schema(rpc_error_source),
        rpc_error_source,
        &mut components,
        &mut BTreeSet::new(),
    )?;

    components.insert(
        "RPCRequest".to_string(),
        json!({
            "type": "object",
            "required": ["id", "method", "params"],
            "properties": {
                "jsonrpc": {"type": "string", "const": "2.0"},
                "id": {"$ref": "#/components/schemas/rpcId"},
                "method": {"type": "string"},
                "params": {"type": "object", "additionalProperties": true}
            },
            "additionalProperties": false
        }),
    );
    components.insert(
        "RPCSuccess".to_string(),
        json!({
            "type": "object",
            "required": ["id", "result"],
            "properties": {
                "jsonrpc": {"type": "string", "const": "2.0"},
                "id": {"$ref": "#/components/schemas/rpcId"},
                "result": {"type": "object", "additionalProperties": true}
            },
            "additionalProperties": false
        }),
    );
    components.insert("RPCErrorPayload".to_string(), rpc_error_payload);
    components.insert(
        "RPCErrorResponse".to_string(),
        json!({
            "type": "object",
            "required": ["id", "error"],
            "properties": {
                "jsonrpc": {"type": "string", "const": "2.0"},
                "id": {"$ref": "#/components/schemas/rpcId"},
                "error": {"$ref": "#/components/schemas/RPCErrorPayload"}
            },
            "additionalProperties": false
        }),
    );

    let mut paths = BTreeMap::new();
    let mut method_request_refs = Vec::with_capacity(methods.len());
    let mut method_response_refs = Vec::with_capacity(methods.len());

    for method in methods {
        let method_id = to_pascal_case(&method.method);
        let params_schema_name = format!("{method_id}Params");
        let result_schema_name = format!("{method_id}Result");
        let request_schema_name = format!("{method_id}Request");
        let response_schema_name = format!("{method_id}Response");

        let source = schema_sources
            .iter()
            .find(|source| source.path == method.source_path)
            .with_context(|| format!("missing schema source for method {}", method.method))?;

        let normalized_params = normalize_schema_with_refs(
            &method.params_schema,
            source,
            &mut components,
            &mut BTreeSet::new(),
        )?;
        let normalized_result = normalize_schema_with_refs(
            &method.result_schema,
            source,
            &mut components,
            &mut BTreeSet::new(),
        )?;
        components.insert(params_schema_name.clone(), normalized_params);
        components.insert(result_schema_name.clone(), normalized_result);

        components.insert(
            request_schema_name.clone(),
            json!({
                "allOf": [
                    {"$ref": "#/components/schemas/RPCRequest"},
                    {
                        "type": "object",
                        "required": ["method", "params"],
                        "properties": {
                            "method": {"type": "string", "enum": [method.method]},
                            "params": {"$ref": format!("#/components/schemas/{params_schema_name}")}
                        },
                    }
                ]
            }),
        );
        method_request_refs.push(format!("#/components/schemas/{request_schema_name}"));

        components.insert(
            response_schema_name.clone(),
            json!({
                "allOf": [
                    {"$ref": "#/components/schemas/RPCSuccess"},
                    {
                        "type": "object",
                        "required": ["result"],
                        "properties": {
                            "result": {"$ref": format!("#/components/schemas/{result_schema_name}")}
                        }
                    }
                ]
            }),
        );
        method_response_refs.push(format!("#/components/schemas/{response_schema_name}"));
    }

    components.insert("RPCRequestUnion".to_string(), one_of_union(&method_request_refs));
    let mut response_variants = Vec::with_capacity(method_response_refs.len() + 1);
    response_variants.push("#/components/schemas/RPCErrorResponse".to_string());
    response_variants.extend(method_response_refs);
    components.insert("RPCResponseUnion".to_string(), one_of_union(&response_variants));

    let mut responses = BTreeMap::new();
    responses.insert(
        "200".to_string(),
        OpenApiResponse {
            description: "RPC response".to_string(),
            content: Some({
                let mut content = BTreeMap::new();
                content.insert(
                    "application/json".to_string(),
                    OpenApiMediaType {
                        schema: json!({"$ref": "#/components/schemas/RPCResponseUnion"}),
                    },
                );
                content
            }),
        },
    );

    let rpc_methods = methods.iter().map(|method| method.method.clone()).collect::<Vec<_>>();
    paths.insert(
        "/rpc".to_string(),
        OpenApiPathItem {
            post: OpenApiOperation {
                operation_id: "rpc".to_string(),
                jsonrpc_methods: rpc_methods.clone(),
                jsonrpc_method: rpc_methods,
                request_body: OpenApiRequestBody {
                    required: true,
                    content: {
                        let mut content = BTreeMap::new();
                        content.insert(
                            "application/json".to_string(),
                            OpenApiMediaType {
                                schema: json!({"$ref": "#/components/schemas/RPCRequestUnion"}),
                            },
                        );
                        content
                    },
                },
                responses,
            },
        },
    );

    let spec = OpenApiSpec {
        openapi: backend.openapi_version.clone(),
        info: OpenApiInfo {
            title: format!("LXMF {} Client API", manifest.schema_namespace),
            version: format!("{}+v{}", manifest.contract_release, manifest.version),
        },
        paths,
        components: OpenApiComponents { schemas: components },
    };

    let mut spec = spec;
    for schema in spec.components.schemas.values_mut() {
        sanitize_component_self_references(schema);
    }
    promote_inline_component_property(
        &mut spec.components.schemas,
        "ResponseMeta",
        "rpc_endpoint",
        "ResponseMetaRpcEndpoint",
    );
    promote_inline_component_property(
        &mut spec.components.schemas,
        "SdkNegotiateV2Params",
        "config",
        "SdkNegotiateV2ParamsConfig",
    );

    let mut encoded = serde_json::to_vec_pretty(&spec).context("serialize OpenAPI spec")?;
    encoded.push(b'\n');

    if let Some(parent) = spec_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create OpenAPI spec parent {}", parent.display()))?;
    }
    fs::write(spec_path, &encoded)
        .with_context(|| format!("write OpenAPI spec {}", spec_path.display()))?;

    Ok(sha256_hex(&encoded))
}

fn promote_inline_component_property(
    components: &mut BTreeMap<String, Value>,
    parent_component: &str,
    property: &str,
    promoted_component: &str,
) {
    if components.contains_key(promoted_component) {
        return;
    }

    let Some(schema) = components
        .get_mut(parent_component)
        .and_then(Value::as_object_mut)
        .and_then(|schema| schema.get_mut("properties"))
        .and_then(Value::as_object_mut)
        .and_then(|properties| properties.get_mut(property))
    else {
        return;
    };

    let promoted = std::mem::replace(
        schema,
        json!({"$ref": format!("#/components/schemas/{promoted_component}")}),
    );
    components.insert(promoted_component.to_string(), promoted);
}

fn select_error_schema_source(sources: &[SchemaSource]) -> Option<&SchemaSource> {
    sources.iter().find(|source| {
        source
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with("error.schema.json"))
    })
}

fn select_error_schema(source: &SchemaSource) -> Value {
    source.schema.clone()
}

fn resolve_rpc_id_schema(sources: &[SchemaSource]) -> Value {
    for source in sources {
        if source.kind == SchemaSourceKind::OpenRpc {
            if let Some(rpc_id) = source
                .schema
                .get("components")
                .and_then(|item| item.get("schemas"))
                .and_then(|schemas| schemas.get("RpcId"))
            {
                return rpc_id.clone();
            }
            continue;
        }
        if source.path.to_string_lossy().ends_with("error.schema.json") {
            continue;
        }
        if let Some(defs) = source.schema.get("$defs").and_then(Value::as_object) {
            if let Some(rpc_id) = defs.get("rpc_id") {
                return rpc_id.clone();
            }
        }
    }

    json!({
        "oneOf": [
            {"type": "string", "minLength": 1},
            {"type": "integer", "minimum": 0}
        ]
    })
}

fn source_to_component_name(prefix: &str, def_name: &str) -> String {
    if prefix.is_empty() {
        return def_name.to_string();
    }
    format!("{prefix}{}", to_pascal_case(def_name))
}

fn normalize_schema_with_refs(
    schema: &Value,
    source: &SchemaSource,
    components: &mut BTreeMap<String, Value>,
    in_progress: &mut BTreeSet<String>,
) -> Result<Value> {
    match schema {
        Value::Object(map) => {
            if let Some(reference) = map.get("$ref").and_then(Value::as_str) {
                if let Some(def_name) = reference.strip_prefix("#/$defs/") {
                    let component_name =
                        source_to_component_name(&source.def_component_prefix, def_name);
                    if !components.contains_key(&component_name) {
                        let defs = source
                            .schema
                            .get("$defs")
                            .and_then(Value::as_object)
                            .context("schema missing $defs")?;
                        let def_schema = defs
                            .get(def_name)
                            .with_context(|| format!("missing $defs entry {def_name}"))?;

                        if !in_progress.insert(component_name.clone()) {
                            return Ok(
                                json!({ "$ref": format!("#/components/schemas/{component_name}") }),
                            );
                        }
                        let normalized = normalize_schema_with_refs(
                            def_schema,
                            source,
                            components,
                            in_progress,
                        )?;
                        in_progress.remove(&component_name);
                        components.insert(component_name.clone(), normalized);
                    }

                    return Ok(json!({ "$ref": format!("#/components/schemas/{component_name}") }));
                }

                if let Some(def_name) = reference.strip_prefix("#/components/schemas/") {
                    let component_name =
                        source_to_component_name(&source.def_component_prefix, def_name);
                    if !components.contains_key(&component_name) {
                        let defs = source
                            .schema
                            .get("components")
                            .and_then(|item| item.get("schemas"))
                            .and_then(Value::as_object)
                            .context("OpenRPC contract missing components.schemas")?;
                        let def_schema = defs.get(def_name).with_context(|| {
                            format!("missing OpenRPC component schema {def_name}")
                        })?;

                        if !in_progress.insert(component_name.clone()) {
                            return Ok(
                                json!({ "$ref": format!("#/components/schemas/{component_name}") }),
                            );
                        }
                        let normalized = normalize_schema_with_refs(
                            def_schema,
                            source,
                            components,
                            in_progress,
                        )?;
                        in_progress.remove(&component_name);
                        components.insert(component_name.clone(), normalized);
                    }

                    return Ok(json!({ "$ref": format!("#/components/schemas/{component_name}") }));
                }

                bail!("unsupported $ref '{reference}' in schema {}", source.path.display());
            }

            let mut out = Map::new();
            for (key, value) in map {
                out.insert(
                    key.clone(),
                    normalize_schema_with_refs(value, source, components, in_progress)?,
                );
            }
            let value = Value::Object(out);
            Ok(normalize_schema(&value))
        }
        Value::Array(values) => {
            let normalized = values
                .iter()
                .map(|value| normalize_schema_with_refs(value, source, components, in_progress))
                .collect::<Result<Vec<_>>>()?;
            Ok(Value::Array(normalized))
        }
        _ => Ok(schema.clone()),
    }
}

fn sanitize_component_self_references(schema: &mut Value) {
    match schema {
        Value::Object(map) => {
            map.remove("$defs");

            for value in map.values_mut() {
                sanitize_component_self_references(value);
            }
        }
        Value::Array(list) => {
            for value in list {
                sanitize_component_self_references(value);
            }
        }
        _ => {}
    }
}

fn compare_file_bytes(expected: impl AsRef<Path>, actual: impl AsRef<Path>) -> Result<()> {
    let expected_bytes = fs::read(expected.as_ref())
        .with_context(|| format!("read expected {}", expected.as_ref().display()))?;
    let actual_bytes = fs::read(actual.as_ref())
        .with_context(|| format!("read actual {}", actual.as_ref().display()))?;
    if expected_bytes != actual_bytes {
        bail!(
            "generated OpenAPI spec {} does not match {}",
            actual.as_ref().display(),
            expected.as_ref().display()
        );
    }
    Ok(())
}

fn compare_spec_hash_to_baseline(path: &Path, spec_hash: &str) -> Result<()> {
    let baseline = load_generation_baseline(path)?;
    if baseline.spec_hash != spec_hash {
        bail!(
            "generated OpenAPI spec hash mismatch for baseline {}, got {}, expected {}",
            path.display(),
            spec_hash,
            baseline.spec_hash
        );
    }
    Ok(())
}
