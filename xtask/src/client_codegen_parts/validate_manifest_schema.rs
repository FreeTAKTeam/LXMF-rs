fn validate_manifest_schema(manifest_path: &Path, manifest_value: &Value) -> Result<()> {
    let schema_path = Path::new(CLIENT_GENERATION_MANIFEST_SCHEMA_PATH);
    let schema_raw = fs::read_to_string(schema_path)
        .with_context(|| format!("read manifest schema {}", schema_path.display()))?;
    let schema: Value = serde_json::from_str(&schema_raw)
        .with_context(|| format!("parse manifest schema {}", schema_path.display()))?;
    let validator =
        JSONSchema::options().with_draft(Draft::Draft202012).compile(&schema).map_err(|error| {
            anyhow!("manifest schema {} failed to compile: {error}", schema_path.display())
        })?;

    if let Err(errors) = validator.validate(manifest_value) {
        let details = errors.map(|error| error.to_string()).collect::<Vec<_>>().join("; ");
        bail!(
            "manifest {} failed schema validation ({}): {details}",
            manifest_path.display(),
            CLIENT_GENERATION_MANIFEST_SCHEMA_PATH
        );
    }

    Ok(())
}

fn discover_schema_sources(
    workspace: &Path,
    manifest: &ClientGenerationManifest,
) -> Result<Vec<SchemaSource>> {
    let mut sources = Vec::new();

    if let Some(contract_path) = manifest.openrpc_contract_file.as_deref() {
        let path = workspace.join(contract_path);
        if !path.is_file() {
            bail!("manifest references missing OpenRPC contract {contract_path}");
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("read OpenRPC contract {}", path.display()))?;
        let schema = serde_json::from_str::<Value>(&raw)
            .with_context(|| format!("parse OpenRPC contract {}", path.display()))?;
        validate_openrpc_contract(&schema, &path)?;
        sources.push(SchemaSource {
            path,
            schema,
            def_component_prefix: String::new(),
            kind: SchemaSourceKind::OpenRpc,
        });
    }

    let schema_paths = resolve_schema_paths(workspace, manifest)?;

    for path in schema_paths {
        let raw =
            fs::read_to_string(&path).with_context(|| format!("read schema {}", path.display()))?;
        let schema = serde_json::from_str::<Value>(&raw)
            .with_context(|| format!("parse schema {}", path.display()))?;
        sources.push(SchemaSource {
            path: path.clone(),
            schema,
            def_component_prefix: schema_source_prefix(&path),
            kind: SchemaSourceKind::JsonSchemaDefs,
        });
    }

    Ok(sources)
}

fn discover_methods(schema_sources: &[SchemaSource]) -> Result<Vec<MethodDescriptor>> {
    let mut seen: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut methods = Vec::new();

    for source in schema_sources {
        if is_error_schema(&source.path) {
            continue;
        }
        let discovered = extract_methods_from_schema(&source.schema, &source.path)?;
        for method in discovered {
            if let Some(previous_source) = seen.get(&method.method) {
                bail!(
                    "duplicate RPC method '{}' discovered in {} and {}",
                    method.method,
                    previous_source.display(),
                    source.path.display()
                );
            }
            seen.insert(method.method.clone(), method.source_path.clone());
            methods.push(method);
        }
    }

    if methods.is_empty() {
        bail!("no RPC methods discovered from manifest schemas");
    }

    methods.sort_by(|a, b| a.method.cmp(&b.method));
    Ok(methods)
}

fn sync_legacy_rpc_schemas(
    workspace: &Path,
    schema_sources: &[SchemaSource],
    mode: SchemaClientMode,
) -> Result<()> {
    let Some(openrpc_source) =
        schema_sources.iter().find(|source| source.kind == SchemaSourceKind::OpenRpc)
    else {
        return Ok(());
    };

    let artifacts = project_legacy_rpc_schemas(&openrpc_source.schema)?;
    for artifact in artifacts {
        let destination = workspace.join(&artifact.path);
        let canonical = canonicalize_json(artifact.schema);
        let mut encoded = serde_json::to_vec_pretty(&canonical)
            .with_context(|| format!("serialize {}", artifact.path.display()))?;
        encoded.push(b'\n');

        match mode {
            SchemaClientMode::Write => {
                if let Some(parent) = destination.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("create legacy rpc dir {}", parent.display()))?;
                }
                fs::write(&destination, &encoded)
                    .with_context(|| format!("write {}", destination.display()))?;
            }
            SchemaClientMode::Check => {
                if !destination.is_file() {
                    bail!("missing generated legacy RPC schema {}", destination.display());
                }
                let existing = fs::read(&destination)
                    .with_context(|| format!("read {}", destination.display()))?;
                if existing != encoded {
                    bail!(
                        "generated legacy RPC schema {} is out of date; run `cargo xtask schema-client-generate`",
                        destination.display()
                    );
                }
            }
        }
    }

    Ok(())
}

fn project_legacy_rpc_schemas(openrpc: &Value) -> Result<Vec<LegacyRpcSchemaArtifact>> {
    let methods = openrpc
        .get("methods")
        .and_then(Value::as_array)
        .context("OpenRPC contract missing methods")?;
    let components = openrpc
        .get("components")
        .and_then(|item| item.get("schemas"))
        .and_then(Value::as_object)
        .context("OpenRPC contract missing components.schemas")?;

    let release_b_methods =
        grouped_method_set(components, "SdkReleaseBRequestEnvelope", "release B request")?;
    let release_c_methods =
        grouped_method_set(components, "SdkReleaseCRequestEnvelope", "release C request")?;

    let discovered_methods = methods
        .iter()
        .map(|method| {
            method
                .get("name")
                .and_then(Value::as_str)
                .context("OpenRPC method missing name")
                .map(str::to_string)
        })
        .collect::<Result<BTreeSet<_>>>()?;

    let mut core_methods = BTreeSet::new();
    for method in methods {
        let method_name =
            method.get("name").and_then(Value::as_str).context("OpenRPC method missing name")?;
        if release_b_methods.contains(method_name) || release_c_methods.contains(method_name) {
            continue;
        }
        if method
            .get("x-client-generation")
            .and_then(Value::as_bool)
            .is_some_and(|enabled| !enabled)
        {
            bail!(
                "OpenRPC method {method_name} is excluded from client generation but not mapped to a grouped legacy schema"
            );
        }
        core_methods.insert(method_name.to_string());
    }

    let mut assigned_methods = BTreeSet::new();
    assigned_methods.extend(core_methods.iter().cloned());
    assigned_methods.extend(release_b_methods.iter().cloned());
    assigned_methods.extend(release_c_methods.iter().cloned());

    if assigned_methods != discovered_methods {
        let missing = discovered_methods.difference(&assigned_methods).cloned().collect::<Vec<_>>();
        let unexpected =
            assigned_methods.difference(&discovered_methods).cloned().collect::<Vec<_>>();
        bail!(
            "OpenRPC legacy projection coverage mismatch: missing={missing:?}, unexpected={unexpected:?}"
        );
    }

    if !release_b_methods.is_disjoint(&release_c_methods) {
        let overlap =
            release_b_methods.intersection(&release_c_methods).cloned().collect::<Vec<_>>();
        bail!("OpenRPC release B/C method grouping overlaps: {overlap:?}");
    }

    let mut artifacts = Vec::new();
    for method_name in core_methods {
        artifacts.push(project_core_legacy_rpc_schema(&method_name, components)?);
    }

    artifacts.push(project_grouped_legacy_rpc_schema(
        "sdk_release_b_methods.schema.json",
        "LXMF SDK RPC Release B Domain Methods v2",
        "SdkReleaseBRequestEnvelope",
        Some("SdkReleaseBResponseOkEnvelope"),
        "SdkReleaseBResponseErrorEnvelope",
        Some("SdkReleaseBResult"),
        Some("ReleaseBExtensionMap"),
        components,
    )?);
    artifacts.push(project_grouped_legacy_rpc_schema(
        "sdk_release_c_methods.schema.json",
        "LXMF SDK RPC Release C Domain Methods v2",
        "SdkReleaseCRequestEnvelope",
        None,
        "SdkReleaseCResponseErrorEnvelope",
        None,
        Some("ReleaseCExtensionMap"),
        components,
    )?);

    artifacts.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(artifacts)
}

fn grouped_method_set(
    components: &Map<String, Value>,
    request_component: &str,
    context_label: &str,
) -> Result<BTreeSet<String>> {
    let method_enum = components
        .get(request_component)
        .and_then(|schema| schema.get("properties"))
        .and_then(Value::as_object)
        .and_then(|props| props.get("method"))
        .and_then(|schema| schema.get("enum"))
        .and_then(Value::as_array)
        .with_context(|| format!("OpenRPC contract missing {context_label} method enum"))?;

    let mut methods = BTreeSet::new();
    for method in method_enum {
        methods.insert(
            method
                .as_str()
                .with_context(|| format!("{context_label} method enum contains non-string"))?
                .to_string(),
        );
    }
    Ok(methods)
}

fn project_core_legacy_rpc_schema(
    method_name: &str,
    components: &Map<String, Value>,
) -> Result<LegacyRpcSchemaArtifact> {
    let method_id = to_pascal_case(method_name);
    let params_component = format!("{method_id}Params");
    let result_component = format!("{method_id}Result");
    let request_component = format!("{method_id}RequestEnvelope");
    let response_ok_component = format!("{method_id}ResponseOkEnvelope");
    let response_error_component = format!("{method_id}ResponseErrorEnvelope");
    let mut ref_map = BTreeMap::from([
        (format!("#/components/schemas/{method_id}Params"), "#/$defs/params".to_string()),
        (format!("#/components/schemas/{method_id}Result"), "#/$defs/result".to_string()),
        ("#/components/schemas/RpcId".to_string(), "#/$defs/rpc_id".to_string()),
        ("#/components/schemas/RpcError".to_string(), "#/$defs/rpc_error".to_string()),
    ]);
    let extra_components = legacy_projection_extra_components(
        components,
        &[
            params_component.as_str(),
            result_component.as_str(),
            request_component.as_str(),
            response_ok_component.as_str(),
            response_error_component.as_str(),
        ],
    )?;
    for (component, def_key) in &extra_components {
        ref_map.insert(format!("#/components/schemas/{component}"), format!("#/$defs/{def_key}"));
    }

    let request = rewrite_schema_refs(component_schema(components, &request_component)?, &ref_map)?;
    let response_ok =
        rewrite_schema_refs(component_schema(components, &response_ok_component)?, &ref_map)?;
    let response_error =
        rewrite_schema_refs(component_schema(components, &response_error_component)?, &ref_map)?;
    let mut defs = Map::new();
    defs.insert("rpc_id".to_string(), component_schema(components, "RpcId")?);
    defs.insert("rpc_error".to_string(), component_schema(components, "RpcError")?);
    defs.insert(
        "params".to_string(),
        rewrite_schema_refs(component_schema(components, &params_component)?, &ref_map)?,
    );
    defs.insert(
        "result".to_string(),
        rewrite_schema_refs(component_schema(components, &result_component)?, &ref_map)?,
    );
    for (component, def_key) in extra_components {
        defs.insert(
            def_key.to_string(),
            rewrite_schema_refs(component_schema(components, component)?, &ref_map)?,
        );
    }
    defs.insert("request".to_string(), request);
    defs.insert("response_ok".to_string(), response_ok);
    defs.insert("response_error".to_string(), response_error);

    Ok(LegacyRpcSchemaArtifact {
        path: Path::new(LEGACY_RPC_SCHEMA_DIR).join(format!("{method_name}.schema.json")),
        schema: json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": format!("https://weft.tak/contracts/sdk/v2/rpc/{method_name}.schema.json"),
            "title": format!("LXMF SDK RPC {method_name} v2"),
            "oneOf": [
                { "$ref": "#/$defs/request" },
                { "$ref": "#/$defs/response_ok" },
                { "$ref": "#/$defs/response_error" }
            ],
            "$defs": defs
        }),
    })
}

fn legacy_projection_extra_components(
    components: &Map<String, Value>,
    root_components: &[&str],
) -> Result<Vec<(&'static str, &'static str)>> {
    let response_meta_ref = "#/components/schemas/ResponseMeta";
    let python_reference_ref = "#/components/schemas/PythonReference";
    let software_parity_ref = "#/components/schemas/SoftwareParityOrientation";
    let send_batch_message_ref = "#/components/schemas/SdkSendBatchV2Message";
    let send_batch_result_item_ref = "#/components/schemas/SdkSendBatchV2ResultItem";
    let mut needs_response_meta = false;
    let mut needs_python_reference = false;
    let mut needs_software_parity = false;
    let mut needs_send_batch_message = false;
    let mut needs_send_batch_result_item = false;
    for component in root_components {
        let schema = component_schema(components, component)?;
        needs_response_meta |= schema_mentions_ref(&schema, response_meta_ref);
        needs_python_reference |= schema_mentions_ref(&schema, python_reference_ref);
        needs_software_parity |= schema_mentions_ref(&schema, software_parity_ref);
        needs_send_batch_message |= schema_mentions_ref(&schema, send_batch_message_ref);
        needs_send_batch_result_item |= schema_mentions_ref(&schema, send_batch_result_item_ref);
    }
    if needs_response_meta {
        let response_meta = component_schema(components, "ResponseMeta")?;
        needs_python_reference |= schema_mentions_ref(&response_meta, python_reference_ref);
    }

    let mut extras = Vec::new();
    if needs_python_reference {
        extras.push(("PythonReference", "python_reference"));
    }
    if needs_software_parity {
        extras.extend([
            ("ParityLevel", "parity_level"),
            ("ParityRatio", "parity_ratio"),
            ("ParityInventory", "parity_inventory"),
            ("ParityCheckpoint", "parity_checkpoint"),
            ("ReferenceRevision", "reference_revision"),
            ("SoftwareParityReferences", "software_parity_references"),
            ("SoftwareParityOrientation", "software_parity_orientation"),
        ]);
    }
    if needs_response_meta {
        extras.push(("ResponseMeta", "response_meta"));
    }
    if needs_send_batch_message {
        extras.push(("SdkSendBatchV2Message", "send_batch_message"));
    }
    if needs_send_batch_result_item {
        extras.push(("SdkSendBatchV2ResultItem", "send_batch_result_item"));
    }
    Ok(extras)
}

fn schema_mentions_ref(schema: &Value, target_ref: &str) -> bool {
    match schema {
        Value::Object(map) => {
            map.get("$ref").and_then(Value::as_str) == Some(target_ref)
                || map.values().any(|value| schema_mentions_ref(value, target_ref))
        }
        Value::Array(items) => items.iter().any(|value| schema_mentions_ref(value, target_ref)),
        _ => false,
    }
}
