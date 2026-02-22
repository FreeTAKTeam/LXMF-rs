use anyhow::{anyhow, bail, Context, Result};
use globset::{Glob, GlobSetBuilder};
use jsonschema::{Draft, JSONSchema};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const DEFAULT_OPENAPI_VERSION: &str = "3.1.0";
const DEFAULT_OPENAPI_SPEC_FILE: &str = "generated/clients/spec/openapi.json";
const DEFAULT_SPEC_HASH_FILE: &str = "target/schema-client/spec.hash";
const DEFAULT_OUTPUT_VALIDATION_MODE: &str = "target_hashes";
const DEFAULT_TARGET_HASH_BASELINE_PATH: &str =
    "docs/contracts/baselines/schema-client-generation-baseline.json";
const SCHEMA_CLIENT_GENERATION_BASELINE_VERSION: u32 = 1;
const COMPILER_CHECK_PASS: &str = "PASS";
const COMPILER_CHECK_SKIP_PREFIX: &str = "SKIP:";
const CLIENT_GENERATION_MANIFEST_SCHEMA_PATH: &str =
    "docs/schemas/sdk/v2/clients/client-generation-manifest.schema.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaClientMode {
    Check,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SchemaDiscoveryMode {
    RequiredSchemas,
    ManifestOnly,
}

impl SchemaDiscoveryMode {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "required_schemas" => Ok(Self::RequiredSchemas),
            "manifest_only" => Ok(Self::ManifestOnly),
            _ => bail!("unsupported mode '{value}'"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputValidationMode {
    CommittedArtifacts,
    TargetHashes,
}

impl OutputValidationMode {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "committed_artifacts" => Ok(Self::CommittedArtifacts),
            "target_hashes" => Ok(Self::TargetHashes),
            _ => bail!("unsupported output validation mode '{value}'"),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct OutputValidationConfig {
    mode: String,
    target_hash_file: Option<String>,
}

impl Default for OutputValidationConfig {
    fn default() -> Self {
        Self {
            mode: DEFAULT_OUTPUT_VALIDATION_MODE.to_string(),
            target_hash_file: Some(DEFAULT_TARGET_HASH_BASELINE_PATH.to_string()),
        }
    }
}

impl OutputValidationConfig {
    fn normalized(&self) -> Result<(OutputValidationMode, PathBuf)> {
        let mode = OutputValidationMode::parse(&self.mode)?;
        let baseline_path = match self.target_hash_file.as_deref() {
            Some(path) if !path.trim().is_empty() => PathBuf::from(path),
            _ => PathBuf::from(DEFAULT_TARGET_HASH_BASELINE_PATH),
        };
        Ok((mode, baseline_path))
    }
}

#[derive(Debug, Clone, Deserialize)]
struct SchemaDiscoveryConfig {
    mode: String,
    include_globs: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
struct MethodCoverageConfig {
    mode: String,
    allow_missing: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
struct GeneratorBackendConfig {
    pub name: String,
    #[serde(default = "default_openapi_version")]
    pub openapi_version: String,
}

fn default_openapi_version() -> String {
    DEFAULT_OPENAPI_VERSION.to_string()
}

#[derive(Debug, Clone, Deserialize)]
struct GeneratorRuntimeConfig {
    #[serde(rename = "type")]
    pub runtime_type: String,
    pub image: String,
    pub command: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct TargetConfig {
    pub language: String,
    pub output_dir: String,
    pub entrypoint: String,
    pub generator: Option<String>,
    pub generator_config_file: Option<String>,
    pub output_style: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ClientGenerationManifest {
    pub version: u32,
    pub contract_release: String,
    pub schema_namespace: String,
    #[serde(default)]
    pub generator_backend: Option<GeneratorBackendConfig>,
    #[serde(default = "default_openapi_spec")]
    pub openapi_spec_file: String,
    #[serde(default)]
    pub schema_discovery: Option<SchemaDiscoveryConfig>,
    #[serde(default)]
    pub method_coverage: Option<MethodCoverageConfig>,
    #[serde(default)]
    pub output_validation: Option<OutputValidationConfig>,
    #[serde(default)]
    pub generator_runtime: Option<GeneratorRuntimeConfig>,
    pub targets: Vec<TargetConfig>,
    pub required_schemas: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SchemaClientGenerationBaseline {
    version: u32,
    spec_hash: String,
    target_hashes: BTreeMap<String, String>,
}

fn default_openapi_spec() -> String {
    DEFAULT_OPENAPI_SPEC_FILE.to_string()
}

#[derive(Debug, Clone)]
struct MethodDescriptor {
    pub method: String,
    pub params_schema: Value,
    pub result_schema: Value,
    pub source_path: PathBuf,
}

#[derive(Debug, Clone)]
struct SchemaSource {
    path: PathBuf,
    schema: Value,
    def_component_prefix: String,
}

#[derive(Debug, Clone)]
pub struct SchemaClientReport {
    pub manifest_path: PathBuf,
    pub spec_path: PathBuf,
    pub method_count: usize,
    pub methods: Vec<String>,
    pub spec_hash: String,
    pub target_hashes: BTreeMap<String, String>,
    pub target_compile_checks: BTreeMap<String, String>,
    pub missing_smoke_count: usize,
}

#[derive(Debug, Serialize)]
struct OpenApiSpec {
    openapi: String,
    info: OpenApiInfo,
    paths: BTreeMap<String, OpenApiPathItem>,
    components: OpenApiComponents,
}

#[derive(Debug, Serialize)]
struct OpenApiInfo {
    title: String,
    version: String,
}

#[derive(Debug, Serialize)]
struct OpenApiComponents {
    schemas: BTreeMap<String, Value>,
}

#[derive(Debug, Serialize)]
struct OpenApiPathItem {
    post: OpenApiOperation,
}

#[derive(Debug, Serialize)]
struct OpenApiOperation {
    #[serde(rename = "operationId")]
    operation_id: String,
    #[serde(rename = "x-jsonrpc-methods")]
    jsonrpc_methods: Vec<String>,
    #[serde(rename = "x-jsonrpc-method")]
    jsonrpc_method: Vec<String>,
    #[serde(rename = "requestBody")]
    request_body: OpenApiRequestBody,
    responses: BTreeMap<String, OpenApiResponse>,
}

#[derive(Debug, Serialize)]
struct OpenApiRequestBody {
    required: bool,
    content: BTreeMap<String, OpenApiMediaType>,
}

#[derive(Debug, Serialize)]
struct OpenApiResponse {
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<BTreeMap<String, OpenApiMediaType>>,
}

#[derive(Debug, Serialize)]
struct OpenApiMediaType {
    schema: Value,
}

pub fn run_schema_client_generate(
    workspace: &Path,
    manifest_path: &Path,
    mode: SchemaClientMode,
) -> Result<SchemaClientReport> {
    let manifest = load_and_validate_manifest(manifest_path)?;
    let schema_sources = discover_schema_sources(workspace, &manifest)?;
    let methods = discover_methods(&schema_sources)?;
    let spec_path = workspace.join(&manifest.openapi_spec_file);
    let temp_dir = TempDir::new(workspace)?;
    let spec_to_use = if mode == SchemaClientMode::Check {
        temp_dir.path.join("openapi-check.json")
    } else {
        spec_path.clone()
    };
    let openapi_version = manifest
        .generator_backend
        .as_ref()
        .map(|backend| backend.openapi_version.clone())
        .unwrap_or_else(default_openapi_version);
    let (validation_mode, target_hash_baseline_path) = manifest
        .output_validation
        .as_ref()
        .unwrap_or(&OutputValidationConfig::default())
        .normalized()?;

    let spec_hash = generate_openapi_spec(&manifest, &schema_sources, &methods, &spec_to_use)?;
    validate_openapi_references(&spec_to_use)?;
    if mode == SchemaClientMode::Check {
        match validation_mode {
            OutputValidationMode::CommittedArtifacts => {
                if !spec_path.is_file() {
                    bail!("missing committed OpenAPI spec {} in check mode", spec_path.display());
                }
                compare_file_bytes(&spec_path, &spec_to_use)?;
            }
            OutputValidationMode::TargetHashes => {
                compare_spec_hash_to_baseline(&target_hash_baseline_path, &spec_hash)?;
            }
        }
    }
    let missing_smoke_count = validate_smoke_coverage(workspace, &methods, &manifest)?;

    let runtime =
        manifest.generator_runtime.as_ref().context("manifest missing generator_runtime")?;
    let (target_hashes, generated_output_paths) = run_generators(
        workspace,
        &manifest,
        runtime,
        &spec_to_use,
        &openapi_version,
        &temp_dir.path,
        mode,
        validation_mode,
    )?;

    let mut target_compile_checks = BTreeMap::new();
    if mode == SchemaClientMode::Check {
        target_compile_checks = compile_generated_targets(
            workspace,
            &manifest.targets,
            validation_mode,
            &generated_output_paths,
        )?;
    }

    if mode == SchemaClientMode::Check && validation_mode == OutputValidationMode::TargetHashes {
        compare_target_hash_baseline(&target_hash_baseline_path, &spec_hash, &target_hashes)?;
    }

    if mode == SchemaClientMode::Write {
        if validation_mode == OutputValidationMode::TargetHashes {
            write_generation_baseline(&target_hash_baseline_path, &spec_hash, &target_hashes)?;
        }
    }

    let mut method_names = methods.iter().map(|m| m.method.clone()).collect::<Vec<_>>();
    method_names.sort_unstable();

    write_spec_hash_file(workspace, &spec_hash)?;

    Ok(SchemaClientReport {
        manifest_path: manifest_path.to_path_buf(),
        spec_path,
        method_count: method_names.len(),
        methods: method_names,
        spec_hash,
        target_compile_checks,
        target_hashes,
        missing_smoke_count,
    })
}

fn load_and_validate_manifest(manifest_path: &Path) -> Result<ClientGenerationManifest> {
    let raw = fs::read_to_string(manifest_path)
        .with_context(|| format!("read manifest {}", manifest_path.display()))?;
    let manifest_value: Value = serde_json::from_str(&raw)
        .with_context(|| format!("parse manifest {}", manifest_path.display()))?;
    validate_manifest_schema(manifest_path, &manifest_value)?;

    let manifest: ClientGenerationManifest = serde_json::from_value(manifest_value)
        .with_context(|| format!("parse manifest {}", manifest_path.display()))?;

    if manifest.targets.is_empty() {
        bail!("manifest.targets must not be empty");
    }

    let mut languages = BTreeSet::new();
    for target in &manifest.targets {
        if target.language.trim().is_empty() {
            bail!("manifest target language cannot be empty");
        }
        if target.output_dir.trim().is_empty() {
            bail!("manifest target output_dir for {} cannot be empty", target.language);
        }
        if target.entrypoint.trim().is_empty() {
            bail!("manifest target entrypoint for {} cannot be empty", target.language);
        }
        if !languages.insert(target.language.clone()) {
            bail!("manifest contains duplicate target language {}", target.language);
        }
        match target.output_style.as_deref() {
            None | Some("multi_file") | Some("single_file") => {}
            Some(style) => bail!("unsupported output_style {} for {}", style, target.language),
        }
    }

    if manifest.required_schemas.is_empty() {
        bail!("manifest.required_schemas must not be empty");
    }

    let runtime =
        manifest.generator_runtime.as_ref().context("manifest missing generator_runtime")?;

    if runtime.image.trim().is_empty() {
        bail!("generator_runtime.image must be set");
    }
    if runtime.runtime_type != "docker" && runtime.runtime_type != "local" {
        bail!("unsupported generator_runtime.type {}", runtime.runtime_type);
    }
    if runtime.runtime_type == "local" && runtime.command.as_deref().map_or(true, str::is_empty) {
        bail!("generator_runtime.command is required when generator_runtime.type is local");
    }

    let backend = manifest
        .generator_backend
        .as_ref()
        .map(|backend| backend.name.as_str())
        .unwrap_or("openapi");
    if backend != "openapi" {
        bail!("unsupported generator_backend {}", backend);
    }

    let discovery = manifest
        .schema_discovery
        .as_ref()
        .map(|cfg| SchemaDiscoveryMode::parse(&cfg.mode))
        .transpose()?
        .unwrap_or(SchemaDiscoveryMode::RequiredSchemas);

    if discovery == SchemaDiscoveryMode::ManifestOnly {
        let include_count = manifest
            .schema_discovery
            .as_ref()
            .and_then(|cfg| cfg.include_globs.as_ref())
            .map_or(0usize, Vec::len);
        if include_count == 0 {
            bail!("schema_discovery.include_globs is required when schema_discovery.mode=manifest_only");
        }
    }

    let _ = manifest
        .method_coverage
        .as_ref()
        .map(|cfg| SchemaDiscoveryMode::parse(&cfg.mode))
        .transpose()?
        .unwrap_or(SchemaDiscoveryMode::RequiredSchemas);

    let _ = manifest
        .output_validation
        .as_ref()
        .unwrap_or(&OutputValidationConfig::default())
        .normalized()?;

    Ok(manifest)
}

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
    let schema_paths = resolve_schema_paths(workspace, manifest)?;
    let mut sources = Vec::new();

    for path in schema_paths {
        let raw =
            fs::read_to_string(&path).with_context(|| format!("read schema {}", path.display()))?;
        let schema = serde_json::from_str::<Value>(&raw)
            .with_context(|| format!("parse schema {}", path.display()))?;
        sources.push(SchemaSource {
            path: path.clone(),
            schema,
            def_component_prefix: schema_source_prefix(&path),
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

fn extract_methods_from_schema(
    schema: &Value,
    source_name: &Path,
) -> Result<Vec<MethodDescriptor>> {
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
        "RPCError".to_string(),
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
    response_variants.push("#/components/schemas/RPCError".to_string());
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
    for (_name, schema) in spec.components.schemas.iter_mut() {
        sanitize_component_self_references(schema);
    }

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
            "object" => {
                if !value.is_object() {
                    bail!("{name} must be object");
                }
            }
            "array" => {
                if !value.is_array() {
                    bail!("{name} must be array");
                }
            }
            "string" => {
                if !value.is_string() {
                    bail!("{name} must be string");
                }
            }
            "number" => {
                if !value.is_number() {
                    bail!("{name} must be number");
                }
            }
            "integer" => {
                if !(value.is_i64() || value.is_u64()) {
                    bail!("{name} must be integer");
                }
            }
            "boolean" => {
                if !value.is_boolean() {
                    bail!("{name} must be boolean");
                }
            }
            "null" => {
                if !value.is_null() {
                    bail!("{name} must be null");
                }
            }
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
    mode: SchemaClientMode,
    validation_mode: OutputValidationMode,
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

        match (mode, validation_mode) {
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

fn run_go_compile_check(output_dir: &Path) -> Result<String> {
    if command_exists("go").is_none() {
        return Ok(format!("{COMPILER_CHECK_SKIP_PREFIX} go command not available"));
    }
    if !output_dir.exists() {
        return Ok(format!("{COMPILER_CHECK_SKIP_PREFIX} missing generated go output"));
    }

    let temp_parent = output_dir.parent().context("generated go output has no parent directory")?;
    let temp_dir = TempDir::new(temp_parent)?;
    let sandbox = temp_dir.path.join("go-compile-check");
    fs::create_dir_all(&sandbox).with_context(|| format!("create {}", sandbox.display()))?;
    copy_dir_recursively(output_dir, &sandbox)?;
    let go_mod = sandbox.join("go.mod");

    if !go_mod.exists() {
        let status = Command::new("go")
            .current_dir(&sandbox)
            .args(["mod", "init", "example.com/lxmfclient"])
            .status()
            .with_context(|| format!("spawn go mod init in {}", sandbox.display()))?;
        if !status.success() {
            return Ok(format!("FAIL: go mod init failed for {}", output_dir.display()));
        }
    }

    let status = Command::new("go")
        .current_dir(&sandbox)
        .args(["mod", "tidy"])
        .status()
        .with_context(|| format!("spawn go mod tidy in {}", sandbox.display()))?;
    if !status.success() {
        return Ok(format!("FAIL: go mod tidy failed for {}", output_dir.display()));
    }

    let status = Command::new("go")
        .current_dir(&sandbox)
        .args(["test", "./..."])
        .status()
        .with_context(|| format!("spawn go test in {}", sandbox.display()))?;
    if !status.success() {
        return Ok(format!("FAIL: go test failed for {}", output_dir.display()));
    }

    Ok(COMPILER_CHECK_PASS.to_string())
}

fn run_python_compile_check(output_dir: &Path) -> Result<String> {
    let python = if command_exists("python3").is_some() {
        "python3"
    } else if command_exists("python").is_some() {
        "python"
    } else {
        return Ok(format!("{COMPILER_CHECK_SKIP_PREFIX} python command not available"));
    };

    if !output_dir.exists() {
        return Ok(format!("{COMPILER_CHECK_SKIP_PREFIX} missing generated python output"));
    }
    let mut files = Vec::new();
    for path in collect_files_recursive(output_dir)? {
        let rel = path
            .strip_prefix(output_dir)
            .with_context(|| format!("compute relative python file path for {}", path.display()))?;
        if rel.extension().and_then(|value| value.to_str()) == Some("py") {
            files.push(rel.to_string_lossy().to_string());
        }
    }
    if files.is_empty() {
        return Ok(format!("{COMPILER_CHECK_SKIP_PREFIX} no python files"));
    }

    files.sort_unstable();

    let mut args = vec![
        "-c",
        r#"import sys

for path in sys.argv[1:]:
    try:
        with open(path, "rb") as handle:
            source = handle.read()
        compile(source, path, "exec")
    except (OSError, SyntaxError) as error:
        print(f"{path}: {error}")
        raise SystemExit(1)
"#,
    ];
    args.extend(files.iter().map(String::as_str));

    let status = Command::new(python)
        .current_dir(output_dir)
        .args(args)
        .status()
        .with_context(|| format!("spawn {python}"))?;
    if !status.success() {
        return Ok(format!("FAIL: python compile failed for {}", output_dir.display()));
    }

    Ok(COMPILER_CHECK_PASS.to_string())
}

fn run_typescript_compile_skip() -> String {
    format!("{COMPILER_CHECK_SKIP_PREFIX} tsc check not configured")
}

fn prepare_generator_openapi_spec(
    spec_path: &Path,
    openapi_version: &str,
    converted_path: &Path,
) -> Result<(PathBuf, String)> {
    if !openapi_version.starts_with("3.1") {
        return Ok((spec_path.to_path_buf(), openapi_version.to_string()));
    }

    let raw =
        fs::read_to_string(spec_path).with_context(|| format!("read {}", spec_path.display()))?;
    let spec = serde_json::from_str::<Value>(&raw)
        .with_context(|| format!("parse {}", spec_path.display()))?;
    let converted = convert_openapi_spec_for_generator(&spec)?;

    if let Some(parent) = converted_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(converted_path, serde_json::to_vec_pretty(&converted)?)
        .with_context(|| format!("write {}", converted_path.display()))?;

    Ok((converted_path.to_path_buf(), "3.0.3".to_string()))
}

fn convert_openapi_spec_for_generator(spec: &Value) -> Result<Value> {
    match spec {
        Value::Object(source) => {
            let mut out = Map::new();
            for (key, value) in source {
                match key.as_str() {
                    "openapi" => {
                        out.insert(key.clone(), json!("3.0.3"));
                    }
                    "$schema" | "$id" | "$defs" => {}
                    _ => {
                        out.insert(key.clone(), transform_schema_node_for_generator(value)?);
                    }
                }
            }
            Ok(Value::Object(out))
        }
        _ => Ok(spec.clone()),
    }
}

fn transform_schema_node_for_generator(value: &Value) -> Result<Value> {
    match value {
        Value::Array(values) => {
            let normalized = values
                .iter()
                .map(transform_schema_node_for_generator)
                .collect::<Result<Vec<_>>>()?;
            Ok(Value::Array(normalized))
        }
        Value::Object(map) => {
            let mut out = Map::new();

            let nullable = if let Some(types) = map.get("type").and_then(Value::as_array) {
                type_with_nullable(types)?
            } else {
                None
            };

            for (key, node) in map {
                if key == "type" {
                    if let Some((base_type, nullable)) = &nullable {
                        out.insert("type".to_string(), Value::String(base_type.to_string()));
                        if *nullable {
                            out.insert("nullable".to_string(), Value::Bool(true));
                        }
                    } else {
                        out.insert(key.clone(), transform_schema_node_for_generator(node)?);
                    }
                    continue;
                }

                if key == "const" {
                    out.insert("enum".to_string(), Value::Array(vec![node.clone()]));
                    continue;
                }

                if key == "$schema" || key == "$id" {
                    continue;
                }
                if key == "propertyNames" {
                    continue;
                }

                let transformed = transform_schema_node_for_generator(node)?;
                out.insert(key.clone(), transformed);
            }

            Ok(Value::Object(out))
        }
        _ => Ok(value.clone()),
    }
}

fn type_with_nullable(type_list: &[Value]) -> Result<Option<(String, bool)>> {
    let has_null = type_list.iter().any(|value| value == "null");
    let mut seen = Vec::new();
    for value in type_list {
        let kind =
            value.as_str().context("type array entries must be strings in type conversion")?;
        if kind != "null" {
            seen.push(kind.to_string());
        }
    }

    match seen.as_slice() {
        [] => Ok(None),
        [kind] => Ok(Some((kind.to_string(), has_null))),
        _ if has_null => Ok(None),
        _ => Ok(None),
    }
}

fn run_openapi_generator(
    workspace: &Path,
    runtime: &GeneratorRuntimeConfig,
    generator: &str,
    spec_path: &Path,
    target: &TargetConfig,
    output_dir: &Path,
    openapi_version: &str,
) -> Result<()> {
    fs::create_dir_all(output_dir)
        .with_context(|| format!("create generator output {}", output_dir.display()))?;
    let workspace = workspace.canonicalize().context("canonicalize workspace")?;

    match runtime.runtime_type.as_str() {
        "local" => {
            let mut command_parts = runtime
                .command
                .as_deref()
                .unwrap_or("openapi-generator-cli")
                .split_whitespace()
                .collect::<Vec<_>>();

            let command_program = command_parts.first().copied().unwrap_or("openapi-generator-cli");
            let command_args = command_parts.split_off(1);

            let mut args = Vec::new();
            args.extend(command_args.into_iter().map(ToString::to_string));
            args.extend(vec![
                "generate".to_string(),
                "-i".to_string(),
                spec_path
                    .canonicalize()
                    .with_context(|| format!("canonicalize {}", spec_path.display()))?
                    .to_string_lossy()
                    .to_string(),
                "-g".to_string(),
                generator.to_string(),
                "-o".to_string(),
                output_dir
                    .canonicalize()
                    .with_context(|| format!("canonicalize {}", output_dir.display()))?
                    .to_string_lossy()
                    .to_string(),
            ]);
            if openapi_version.starts_with("3.1") {
                args.push("--skip-validate-spec".to_string());
            }
            if let Some(config_file) = &target.generator_config_file {
                let abs = workspace.join(config_file);
                if !abs.is_file() {
                    bail!("missing generator config file {}", abs.display());
                }
                args.push("-c".to_string());
                args.push(abs.to_string_lossy().to_string());
            }

            run_command(command_program, &args.iter().map(String::as_str).collect::<Vec<_>>())?;
        }
        "docker" => {
            if command_exists("docker").is_none() {
                bail!("docker is required for generator runtime type docker");
            }

            let spec_path = spec_path
                .canonicalize()
                .with_context(|| format!("canonicalize {}", spec_path.display()))?;
            let output_dir = output_dir
                .canonicalize()
                .with_context(|| format!("canonicalize {}", output_dir.display()))?;
            let spec_rel = spec_path
                .strip_prefix(&workspace)
                .with_context(|| format!("spec path {} outside workspace", spec_path.display()))?
                .to_string_lossy()
                .to_string()
                .replace('\\', "/");
            let out_rel = output_dir
                .strip_prefix(&workspace)
                .with_context(|| format!("output path {} outside workspace", output_dir.display()))?
                .to_string_lossy()
                .to_string()
                .replace('\\', "/");

            let mut args = vec![
                "run".to_string(),
                "--rm".to_string(),
                "-v".to_string(),
                format!("{}:/local", workspace.display()),
                runtime.image.clone(),
                "generate".to_string(),
                "-i".to_string(),
                format!("/local/{spec_rel}"),
                "-g".to_string(),
                generator.to_string(),
                "-o".to_string(),
                format!("/local/{out_rel}"),
            ];
            if openapi_version.starts_with("3.1") {
                args.push("--skip-validate-spec".to_string());
            }
            if let Some(config_file) = &target.generator_config_file {
                let full = workspace.join(config_file);
                let rel = full
                    .strip_prefix(&workspace)
                    .with_context(|| format!("config path {} outside workspace", full.display()))?
                    .to_string_lossy()
                    .to_string()
                    .replace('\\', "/");
                if !full.is_file() {
                    bail!("missing generator config file {}", full.display());
                }
                args.push("-c".to_string());
                args.push(format!("/local/{rel}"));
            }

            let extra = runtime.command.as_deref().unwrap_or("");
            if !extra.trim().is_empty() {
                args.extend(extra.split_whitespace().map(ToString::to_string));
            }

            run_command("docker", &args.iter().map(String::as_str).collect::<Vec<_>>())?;
        }
        value => bail!("unsupported generator runtime type {}", value),
    }

    Ok(())
}

fn run_command(cmd: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(cmd).args(args).status().with_context(|| format!("spawn {cmd}"))?;
    if !status.success() {
        bail!("command failed: {} {}", cmd, args.join(" "));
    }
    Ok(())
}

fn collect_files_recursive(path: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(path).with_context(|| format!("read_dir {}", path.display()))? {
        let entry = entry.with_context(|| format!("read entry in {}", path.display()))?;
        let item = entry.path();
        if item.is_dir() {
            out.extend(collect_files_recursive(&item)?);
            continue;
        }
        if item.is_file() {
            out.push(item);
        }
    }
    Ok(out)
}

fn compare_dirs(expected: &Path, actual: &Path) -> Result<()> {
    if !actual.is_dir() {
        bail!("generated target missing {}", actual.display());
    }

    let expected_map = collect_file_hashes(expected)?;
    let actual_map = collect_file_hashes(actual)?;

    if expected_map == actual_map {
        return Ok(());
    }

    let missing =
        expected_map.keys().filter(|path| !actual_map.contains_key(*path)).collect::<Vec<_>>();
    let extra =
        actual_map.keys().filter(|path| !expected_map.contains_key(*path)).collect::<Vec<_>>();

    if !missing.is_empty() || !extra.is_empty() {
        bail!("output mismatch: missing {:?}, extra {:?}", missing, extra);
    }

    let mut mismatched = Vec::new();
    for (path, expected_hash) in expected_map {
        let actual_hash = actual_map.get(&path).context(format!("missing hash for {path}"))?;
        if expected_hash != *actual_hash {
            mismatched.push(path);
        }
    }

    if !mismatched.is_empty() {
        bail!("generated output drift: {:?}", mismatched);
    }

    Ok(())
}

fn sync_dirs(src: &Path, dst: &Path) -> Result<()> {
    if dst.exists() {
        fs::remove_dir_all(dst).with_context(|| format!("remove {}", dst.display()))?;
    }
    fs::create_dir_all(dst).with_context(|| format!("create {}", dst.display()))?;
    copy_dir_recursively(src, dst)
}

fn copy_dir_recursively(src: &Path, dst: &Path) -> Result<()> {
    for entry in fs::read_dir(src).with_context(|| format!("read_dir {}", src.display()))? {
        let entry = entry.with_context(|| format!("read entry in {}", src.display()))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            fs::create_dir_all(&dst_path)
                .with_context(|| format!("create dir {}", dst_path.display()))?;
            copy_dir_recursively(&src_path, &dst_path)?;
        } else if src_path.is_file() {
            fs::copy(&src_path, &dst_path)
                .with_context(|| format!("copy {}", src_path.display()))?;
        }
    }
    Ok(())
}

fn collect_file_hashes(dir: &Path) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];

    while let Some(path) = stack.pop() {
        for entry in fs::read_dir(&path).with_context(|| format!("read_dir {}", path.display()))? {
            let entry = entry.with_context(|| format!("read entry {}", path.display()))?;
            let entry_path = entry.path();
            let rel = entry_path
                .strip_prefix(dir)
                .context("strip_prefix")?
                .to_string_lossy()
                .replace('\\', "/");

            if entry_path.is_dir() {
                if rel.ends_with("/__pycache__") || rel == "__pycache__" {
                    continue;
                }
                stack.push(entry_path);
                continue;
            }

            let bytes = fs::read(&entry_path)
                .with_context(|| format!("read file {}", entry_path.display()))?;
            out.insert(rel, sha256_hex(&bytes));
        }
    }

    Ok(out)
}

fn directory_hash(dir: &Path) -> Result<String> {
    let files = collect_file_hashes(dir)?;
    let mut rendered = String::new();
    for (path, hash) in files {
        rendered.push_str(&path);
        rendered.push('\n');
        rendered.push_str(&hash);
        rendered.push('\n');
    }
    Ok(sha256_hex(rendered.as_bytes()))
}

fn write_spec_hash_file(workspace: &Path, hash: &str) -> Result<()> {
    let path = workspace.join(DEFAULT_SPEC_HASH_FILE);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", path.display()))?;
    }
    fs::write(&path, format!("{hash}\n")).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn command_exists(cmd: &str) -> Option<PathBuf> {
    env::var_os("PATH").and_then(|path| {
        env::split_paths(&path)
            .map(|dir| if cfg!(windows) { dir.join(format!("{cmd}.exe")) } else { dir.join(cmd) })
            .find(|candidate| candidate.exists())
    })
}

fn map_generator_from_language(language: &str) -> Option<String> {
    match language {
        "go" => Some("go".to_string()),
        "javascript" => Some("typescript-axios".to_string()),
        "typescript" => Some("typescript-axios".to_string()),
        "python" => Some("python".to_string()),
        _ => None,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn to_pascal_case(input: &str) -> String {
    let mut out = String::new();
    for part in input.split(|c: char| !c.is_ascii_alphanumeric()) {
        if part.is_empty() {
            continue;
        }
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.extend(chars.map(|ch| ch.to_ascii_lowercase()));
        }
    }

    if out.is_empty() {
        return "Method".to_string();
    }

    if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        out.insert(0, '_');
    }

    out
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(base: &Path) -> Result<Self> {
        let mut base_dir = base.join(".tmp").join("schema-client");
        let unique = format!(
            "lxmf-schema-client-generate-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_else(|_| Default::default())
                .as_nanos(),
        );
        base_dir.push(unique);
        fs::create_dir_all(&base_dir)
            .with_context(|| format!("create temp dir {}", base_dir.display()))?;
        Ok(Self { path: base_dir })
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

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
        };
        let source_b = SchemaSource {
            path: Path::new("docs/schemas/sdk/v2/rpc/sdk_send_v2_duplicate.schema.json")
                .to_path_buf(),
            schema,
            def_component_prefix: "SdkSendV2".to_string(),
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
                        },
                        "required": ["method"]
                    },
                    "Payload": {
                        "$id": "urn:example",
                        "$schema": "https://json-schema.org/draft/2020-12/schema",
                        "type": ["string", "null"]
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

        let payload = &converted["components"]["schemas"]["Payload"];
        assert!(payload.get("const").is_none());
        assert_eq!(payload["type"], "string");
        assert_eq!(payload["nullable"], true);

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
}
