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

const LEGACY_RPC_SCHEMA_DIR: &str = "docs/schemas/sdk/v2/rpc";

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GeneratorRunOptions {
    mode: SchemaClientMode,
    validation_mode: OutputValidationMode,
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
    #[serde(default)]
    pub openrpc_contract_file: Option<String>,
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
    kind: SchemaSourceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SchemaSourceKind {
    JsonSchemaDefs,
    OpenRpc,
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

#[derive(Debug, Clone)]
struct LegacyRpcSchemaArtifact {
    path: PathBuf,
    schema: Value,
}

pub fn run_schema_client_generate(
    workspace: &Path,
    manifest_path: &Path,
    mode: SchemaClientMode,
) -> Result<SchemaClientReport> {
    let manifest = load_and_validate_manifest(manifest_path)?;
    let schema_sources = discover_schema_sources(workspace, &manifest)?;
    sync_legacy_rpc_schemas(workspace, &schema_sources, mode)?;
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
    let generator_run_options = GeneratorRunOptions { mode, validation_mode };
    let (target_hashes, generated_output_paths) = run_generators(
        workspace,
        &manifest,
        runtime,
        &spec_to_use,
        &openapi_version,
        &temp_dir.path,
        generator_run_options,
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

    if mode == SchemaClientMode::Write && validation_mode == OutputValidationMode::TargetHashes {
        write_generation_baseline(&target_hash_baseline_path, &spec_hash, &target_hashes)?;
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
    if runtime.runtime_type == "local" && runtime.command.as_deref().is_none_or(str::is_empty) {
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
