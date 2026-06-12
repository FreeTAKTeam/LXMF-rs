#[allow(clippy::too_many_arguments)]
fn project_grouped_legacy_rpc_schema(
    file_name: &str,
    title: &str,
    request_component: &str,
    response_ok_component: Option<&str>,
    response_error_component: &str,
    result_component: Option<&str>,
    extension_map_component: Option<&str>,
    components: &Map<String, Value>,
) -> Result<LegacyRpcSchemaArtifact> {
    let mut defs = Map::new();
    defs.insert("rpc_id".to_string(), component_schema(components, "RpcId")?);
    defs.insert("rpc_error".to_string(), component_schema(components, "RpcError")?);

    let mut ref_map = BTreeMap::from([
        ("#/components/schemas/RpcId".to_string(), "#/$defs/rpc_id".to_string()),
        ("#/components/schemas/RpcError".to_string(), "#/$defs/rpc_error".to_string()),
    ]);

    if let Some(extension_map_component) = extension_map_component {
        defs.insert(
            "extension_map".to_string(),
            component_schema(components, extension_map_component)?,
        );
        ref_map.insert(
            format!("#/components/schemas/{extension_map_component}"),
            "#/$defs/extension_map".to_string(),
        );
    }

    if let Some(result_component) = result_component {
        defs.insert(
            "result".to_string(),
            rewrite_schema_refs(component_schema(components, result_component)?, &ref_map)?,
        );
        ref_map.insert(
            format!("#/components/schemas/{result_component}"),
            "#/$defs/result".to_string(),
        );
    }

    defs.insert(
        "request".to_string(),
        rewrite_schema_refs(component_schema(components, request_component)?, &ref_map)?,
    );
    if let Some(response_ok_component) = response_ok_component {
        defs.insert(
            "response_ok".to_string(),
            rewrite_schema_refs(component_schema(components, response_ok_component)?, &ref_map)?,
        );
    }
    defs.insert(
        "response_error".to_string(),
        rewrite_schema_refs(component_schema(components, response_error_component)?, &ref_map)?,
    );

    let mut one_of = vec![json!({ "$ref": "#/$defs/request" })];
    if response_ok_component.is_some() {
        one_of.push(json!({ "$ref": "#/$defs/response_ok" }));
    }
    one_of.push(json!({ "$ref": "#/$defs/response_error" }));

    Ok(LegacyRpcSchemaArtifact {
        path: Path::new(LEGACY_RPC_SCHEMA_DIR).join(file_name),
        schema: json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": format!("https://weft.tak/contracts/sdk/v2/rpc/{file_name}"),
            "title": title,
            "oneOf": one_of,
            "$defs": defs
        }),
    })
}

fn component_schema(components: &Map<String, Value>, name: &str) -> Result<Value> {
    components
        .get(name)
        .cloned()
        .with_context(|| format!("OpenRPC contract missing component schema {name}"))
}

fn rewrite_schema_refs(schema: Value, ref_map: &BTreeMap<String, String>) -> Result<Value> {
    match schema {
        Value::Object(map) => {
            let mut rewritten = Map::new();
            for (key, value) in map {
                if key == "$ref" {
                    let reference = value
                        .as_str()
                        .context("schema $ref value must be a string during rewrite")?;
                    let rewritten_ref = ref_map.get(reference).with_context(|| {
                        format!("no legacy compatibility ref mapping for {reference}")
                    })?;
                    rewritten.insert(key, Value::String(rewritten_ref.clone()));
                } else {
                    rewritten.insert(key, rewrite_schema_refs(value, ref_map)?);
                }
            }
            Ok(normalize_schema(&Value::Object(rewritten)))
        }
        Value::Array(values) => Ok(Value::Array(
            values
                .into_iter()
                .map(|value| rewrite_schema_refs(value, ref_map))
                .collect::<Result<Vec<_>>>()?,
        )),
        other => Ok(other),
    }
}

fn canonicalize_json(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries = map
                .into_iter()
                .map(|(key, value)| (key, canonicalize_json(value)))
                .collect::<Vec<_>>();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));

            let mut ordered = Map::new();
            for (key, value) in entries {
                ordered.insert(key, value);
            }
            Value::Object(ordered)
        }
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize_json).collect()),
        other => other,
    }
}

fn schema_source_prefix(path: &Path) -> String {
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("schema");
    let stem = file_name.strip_suffix(".schema.json").unwrap_or(file_name);
    to_pascal_case(stem).replace("Schema", "")
}

fn resolve_schema_paths(
    workspace: &Path,
    manifest: &ClientGenerationManifest,
) -> Result<Vec<PathBuf>> {
    let mode = manifest
        .schema_discovery
        .as_ref()
        .map(|cfg| SchemaDiscoveryMode::parse(&cfg.mode))
        .transpose()?
        .unwrap_or(SchemaDiscoveryMode::RequiredSchemas);

    let mut paths = Vec::new();

    for path in &manifest.required_schemas {
        let full = workspace.join(path);
        if !full.is_file() {
            bail!("manifest references missing schema {path}");
        }
        if !paths.contains(&full) {
            paths.push(full);
        }
    }

    if mode == SchemaDiscoveryMode::ManifestOnly {
        if let Some(cfg) = &manifest.schema_discovery {
            for pattern in cfg.include_globs.iter().flatten() {
                for path in discover_schema_glob(workspace, pattern)? {
                    if !paths.contains(&path) {
                        paths.push(path);
                    }
                }
            }
        }
    }

    Ok(paths)
}

fn validate_openapi_references(spec_path: &Path) -> Result<()> {
    let raw = fs::read_to_string(spec_path)
        .with_context(|| format!("read openapi spec {}", spec_path.display()))?;
    let spec: Value = serde_json::from_str(&raw)
        .with_context(|| format!("parse openapi spec {}", spec_path.display()))?;

    let mut missing = Vec::new();
    let mut stack: Vec<(&Value, String)> = Vec::new();
    stack.push((&spec, "/".to_string()));

    while let Some((value, path)) = stack.pop() {
        match value {
            Value::Array(values) => {
                for (index, item) in values.iter().enumerate() {
                    stack.push((item, format!("{path}{index}/")));
                }
            }
            Value::Object(map) => {
                for (key, entry) in map {
                    let entry_path = format!("{path}{key}/");
                    if key == "$ref" {
                        if let Some(reference) = entry.as_str() {
                            if reference.starts_with("#/")
                                && !json_pointer_resolves(&spec, reference)
                            {
                                missing.push(format!("{} -> {}", path, reference));
                            }
                        }
                    }
                    stack.push((entry, entry_path));
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }

    if !missing.is_empty() {
        bail!("openapi spec has unresolved refs: {}", missing.join(", "));
    }

    Ok(())
}

fn validate_openrpc_contract(contract: &Value, contract_path: &Path) -> Result<()> {
    let object = contract.as_object().with_context(|| {
        format!("OpenRPC contract {} root must be object", contract_path.display())
    })?;

    let version = object.get("openrpc").and_then(Value::as_str).with_context(|| {
        format!("OpenRPC contract {} missing openrpc version", contract_path.display())
    })?;
    if version.trim().is_empty() {
        bail!("OpenRPC contract {} has empty openrpc version", contract_path.display());
    }

    let methods = object.get("methods").and_then(Value::as_array).with_context(|| {
        format!("OpenRPC contract {} missing methods array", contract_path.display())
    })?;
    if methods.is_empty() {
        bail!("OpenRPC contract {} has no methods", contract_path.display());
    }

    let schemas = object
        .get("components")
        .and_then(|item| item.get("schemas"))
        .and_then(Value::as_object)
        .with_context(|| {
            format!("OpenRPC contract {} missing components.schemas", contract_path.display())
        })?;
    if schemas.is_empty() {
        bail!("OpenRPC contract {} has no component schemas", contract_path.display());
    }

    let mut seen_methods = BTreeSet::new();
    for method in methods {
        let name = method.get("name").and_then(Value::as_str).with_context(|| {
            format!("OpenRPC contract {} method entry missing name", contract_path.display())
        })?;
        if !seen_methods.insert(name.to_string()) {
            bail!(
                "OpenRPC contract {} contains duplicate method {}",
                contract_path.display(),
                name
            );
        }
        if method.get("result").is_none() {
            bail!("OpenRPC contract {} method {} missing result", contract_path.display(), name);
        }
    }

    let mut missing = Vec::new();
    let mut stack: Vec<(&Value, String)> = vec![(contract, "/".to_string())];
    while let Some((value, path)) = stack.pop() {
        match value {
            Value::Array(values) => {
                for (index, item) in values.iter().enumerate() {
                    stack.push((item, format!("{path}{index}/")));
                }
            }
            Value::Object(map) => {
                for (key, entry) in map {
                    let entry_path = format!("{path}{key}/");
                    if key == "$ref" {
                        if let Some(reference) = entry.as_str() {
                            if reference.starts_with("#/")
                                && !json_pointer_resolves(contract, reference)
                            {
                                missing.push(format!("{} -> {}", path, reference));
                            }
                        }
                    }
                    stack.push((entry, entry_path));
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }

    if !missing.is_empty() {
        bail!(
            "OpenRPC contract {} has unresolved refs: {}",
            contract_path.display(),
            missing.join(", ")
        );
    }

    Ok(())
}

fn json_pointer_resolves(spec: &Value, reference: &str) -> bool {
    let pointer = reference.trim_start_matches("#/");
    let mut cursor = spec;

    if pointer.is_empty() {
        return true;
    }

    for segment in pointer.split('/') {
        let decoded = segment.replace("~1", "/").replace("~0", "~");
        if let Ok(index) = decoded.parse::<usize>() {
            match cursor {
                Value::Array(entries) if index < entries.len() => {
                    cursor = &entries[index];
                }
                _ => return false,
            }
        } else {
            match cursor {
                Value::Object(entries) if entries.contains_key(&decoded) => {
                    cursor = &entries[&decoded];
                }
                _ => return false,
            }
        }
    }

    true
}

fn discover_schema_glob(workspace: &Path, pattern: &str) -> Result<Vec<PathBuf>> {
    let base = workspace.join("docs/schemas/sdk/v2/rpc");
    if !base.is_dir() {
        bail!("expected schema directory {}", base.display());
    }

    let glob = Glob::new(pattern).with_context(|| {
        format!("invalid glob pattern '{pattern}' in schema_discovery.include_globs")
    })?;
    let matcher = GlobSetBuilder::new().add(glob).build()?;

    let mut out = Vec::new();
    for entry in collect_paths_recursive(&base)? {
        let entry_str = entry.to_string_lossy();
        let relative = entry
            .strip_prefix(&base)
            .ok()
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_else(|| entry_str.to_string());
        let relative = relative.replace('\\', "/");

        if matcher.is_match(relative.as_str()) || matcher.is_match(&*entry_str) {
            out.push(entry);
        }
    }

    if out.is_empty() {
        bail!("schema discovery glob {pattern} returned no matches");
    }

    out.sort_unstable();

    Ok(out)
}

fn is_error_schema(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with("error.schema.json"))
}

fn collect_paths_recursive(path: &Path) -> Result<Vec<PathBuf>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(path).with_context(|| format!("read_dir {}", path.display()))? {
        let entry = entry.with_context(|| format!("read entry in {}", path.display()))?;
        entries.push(entry);
    }

    entries.sort_by_key(|entry| entry.path());
    let mut out = Vec::new();
    for entry in entries {
        let item = entry.path();
        if item.is_dir() {
            out.extend(collect_paths_recursive(&item)?);
            continue;
        }
        if item.is_file() {
            out.push(item);
        }
    }
    Ok(out)
}
