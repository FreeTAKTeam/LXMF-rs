fn extract_methods_from_schema(
    schema: &Value,
    source_name: &Path,
) -> Result<Vec<MethodDescriptor>> {
    if schema.get("openrpc").is_some() {
        return extract_methods_from_openrpc(schema, source_name);
    }

    let source_path = source_name.to_path_buf();
    let defs = schema.get("$defs").and_then(Value::as_object).context("schema missing $defs")?;
    let request =
        defs.get("request").and_then(Value::as_object).context("schema missing $defs.request")?;
    let request_properties = request
        .get("properties")
        .and_then(Value::as_object)
        .context("schema request missing properties")?;

    let method_schema = request_properties
        .get("method")
        .and_then(Value::as_object)
        .context("schema request missing properties.method")?;

    let base_params = request_properties
        .get("params")
        .cloned()
        .unwrap_or_else(|| json!({"type":"object","properties":{}}));

    let base_result = defs
        .get("response_ok")
        .and_then(|def| def.get("properties"))
        .and_then(|props| props.get("result"))
        .cloned()
        .unwrap_or_else(method_fallback_result_schema);

    if let Some(method_name) = method_schema.get("const").and_then(Value::as_str) {
        return Ok(vec![MethodDescriptor {
            method: method_name.to_string(),
            params_schema: normalize_schema(&base_params),
            result_schema: normalize_schema(&base_result),
            source_path: source_path.clone(),
        }]);
    }

    let enum_methods = method_schema
        .get("enum")
        .and_then(Value::as_array)
        .context("schema request method does not define const or enum")?;

    let mut method_names = Vec::new();
    for method in enum_methods {
        method_names.push(
            method
                .as_str()
                .with_context(|| {
                    format!("method enum entry is not a string in {}", source_name.display())
                })?
                .to_string(),
        );
    }

    let mut method_params: BTreeMap<String, Value> =
        method_names.into_iter().map(|method| (method, normalize_schema(&base_params))).collect();

    for branch in request.get("allOf").and_then(Value::as_array).into_iter().flatten() {
        let branch = branch
            .as_object()
            .with_context(|| format!("allOf entry in {} must be object", source_name.display()))?;

        let if_schema =
            branch.get("if").and_then(Value::as_object).context("allOf branch missing if")?;
        let then_schema =
            branch.get("then").and_then(Value::as_object).context("allOf branch missing then")?;

        let methods = extract_methods_from_if(if_schema)?;
        if methods.is_empty() {
            continue;
        }

        let params = then_schema
            .get("properties")
            .and_then(Value::as_object)
            .and_then(|props| props.get("params"))
            .unwrap_or(&base_params);

        for method_name in methods {
            method_params.insert(method_name, normalize_schema(params));
        }
    }

    let mut out = Vec::new();
    for (method, params_schema) in method_params {
        out.push(MethodDescriptor {
            method,
            params_schema,
            result_schema: normalize_schema(&base_result),
            source_path: source_path.clone(),
        });
    }

    Ok(out)
}

fn extract_methods_from_openrpc(
    schema: &Value,
    source_name: &Path,
) -> Result<Vec<MethodDescriptor>> {
    let source_path = source_name.to_path_buf();
    let methods = schema
        .get("methods")
        .and_then(Value::as_array)
        .context("OpenRPC contract missing methods")?;
    let components = schema
        .get("components")
        .and_then(|item| item.get("schemas"))
        .and_then(Value::as_object)
        .context("OpenRPC contract missing components.schemas")?;

    let mut discovered = Vec::new();
    for method in methods {
        if method
            .get("x-client-generation")
            .and_then(Value::as_bool)
            .is_some_and(|enabled| !enabled)
        {
            continue;
        }
        let method_name = method
            .get("name")
            .and_then(Value::as_str)
            .with_context(|| format!("OpenRPC method in {} missing name", source_name.display()))?;
        let params = method
            .get("params")
            .and_then(Value::as_array)
            .with_context(|| format!("OpenRPC method {method_name} missing params"))?;
        let param_schema = params
            .first()
            .and_then(|item| item.get("schema"))
            .with_context(|| format!("OpenRPC method {method_name} missing params[0].schema"))?;
        let result_schema = method
            .get("result")
            .and_then(|item| item.get("schema"))
            .with_context(|| format!("OpenRPC method {method_name} missing result.schema"))?;

        discovered.push(MethodDescriptor {
            method: method_name.to_string(),
            params_schema: normalize_schema(&resolve_openrpc_schema_ref(
                param_schema,
                components,
                source_name,
                method_name,
                "params",
            )?),
            result_schema: normalize_schema(&resolve_openrpc_schema_ref(
                result_schema,
                components,
                source_name,
                method_name,
                "result",
            )?),
            source_path: source_path.clone(),
        });
    }

    Ok(discovered)
}

fn resolve_openrpc_schema_ref(
    schema: &Value,
    components: &Map<String, Value>,
    source_name: &Path,
    method_name: &str,
    field_name: &str,
) -> Result<Value> {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        let def_name = reference.strip_prefix("#/components/schemas/").with_context(|| {
            format!(
                "unsupported OpenRPC ref '{}' for method {} {} in {}",
                reference,
                method_name,
                field_name,
                source_name.display()
            )
        })?;
        return components
            .get(def_name)
            .cloned()
            .with_context(|| format!("missing OpenRPC component schema {}", def_name));
    }

    Ok(schema.clone())
}

fn extract_methods_from_if(if_schema: &Map<String, Value>) -> Result<Vec<String>> {
    let method = if_schema
        .get("properties")
        .and_then(Value::as_object)
        .and_then(|props| props.get("method"))
        .and_then(Value::as_object)
        .context("if branch missing properties.method")?;

    if let Some(constant) = method.get("const").and_then(Value::as_str) {
        return Ok(vec![constant.to_string()]);
    }

    let methods =
        method.get("enum").and_then(Value::as_array).context("if.method neither const nor enum")?;

    let mut out = Vec::new();
    for method in methods {
        out.push(
            method.as_str().with_context(|| "if.method.enum item is not a string")?.to_string(),
        );
    }
    Ok(out)
}

fn method_fallback_result_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": true,
    })
}

fn normalize_schema(schema: &Value) -> Value {
    let mut normalized = schema.clone();

    if let Some(obj) = normalized.as_object_mut() {
        if let Some(required) = obj.get_mut("required").and_then(Value::as_array_mut) {
            let mut sorted = required
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect::<Vec<_>>();
            sorted.sort_unstable();
            *required = sorted.into_iter().map(Value::from).collect();
        }

        if let Some(properties) = obj.get_mut("properties").and_then(Value::as_object_mut) {
            let mut entries: Vec<(String, Value)> = properties
                .iter()
                .map(|(name, schema)| (name.to_string(), schema.clone()))
                .collect();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            properties.clear();
            for (name, schema) in entries {
                properties.insert(name, schema);
            }
        }
    }

    normalized
}

fn one_of_union(refs: &[String]) -> Value {
    let mut sorted = refs.to_vec();
    sorted.sort_unstable();
    match sorted.as_slice() {
        [] => json!({"type":"object","additionalProperties":true}),
        [ref_name] => json!({ "$ref": ref_name }),
        _ => {
            let one_of = sorted.into_iter().map(|r| json!({"$ref": r})).collect::<Vec<_>>();
            json!({"oneOf": one_of})
        }
    }
}
