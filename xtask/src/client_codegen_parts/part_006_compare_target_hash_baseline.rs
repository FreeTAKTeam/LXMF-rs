fn compare_target_hash_baseline(
    path: &Path,
    spec_hash: &str,
    target_hashes: &BTreeMap<String, String>,
) -> Result<()> {
    let baseline = load_generation_baseline(path)?;
    if baseline.spec_hash != spec_hash {
        bail!(
            "generated OpenAPI spec hash mismatch for baseline {}, got {}, expected {}",
            path.display(),
            spec_hash,
            baseline.spec_hash
        );
    }

    if baseline.target_hashes.len() != target_hashes.len() {
        bail!(
            "target hash baseline mismatch for {}: expected {} targets, got {}",
            path.display(),
            baseline.target_hashes.len(),
            target_hashes.len()
        );
    }

    let mut mismatched = Vec::new();
    for (language, expected_hash) in &baseline.target_hashes {
        let Some(got_hash) = target_hashes.get(language) else {
            mismatched.push(format!("{language}:missing-output"));
            continue;
        };
        if got_hash != expected_hash {
            mismatched.push(format!("{language}:{expected_hash}->{got_hash}"));
        }
    }
    for language in target_hashes.keys() {
        if !baseline.target_hashes.contains_key(language) {
            mismatched.push(format!("{language}:new-output"));
        }
    }

    if !mismatched.is_empty() {
        bail!(
            "target output hash mismatch for baseline {}: {}",
            path.display(),
            mismatched.join(", ")
        );
    }

    Ok(())
}

fn write_generation_baseline(
    path: &Path,
    spec_hash: &str,
    target_hashes: &BTreeMap<String, String>,
) -> Result<()> {
    let baseline = SchemaClientGenerationBaseline {
        version: SCHEMA_CLIENT_GENERATION_BASELINE_VERSION,
        spec_hash: spec_hash.to_string(),
        target_hashes: target_hashes.clone(),
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create baseline directory {}", parent.display()))?;
    }
    let serialized = serde_json::to_string_pretty(&baseline)
        .context("serialize schema client generation baseline")?;
    fs::write(path, format!("{serialized}\n"))
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn load_generation_baseline(path: &Path) -> Result<SchemaClientGenerationBaseline> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("read schema client generation baseline {}", path.display()))?;
    let baseline: SchemaClientGenerationBaseline =
        serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;

    if baseline.version != SCHEMA_CLIENT_GENERATION_BASELINE_VERSION {
        bail!(
            "unsupported schema client generation baseline version {} in {}; expected {}",
            baseline.version,
            path.display(),
            SCHEMA_CLIENT_GENERATION_BASELINE_VERSION
        );
    }

    Ok(baseline)
}

fn validate_smoke_coverage(
    workspace: &Path,
    methods: &[MethodDescriptor],
    manifest: &ClientGenerationManifest,
) -> Result<usize> {
    let smoke_path = workspace.join("docs/schemas/sdk/v2/clients/smoke-requests.json");
    if !smoke_path.is_file() {
        bail!("missing smoke vectors at {}", smoke_path.display());
    }

    let raw = fs::read_to_string(&smoke_path)
        .with_context(|| format!("read smoke vectors {}", smoke_path.display()))?;
    let parsed: Value = serde_json::from_str(&raw)
        .with_context(|| format!("parse smoke vectors {}", smoke_path.display()))?;

    let vectors = parsed
        .get("smoke_vectors")
        .and_then(Value::as_array)
        .context("smoke vectors file missing smoke_vectors")?;

    let discovered: BTreeSet<_> = methods.iter().map(|method| method.method.clone()).collect();
    let mut smoke_methods = BTreeSet::new();
    let mut smoke_languages = BTreeSet::new();
    for vector in vectors {
        let method =
            vector.get("method").and_then(Value::as_str).context("smoke vector missing method")?;
        let request = vector.get("request").context("smoke vector missing request")?;
        let schema = methods
            .iter()
            .find(|m| m.method == method)
            .context("smoke vector references unknown method")?;
        validate_smoke_request(method, request, &schema.params_schema)?;
        smoke_methods.insert(method.to_string());

        let language = vector
            .get("language")
            .and_then(Value::as_str)
            .context(format!("smoke vector for method '{method}' missing language"))?;
        smoke_languages.insert(language.to_string());
        if !manifest.targets.iter().any(|target| target.language == language) {
            bail!(
                "smoke vector references language {language} not declared in manifest for method {method}",
            );
        }
    }

    let mode = manifest
        .method_coverage
        .as_ref()
        .map(|cfg| SchemaDiscoveryMode::parse(&cfg.mode))
        .transpose()?
        .unwrap_or(SchemaDiscoveryMode::RequiredSchemas);
    let allow_missing =
        manifest.method_coverage.as_ref().and_then(|cfg| cfg.allow_missing).unwrap_or(false);

    let missing_coverage = if mode == SchemaDiscoveryMode::RequiredSchemas && !allow_missing {
        let missing: Vec<_> =
            discovered.iter().filter(|method| !smoke_methods.contains(*method)).cloned().collect();
        if !missing.is_empty() {
            bail!("smoke vectors are not covering discovered methods: {:?}", missing,);
        }
        0
    } else {
        let missing: Vec<_> =
            discovered.iter().filter(|method| !smoke_methods.contains(*method)).cloned().collect();
        missing.len()
    };

    if vectors.is_empty() {
        bail!("smoke_vectors must not be empty");
    }

    let missing_target_coverage = manifest
        .targets
        .iter()
        .filter(|target| !smoke_languages.contains(target.language.as_str()))
        .collect::<Vec<_>>();
    if !missing_target_coverage.is_empty() {
        let missing = missing_target_coverage
            .iter()
            .map(|target| target.language.as_str())
            .collect::<Vec<_>>();
        bail!("smoke vectors do not cover target languages: {}", missing.join(", "));
    }

    Ok(missing_coverage)
}

fn validate_smoke_request(method: &str, request: &Value, params_schema: &Value) -> Result<()> {
    let request =
        request.as_object().context(format!("smoke vector request for {method} must be object"))?;
    let schema_props = params_schema
        .get("properties")
        .and_then(Value::as_object)
        .context(format!("params schema for {method} missing properties"))?;
    let required = params_schema
        .get("required")
        .and_then(Value::as_array)
        .map_or(&[] as &[Value], |required| required.as_slice())
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();

    for field in required {
        if !request.contains_key(&field) {
            bail!("smoke request for {method} missing required field {field}");
        }
    }

    let additional_properties = params_schema.get("additionalProperties").unwrap_or(&json!(true));
    let disallow_additional = additional_properties == &json!(false);
    if disallow_additional {
        for key in request.keys() {
            if !schema_props.contains_key(key) {
                bail!(
                    "smoke request for {method} contains unknown field {key} (schema disallows additionalProperties)"
                );
            }
        }
    }

    for (name, value) in request {
        let prop_schema =
            schema_props.get(name).context(format!("unknown field {name} for method {method}"))?;
        validate_json_value(name, value, prop_schema)
            .with_context(|| format!("smoke request field {method}.{name}"))?;
    }

    Ok(())
}

fn validate_json_value(name: &str, value: &Value, schema: &Value) -> Result<()> {
    let schema_type = schema.get("type").and_then(Value::as_str);
    if let Some(value_type) = schema_type {
        if !matches!(
            value_type,
            "object" | "array" | "string" | "number" | "integer" | "boolean" | "null"
        ) {
            return Ok(());
        }
        match value_type {
            "object" if !value.is_object() => bail!("{name} must be object"),
            "array" if !value.is_array() => bail!("{name} must be array"),
            "string" if !value.is_string() => bail!("{name} must be string"),
            "number" if !value.is_number() => bail!("{name} must be number"),
            "integer" if !(value.is_i64() || value.is_u64()) => bail!("{name} must be integer"),
            "boolean" if !value.is_boolean() => bail!("{name} must be boolean"),
            "null" if !value.is_null() => bail!("{name} must be null"),
            _ => {}
        }
    }

    if let Some(variants) = schema.get("type").and_then(Value::as_array) {
        let mut matches = false;
        for variant in variants {
            if let Some(kind) = variant.as_str() {
                let temporary = json!({ "type": kind });
                if validate_json_value(name, value, &temporary).is_ok() {
                    matches = true;
                    break;
                }
            }
        }
        if !matches {
            bail!("{name} has wrong type");
        }
    }

    Ok(())
}

fn run_generators(
    workspace: &Path,
    manifest: &ClientGenerationManifest,
    runtime: &GeneratorRuntimeConfig,
    spec_path: &Path,
    openapi_version: &str,
    scratch_dir: &Path,
    run_options: GeneratorRunOptions,
) -> Result<(BTreeMap<String, String>, BTreeMap<String, PathBuf>)> {
    let converted_spec_path = scratch_dir.join("openapi-generator.json");
    let (generator_spec_path, generator_openapi_version) =
        prepare_generator_openapi_spec(spec_path, openapi_version, &converted_spec_path)?;
    let mut target_hashes = BTreeMap::new();
    let mut generated_output_paths = BTreeMap::new();

    for target in &manifest.targets {
        let generator = target
            .generator
            .clone()
            .or_else(|| map_generator_from_language(&target.language))
            .context(format!("missing generator for language {}", target.language))?;

        let generated_dir = scratch_dir.join(&target.language);
        if generated_dir.exists() {
            fs::remove_dir_all(&generated_dir).with_context(|| {
                format!("clear temporary generated output {}", generated_dir.display())
            })?;
        }

        run_openapi_generator(
            workspace,
            runtime,
            &generator,
            &generator_spec_path,
            target,
            &generated_dir,
            &generator_openapi_version,
        )?;

        let normalized = normalize_generated_output(&generated_dir, target)?;
        let output_dir = workspace.join(&target.output_dir);
        let generated_hash = directory_hash(&normalized)?;
        target_hashes.insert(target.language.clone(), generated_hash.clone());
        generated_output_paths.insert(target.language.clone(), normalized.clone());

        match (run_options.mode, run_options.validation_mode) {
            (SchemaClientMode::Check, OutputValidationMode::CommittedArtifacts) => {
                compare_dirs(&normalized, &output_dir)?;
            }
            (SchemaClientMode::Check, OutputValidationMode::TargetHashes) => {
                if output_dir.is_dir() {
                    let committed_hash = directory_hash(&output_dir)?;
                    if committed_hash != generated_hash {
                        bail!(
                        "generated target output mismatch for {}: generated hash {generated_hash}, committed hash {committed_hash}",
                        target.language
                    );
                    }
                }
            }
            (SchemaClientMode::Write, _) => {
                sync_dirs(&normalized, &output_dir)?;
            }
        }
    }

    Ok((target_hashes, generated_output_paths))
}

fn normalize_generated_output(generated_dir: &Path, target: &TargetConfig) -> Result<PathBuf> {
    match target.output_style.as_deref().unwrap_or("multi_file") {
        "multi_file" => Ok(generated_dir.to_path_buf()),
        "single_file" => {
            let files = collect_files_recursive(generated_dir)?;
            if files.is_empty() {
                bail!("no generated files for language {}", target.language);
            }

            let mut parts = Vec::new();
            let mut sorted =
                files.into_iter().filter(|path| path.file_name().is_some()).collect::<Vec<_>>();
            sorted.sort_unstable();

            for path in &sorted {
                let rel = path.strip_prefix(generated_dir).unwrap_or(path);
                let body = fs::read_to_string(path)
                    .with_context(|| format!("read generated file {}", path.display()))?;
                parts.push(format!("// BEGIN {}\n{}\n", rel.display(), body));
            }

            let normalized = generated_dir.join(&target.entrypoint);
            if let Some(parent) = normalized.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("create path {}", normalized.display()))?;
            }
            fs::write(&normalized, parts.join("\n")).with_context(|| {
                format!("write merged generated output {}", normalized.display())
            })?;

            for entry in fs::read_dir(generated_dir)? {
                let entry = entry?;
                let entry_path = entry.path();
                if entry_path == normalized {
                    continue;
                }
                if entry_path.is_dir() {
                    fs::remove_dir_all(&entry_path)?;
                } else {
                    fs::remove_file(&entry_path)?;
                }
            }

            Ok(generated_dir.to_path_buf())
        }
        style => bail!("unsupported output_style {}", style),
    }
}

fn compile_generated_targets(
    workspace: &Path,
    targets: &[TargetConfig],
    validation_mode: OutputValidationMode,
    generated_output_paths: &BTreeMap<String, PathBuf>,
) -> Result<BTreeMap<String, String>> {
    let mut checks = BTreeMap::new();
    for target in targets {
        let output_dir = match validation_mode {
            OutputValidationMode::CommittedArtifacts => workspace.join(&target.output_dir),
            OutputValidationMode::TargetHashes => generated_output_paths
                .get(&target.language)
                .cloned()
                .with_context(|| format!("missing generated output for {}", target.language))?,
        };
        let status = match target.language.as_str() {
            "go" => run_go_compile_check(&output_dir)?,
            "python" => run_python_compile_check(&output_dir)?,
            "javascript" => run_typescript_compile_skip(),
            "typescript" => run_typescript_compile_skip(),
            _ => format!("{COMPILER_CHECK_SKIP_PREFIX} unsupported language"),
        };
        checks.insert(target.language.clone(), status);
    }

    Ok(checks)
}
