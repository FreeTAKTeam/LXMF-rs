use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const INTEROP_BASELINE_PATH: &str = "docs/contracts/baselines/interop-artifacts-manifest.json";
const INTEROP_DRIFT_BASELINE_PATH: &str = "docs/contracts/baselines/interop-drift-baseline.json";
const INTEROP_MATRIX_PATH: &str = "docs/contracts/compatibility-matrix.md";
const INTEROP_CORPUS_PATH: &str = "docs/fixtures/interop/v1/golden-corpus.json";
const RPC_CONTRACT_PATH: &str = "docs/contracts/rpc-contract.md";
const PAYLOAD_CONTRACT_PATH: &str = "docs/contracts/payload-contract.md";

#[derive(Parser)]
#[command(name = "xtask")]
struct Xtask {
    #[command(subcommand)]
    command: XtaskCommand,
}

#[derive(Subcommand)]
enum XtaskCommand {
    Ci {
        #[arg(long)]
        stage: Option<CiStage>,
    },
    ReleaseCheck,
    ApiDiff,
    Licenses,
    MigrationChecks,
    ArchitectureChecks,
    ForbiddenDeps,
    SdkConformance,
    SdkSchemaCheck,
    InteropArtifacts {
        #[arg(long)]
        update: bool,
    },
    InteropMatrixCheck,
    InteropCorpusCheck,
    InteropDriftCheck {
        #[arg(long)]
        update: bool,
    },
    E2eCompatibility,
    MeshSim,
    SdkProfileBuild,
    SdkExamplesCheck,
    SdkApiBreak,
    SdkMigrationCheck,
    SdkSecurityCheck,
    SdkPropertyCheck,
    SdkModelCheck,
    SdkMatrixCheck,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum CiStage {
    LintFormat,
    BuildMatrix,
    TestNextestUnit,
    TestIntegration,
    Doc,
    Security,
    UnusedDeps,
    ApiSurfaceCheck,
    SdkConformance,
    SdkSchemaCheck,
    InteropArtifacts,
    InteropMatrixCheck,
    InteropCorpusCheck,
    InteropDriftCheck,
    E2eCompatibility,
    SdkProfileBuild,
    SdkExamplesCheck,
    SdkApiBreak,
    SdkMigrationCheck,
    SdkSecurityCheck,
    SdkPropertyCheck,
    SdkModelCheck,
    SdkMatrixCheck,
    MigrationChecks,
    ArchitectureChecks,
    ForbiddenDeps,
}

fn main() -> Result<()> {
    let xtask = Xtask::parse();
    match xtask.command {
        XtaskCommand::Ci { stage } => run_ci(stage),
        XtaskCommand::ReleaseCheck => run_release_check(),
        XtaskCommand::ApiDiff => run_api_diff(),
        XtaskCommand::Licenses => run_licenses(),
        XtaskCommand::MigrationChecks => run_migration_checks(),
        XtaskCommand::ArchitectureChecks => run_architecture_checks(),
        XtaskCommand::ForbiddenDeps => run_forbidden_deps(),
        XtaskCommand::SdkConformance => run_sdk_conformance(),
        XtaskCommand::SdkSchemaCheck => run_sdk_schema_check(),
        XtaskCommand::InteropArtifacts { update } => run_interop_artifacts(update),
        XtaskCommand::InteropMatrixCheck => run_interop_matrix_check(),
        XtaskCommand::InteropCorpusCheck => run_interop_corpus_check(),
        XtaskCommand::InteropDriftCheck { update } => run_interop_drift_check(update),
        XtaskCommand::E2eCompatibility => run_e2e_compatibility(),
        XtaskCommand::MeshSim => run_mesh_sim(),
        XtaskCommand::SdkProfileBuild => run_sdk_profile_build(),
        XtaskCommand::SdkExamplesCheck => run_sdk_examples_check(),
        XtaskCommand::SdkApiBreak => run_sdk_api_break(),
        XtaskCommand::SdkMigrationCheck => run_sdk_migration_check(),
        XtaskCommand::SdkSecurityCheck => run_sdk_security_check(),
        XtaskCommand::SdkPropertyCheck => run_sdk_property_check(),
        XtaskCommand::SdkModelCheck => run_sdk_model_check(),
        XtaskCommand::SdkMatrixCheck => run_sdk_matrix_check(),
    }
}

fn run_ci(stage: Option<CiStage>) -> Result<()> {
    if let Some(stage) = stage {
        return run_ci_stage(stage);
    }

    run("cargo", &["fmt", "--all", "--", "--check"])?;
    run(
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--no-deps",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    run("cargo", &["test", "--workspace"])?;
    run("cargo", &["doc", "--workspace", "--no-deps"])?;
    run_sdk_schema_check()?;
    run_interop_artifacts(false)?;
    run_interop_matrix_check()?;
    run_interop_corpus_check()?;
    run_interop_drift_check(false)?;
    run_e2e_compatibility()?;
    run_sdk_conformance()?;
    run_sdk_profile_build()?;
    run_sdk_examples_check()?;
    run_sdk_security_check()?;
    run_sdk_property_check()?;
    run_sdk_model_check()?;
    run_sdk_matrix_check()?;
    run_migration_checks()?;
    run_architecture_checks()?;
    Ok(())
}

fn run_ci_stage(stage: CiStage) -> Result<()> {
    match stage {
        CiStage::LintFormat => run("cargo", &["fmt", "--all", "--", "--check"]),
        CiStage::BuildMatrix => run("cargo", &["build", "--workspace", "--all-targets"]),
        CiStage::TestNextestUnit => {
            run("cargo", &["nextest", "run", "--workspace", "--lib", "--bins"])
        }
        CiStage::TestIntegration => run("cargo", &["test", "--workspace", "--tests"]),
        CiStage::Doc => run("cargo", &["doc", "--workspace", "--no-deps"]),
        CiStage::Security => {
            run("cargo", &["deny", "check"])?;
            run("cargo", &["audit"])
        }
        CiStage::UnusedDeps => run_unused_deps(),
        CiStage::ApiSurfaceCheck => run_api_diff(),
        CiStage::SdkConformance => run_sdk_conformance(),
        CiStage::SdkSchemaCheck => run_sdk_schema_check(),
        CiStage::InteropArtifacts => run_interop_artifacts(false),
        CiStage::InteropMatrixCheck => run_interop_matrix_check(),
        CiStage::InteropCorpusCheck => run_interop_corpus_check(),
        CiStage::InteropDriftCheck => run_interop_drift_check(false),
        CiStage::E2eCompatibility => run_e2e_compatibility(),
        CiStage::SdkProfileBuild => run_sdk_profile_build(),
        CiStage::SdkExamplesCheck => run_sdk_examples_check(),
        CiStage::SdkApiBreak => run_sdk_api_break(),
        CiStage::SdkMigrationCheck => run_sdk_migration_check(),
        CiStage::SdkSecurityCheck => run_sdk_security_check(),
        CiStage::SdkPropertyCheck => run_sdk_property_check(),
        CiStage::SdkModelCheck => run_sdk_model_check(),
        CiStage::SdkMatrixCheck => run_sdk_matrix_check(),
        CiStage::MigrationChecks => run_migration_checks(),
        CiStage::ArchitectureChecks => run_architecture_checks(),
        CiStage::ForbiddenDeps => run_forbidden_deps(),
    }
}

fn run_release_check() -> Result<()> {
    run_ci(None)?;
    run_interop_matrix_check()?;
    run_interop_corpus_check()?;
    run_interop_drift_check(false)?;
    run_sdk_api_break()?;
    run("cargo", &["deny", "check"])?;
    run("cargo", &["audit"])?;
    Ok(())
}

fn run_api_diff() -> Result<()> {
    let toolchain = public_api_toolchain();
    for manifest in [
        "crates/libs/lxmf-core/Cargo.toml",
        "crates/libs/lxmf-sdk/Cargo.toml",
        "crates/libs/rns-core/Cargo.toml",
        "crates/libs/rns-transport/Cargo.toml",
        "crates/libs/rns-rpc/Cargo.toml",
    ] {
        let args = format!("public-api --manifest-path {manifest} -sss --color never");
        let command = toolchain_cargo_command(&toolchain, &args);
        run("bash", &["-lc", &command])?;
    }
    Ok(())
}

fn run_licenses() -> Result<()> {
    run("cargo", &["deny", "check", "licenses"])
}

fn run_sdk_conformance() -> Result<()> {
    run("cargo", &["test", "-p", "test-support", "sdk_conformance", "--", "--nocapture"])
}

fn run_sdk_schema_check() -> Result<()> {
    run("cargo", &["test", "-p", "test-support", "sdk_schema", "--", "--nocapture"])
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct InteropArtifactsManifest {
    version: u32,
    files: Vec<InteropArtifactEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct InteropArtifactEntry {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct InteropDriftBaseline {
    version: u32,
    corpus_version: u32,
    clients: BTreeMap<String, InteropClientSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct InteropClientSummary {
    release_track: String,
    entry_ids: Vec<String>,
    slices: Vec<String>,
    rpc_methods: Vec<String>,
    event_types: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct InteropCorpus {
    version: u32,
    entries: Vec<InteropCorpusEntry>,
}

#[derive(Debug, Deserialize)]
struct InteropCorpusEntry {
    id: String,
    client: String,
    release_track: String,
    slices: Vec<String>,
    rpc_send_request: InteropRpcRequest,
    event_payload: InteropEventPayload,
}

#[derive(Debug, Deserialize)]
struct InteropRpcRequest {
    method: String,
}

#[derive(Debug, Deserialize)]
struct InteropEventPayload {
    event_type: String,
}

#[derive(Debug, Default)]
struct InteropDriftClassification {
    breaking: Vec<String>,
    additive: Vec<String>,
}

fn run_interop_artifacts(update: bool) -> Result<()> {
    let manifest = build_interop_artifacts_manifest()?;
    if update {
        let serialized = serde_json::to_string_pretty(&manifest)
            .context("serialize interop artifacts manifest")?;
        fs::write(INTEROP_BASELINE_PATH, format!("{serialized}\n"))
            .with_context(|| format!("write {INTEROP_BASELINE_PATH}"))?;
        return Ok(());
    }

    let baseline_raw = fs::read_to_string(INTEROP_BASELINE_PATH).with_context(|| {
        format!(
            "missing interop artifact baseline at {INTEROP_BASELINE_PATH}; run `cargo run -p xtask -- interop-artifacts --update`"
        )
    })?;
    let baseline: InteropArtifactsManifest =
        serde_json::from_str(&baseline_raw).context("parse interop artifact baseline")?;
    if baseline != manifest {
        bail!(
            "interop artifacts drift detected; run `cargo run -p xtask -- interop-artifacts --update` and review {INTEROP_BASELINE_PATH}"
        );
    }
    Ok(())
}

fn run_interop_drift_check(update: bool) -> Result<()> {
    let current = build_interop_drift_baseline()?;
    if update {
        let serialized =
            serde_json::to_string_pretty(&current).context("serialize interop drift baseline")?;
        fs::write(INTEROP_DRIFT_BASELINE_PATH, format!("{serialized}\n"))
            .with_context(|| format!("write {INTEROP_DRIFT_BASELINE_PATH}"))?;
        return Ok(());
    }

    let baseline_raw = fs::read_to_string(INTEROP_DRIFT_BASELINE_PATH).with_context(|| {
        format!(
            "missing interop drift baseline at {INTEROP_DRIFT_BASELINE_PATH}; run `cargo run -p xtask -- interop-drift-check --update`"
        )
    })?;
    let baseline: InteropDriftBaseline =
        serde_json::from_str(&baseline_raw).context("parse interop drift baseline")?;
    let classification = classify_interop_drift(&baseline, &current);

    for note in &classification.additive {
        println!("interop drift additive: {note}");
    }
    if !classification.breaking.is_empty() {
        let details = classification.breaking.join("; ");
        bail!("interop semantic drift detected (breaking): {details}");
    }
    Ok(())
}

fn build_interop_drift_baseline() -> Result<InteropDriftBaseline> {
    let corpus_raw = fs::read_to_string(INTEROP_CORPUS_PATH)
        .with_context(|| format!("read {INTEROP_CORPUS_PATH}"))?;
    let corpus: InteropCorpus =
        serde_json::from_str(&corpus_raw).context("parse interop golden corpus")?;

    #[derive(Default)]
    struct ClientAccumulator {
        release_track: String,
        entry_ids: BTreeSet<String>,
        slices: BTreeSet<String>,
        rpc_methods: BTreeSet<String>,
        event_types: BTreeSet<String>,
    }

    let mut by_client: BTreeMap<String, ClientAccumulator> = BTreeMap::new();
    for entry in corpus.entries {
        let slot = by_client.entry(entry.client.clone()).or_default();
        if slot.release_track.is_empty() {
            slot.release_track = entry.release_track.clone();
        }
        slot.entry_ids.insert(entry.id);
        slot.rpc_methods.insert(entry.rpc_send_request.method);
        slot.event_types.insert(entry.event_payload.event_type);
        for slice in entry.slices {
            slot.slices.insert(slice);
        }
    }

    let clients = by_client
        .into_iter()
        .map(|(client, acc)| {
            (
                client,
                InteropClientSummary {
                    release_track: acc.release_track,
                    entry_ids: acc.entry_ids.into_iter().collect(),
                    slices: acc.slices.into_iter().collect(),
                    rpc_methods: acc.rpc_methods.into_iter().collect(),
                    event_types: acc.event_types.into_iter().collect(),
                },
            )
        })
        .collect();

    Ok(InteropDriftBaseline { version: 1, corpus_version: corpus.version, clients })
}

fn classify_interop_drift(
    baseline: &InteropDriftBaseline,
    current: &InteropDriftBaseline,
) -> InteropDriftClassification {
    let mut drift = InteropDriftClassification::default();

    for (client, baseline_summary) in &baseline.clients {
        let Some(current_summary) = current.clients.get(client) else {
            drift.breaking.push(format!("client '{client}' removed from corpus"));
            continue;
        };

        if baseline_summary.release_track != current_summary.release_track {
            drift.breaking.push(format!(
                "client '{client}' release_track changed '{}' -> '{}'",
                baseline_summary.release_track, current_summary.release_track
            ));
        }

        classify_vector_drift(
            &mut drift,
            client,
            "entry_ids",
            &baseline_summary.entry_ids,
            &current_summary.entry_ids,
        );
        classify_vector_drift(
            &mut drift,
            client,
            "slices",
            &baseline_summary.slices,
            &current_summary.slices,
        );
        classify_vector_drift(
            &mut drift,
            client,
            "rpc_methods",
            &baseline_summary.rpc_methods,
            &current_summary.rpc_methods,
        );
        classify_vector_drift(
            &mut drift,
            client,
            "event_types",
            &baseline_summary.event_types,
            &current_summary.event_types,
        );
    }

    for client in current.clients.keys() {
        if !baseline.clients.contains_key(client) {
            drift.additive.push(format!("client '{client}' added to corpus"));
        }
    }

    drift
}

fn classify_vector_drift(
    drift: &mut InteropDriftClassification,
    client: &str,
    field: &str,
    baseline: &[String],
    current: &[String],
) {
    let baseline_set = baseline.iter().cloned().collect::<BTreeSet<_>>();
    let current_set = current.iter().cloned().collect::<BTreeSet<_>>();

    for removed in baseline_set.difference(&current_set) {
        drift.breaking.push(format!(
            "client '{client}' removed {field} value '{removed}' from interop baseline"
        ));
    }
    for added in current_set.difference(&baseline_set) {
        drift
            .additive
            .push(format!("client '{client}' added {field} value '{added}' to interop corpus"));
    }
}

fn build_interop_artifacts_manifest() -> Result<InteropArtifactsManifest> {
    let mut files = Vec::new();
    for root in ["docs/contracts", "docs/schemas", "docs/fixtures"] {
        let root_path = Path::new(root);
        if !root_path.exists() {
            continue;
        }
        collect_files(root_path, &mut files)?;
    }

    files.sort();
    files.dedup();
    let mut entries = Vec::with_capacity(files.len());
    for path in files {
        if path == Path::new(INTEROP_BASELINE_PATH) {
            continue;
        }
        let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let sha256 = hex::encode(hasher.finalize());
        let relative = path
            .strip_prefix(Path::new("."))
            .unwrap_or(path.as_path())
            .to_string_lossy()
            .replace('\\', "/");
        entries.push(InteropArtifactEntry {
            path: relative,
            bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            sha256,
        });
    }
    entries.sort_by(|left, right| left.path.cmp(&right.path));

    Ok(InteropArtifactsManifest { version: 1, files: entries })
}

fn collect_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if root.is_file() {
        files.push(root.to_path_buf());
        return Ok(());
    }
    let mut children = fs::read_dir(root)
        .with_context(|| format!("read dir {}", root.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect::<Vec<_>>();
    children.sort();
    for path in children {
        if path.is_dir() {
            collect_files(path.as_path(), files)?;
        } else if path.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn run_sdk_profile_build() -> Result<()> {
    run(
        "cargo",
        &[
            "check",
            "-p",
            "lxmf-sdk",
            "--no-default-features",
            "--features",
            "std,rpc-backend,sdk-async",
        ],
    )?;
    run(
        "cargo",
        &["check", "-p", "lxmf-sdk", "--no-default-features", "--features", "std,rpc-backend"],
    )?;
    run(
        "cargo",
        &[
            "check",
            "-p",
            "lxmf-sdk",
            "--no-default-features",
            "--features",
            "std,rpc-backend,embedded-alloc",
        ],
    )?;
    Ok(())
}

fn run_sdk_examples_check() -> Result<()> {
    run("cargo", &["test", "-p", "lxmf-sdk", "--examples", "--no-run"])
}

fn run_sdk_api_break() -> Result<()> {
    const BASELINE_PATH: &str = "docs/contracts/baselines/lxmf-sdk-public-api.txt";
    const MANIFEST_PATH: &str = "crates/libs/lxmf-sdk/Cargo.toml";

    let baseline = fs::read_to_string(BASELINE_PATH).with_context(|| {
        format!(
            "missing SDK API baseline at {BASELINE_PATH}; add baseline before enabling sdk-api-break"
        )
    })?;
    let current = capture_public_api(MANIFEST_PATH)?;

    let baseline_normalized = normalize_public_api(&baseline);
    let current_normalized = normalize_public_api(&current);

    if baseline_normalized != current_normalized {
        bail!(
            "sdk public API drift detected for {MANIFEST_PATH}; review and refresh {BASELINE_PATH}"
        );
    }

    Ok(())
}

fn run_sdk_migration_check() -> Result<()> {
    const CUTOVER_MAP_PATH: &str = "docs/migrations/sdk-v2.5-cutover-map.md";
    let markdown = fs::read_to_string(CUTOVER_MAP_PATH)
        .with_context(|| format!("missing {CUTOVER_MAP_PATH}"))?;
    let rows = parse_cutover_rows(&markdown)?;
    if rows.is_empty() {
        bail!("cutover map must contain at least one consumer row");
    }

    for (idx, row) in rows.iter().enumerate() {
        let owner = row[2].trim();
        let classification = row[3].trim().to_ascii_lowercase();
        let replacement = row[4].trim();
        let removal_version = row[5].trim();

        if owner.is_empty() {
            bail!("cutover row {idx} missing owner");
        }
        if classification.is_empty() {
            bail!("cutover row {idx} missing classification");
        }
        if replacement.is_empty() {
            bail!("cutover row {idx} missing replacement");
        }
        if removal_version.is_empty() {
            bail!("cutover row {idx} missing removal version");
        }
        if !matches!(classification.as_str(), "keep" | "wrap" | "deprecate") {
            bail!("cutover row {idx} has invalid classification '{classification}'");
        }
        if classification == "wrap" && removal_version.eq_ignore_ascii_case("n/a") {
            bail!("cutover row {idx} classification=wrap requires explicit removal version");
        }
    }

    Ok(())
}

fn run_sdk_security_check() -> Result<()> {
    run("cargo", &["test", "-p", "rns-rpc", "sdk_security", "--", "--nocapture"])
}

fn run_sdk_property_check() -> Result<()> {
    run("cargo", &["test", "-p", "rns-rpc", "sdk_property", "--", "--nocapture"])
}

fn run_sdk_model_check() -> Result<()> {
    run(
        "cargo",
        &[
            "test",
            "-p",
            "lxmf-sdk",
            "lifecycle_model_transitions_and_method_legality_match_reference",
            "--",
            "--nocapture",
        ],
    )?;
    run("cargo", &["test", "-p", "test-support", "sdk_model", "--", "--nocapture"])
}

fn run_sdk_matrix_check() -> Result<()> {
    run("cargo", &["test", "-p", "test-support", "sdk_matrix", "--", "--nocapture"])
}

fn run_interop_matrix_check() -> Result<()> {
    let matrix = fs::read_to_string(INTEROP_MATRIX_PATH)
        .with_context(|| format!("missing {INTEROP_MATRIX_PATH}"))?;
    for required_section in [
        "## Matrix Version",
        "## Protocol Slice Definitions",
        "## Client Matrix (v1)",
        "## Support Windows",
    ] {
        if !matrix.contains(required_section) {
            bail!("interop matrix missing required section '{required_section}'");
        }
    }

    let client_rows = parse_markdown_table_rows(
        &matrix,
        &[
            "Client",
            "Version window",
            "RPC v2",
            "Payload v2",
            "Event Cursor v2",
            "Release B Domains",
            "Release C Domains",
            "Auth Token",
            "Auth mTLS",
            "Delivery Modes",
        ],
    )?;
    if client_rows.is_empty() {
        bail!("interop matrix client table must contain at least one row");
    }

    let required_clients = ["lxmf-sdk", "reticulumd", "sideband", "rch", "columba"];
    for required_client in required_clients {
        if !client_rows.iter().any(|row| {
            row.first()
                .map(|cell| cell.to_ascii_lowercase().contains(required_client))
                .unwrap_or(false)
        }) {
            bail!("interop matrix missing required client row containing '{required_client}'");
        }
    }

    for row in &client_rows {
        if row.len() != 10 {
            bail!("interop matrix row must have 10 columns, found {} in '{row:?}'", row.len());
        }
        if row[1].trim().is_empty() {
            bail!("interop matrix row '{}' has empty version window", row[0].trim());
        }
        for (column_name, value) in [
            ("RPC v2", row[2].trim()),
            ("Payload v2", row[3].trim()),
            ("Event Cursor v2", row[4].trim()),
            ("Release B Domains", row[5].trim()),
            ("Release C Domains", row[6].trim()),
            ("Auth Token", row[7].trim()),
            ("Auth mTLS", row[8].trim()),
            ("Delivery Modes", row[9].trim()),
        ] {
            let status_token = value
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_matches(|ch: char| ch == ',' || ch == ';')
                .to_ascii_lowercase();
            if !matches!(status_token.as_str(), "required" | "optional" | "planned" | "n/a") {
                bail!(
                    "interop matrix row '{}' has invalid status '{value}' in column '{column_name}'",
                    row[0].trim()
                );
            }
        }
    }

    let rpc_contract = fs::read_to_string(RPC_CONTRACT_PATH)
        .with_context(|| format!("missing {RPC_CONTRACT_PATH}"))?;
    if !rpc_contract.contains("`slice_id`: `rpc_v2`")
        || !rpc_contract.contains("docs/contracts/compatibility-matrix.md")
    {
        bail!("rpc contract must declare `slice_id`: `rpc_v2` and reference compatibility matrix");
    }

    let payload_contract = fs::read_to_string(PAYLOAD_CONTRACT_PATH)
        .with_context(|| format!("missing {PAYLOAD_CONTRACT_PATH}"))?;
    if !payload_contract.contains("`slice_id`: `payload_v2`")
        || !payload_contract.contains("docs/contracts/compatibility-matrix.md")
    {
        bail!(
            "payload contract must declare `slice_id`: `payload_v2` and reference compatibility matrix"
        );
    }

    Ok(())
}

fn run_interop_corpus_check() -> Result<()> {
    run("cargo", &["test", "-p", "test-support", "sdk_interop_corpus", "--", "--nocapture"])
}

fn run_e2e_compatibility() -> Result<()> {
    run("cargo", &["run", "-p", "rns-tools", "--bin", "rnx", "--", "e2e", "--timeout-secs", "20"])
}

fn run_mesh_sim() -> Result<()> {
    run(
        "cargo",
        &[
            "run",
            "-p",
            "rns-tools",
            "--bin",
            "rnx",
            "--",
            "mesh-sim",
            "--nodes",
            "5",
            "--timeout-secs",
            "60",
        ],
    )
}

fn run_unused_deps() -> Result<()> {
    let rustup_available = Command::new("rustup")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);

    if rustup_available {
        let nightly_udeps = toolchain_cargo_command("nightly", "udeps --workspace --all-targets");
        return run("bash", &["-lc", &nightly_udeps]);
    }

    run("cargo", &["+nightly", "udeps", "--workspace", "--all-targets"])
}

fn run_migration_checks() -> Result<()> {
    let enforce_legacy_imports =
        std::env::var("ENFORCE_LEGACY_APP_IMPORTS").unwrap_or("1".to_string());
    let enforce_legacy_shims =
        std::env::var("ENFORCE_RETM_LEGACY_SHIMS").unwrap_or("1".to_string());
    run_sdk_migration_check()?;
    run_boundary_checks(&enforce_legacy_imports, &enforce_legacy_shims)?;
    run(
        "bash",
        &["-lc", "! rg -n 'crates/(lxmf|reticulum|reticulum-daemon)/' README.md .github/workflows || exit 1"],
    )?;
    Ok(())
}

fn run_architecture_checks() -> Result<()> {
    run_forbidden_deps()
}

fn run_forbidden_deps() -> Result<()> {
    let enforce_legacy_imports =
        std::env::var("ENFORCE_LEGACY_APP_IMPORTS").unwrap_or("1".to_string());
    let enforce_legacy_shims =
        std::env::var("ENFORCE_RETM_LEGACY_SHIMS").unwrap_or("1".to_string());
    run_boundary_checks(&enforce_legacy_imports, &enforce_legacy_shims)
}

fn run_boundary_checks(enforce_legacy_imports: &str, enforce_legacy_shims: &str) -> Result<()> {
    let command = format!(
        "ENFORCE_LEGACY_APP_IMPORTS={enforce_legacy_imports} ENFORCE_RETM_LEGACY_SHIMS={enforce_legacy_shims} ./tools/scripts/check-boundaries.sh"
    );
    run("bash", &["-lc", &command])
}

fn parse_cutover_rows(markdown: &str) -> Result<Vec<Vec<String>>> {
    let mut rows = Vec::new();
    let mut in_table = false;

    for line in markdown.lines() {
        let trimmed = line.trim();
        if !in_table {
            if trimmed.starts_with("| Surface |")
                && trimmed.contains("| Classification |")
                && trimmed.contains("| Removal version |")
            {
                in_table = true;
            }
            continue;
        }

        if !trimmed.starts_with('|') {
            if !rows.is_empty() {
                break;
            }
            continue;
        }
        if trimmed.contains("---") {
            continue;
        }

        let cells = trimmed
            .trim_matches('|')
            .split('|')
            .map(|cell| cell.trim().to_string())
            .collect::<Vec<_>>();
        if cells.len() != 7 {
            bail!("malformed cutover row '{trimmed}' (expected 7 columns, found {})", cells.len());
        }
        rows.push(cells);
    }

    Ok(rows)
}

fn parse_markdown_table_rows(markdown: &str, header_cells: &[&str]) -> Result<Vec<Vec<String>>> {
    let mut rows = Vec::new();
    let mut in_table = false;

    for line in markdown.lines() {
        let trimmed = line.trim();
        if !in_table {
            if trimmed.starts_with('|')
                && header_cells.iter().all(|header_cell| trimmed.contains(header_cell))
            {
                in_table = true;
            }
            continue;
        }

        if !trimmed.starts_with('|') {
            if !rows.is_empty() {
                break;
            }
            continue;
        }
        if trimmed.contains("---") {
            continue;
        }

        let cells = trimmed
            .trim_matches('|')
            .split('|')
            .map(|cell| cell.trim().to_string())
            .collect::<Vec<_>>();
        rows.push(cells);
    }

    Ok(rows)
}

fn capture_public_api(manifest: &str) -> Result<String> {
    let toolchain = public_api_toolchain();
    let args = format!("public-api --manifest-path {manifest} -sss --color never");
    let command = toolchain_cargo_command(&toolchain, &args);
    let output = Command::new("bash")
        .args(["-lc", &command])
        .output()
        .with_context(|| format!("failed to spawn cargo public-api for {manifest}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("cargo public-api failed for {manifest}: {stderr}");
    }
    String::from_utf8(output.stdout)
        .with_context(|| format!("cargo public-api output was not valid utf-8 for {manifest}"))
}

fn public_api_toolchain() -> String {
    std::env::var("SDK_API_BREAK_TOOLCHAIN").unwrap_or_else(|_| "nightly".to_string())
}

fn toolchain_cargo_command(toolchain: &str, cargo_args: &str) -> String {
    format!(
        "set -euo pipefail; \
         CARGO_BIN=\"$(rustup which --toolchain {toolchain} cargo)\"; \
         RUSTC_BIN=\"$(rustup which --toolchain {toolchain} rustc)\"; \
         RUSTDOC_BIN=\"$(rustup which --toolchain {toolchain} rustdoc)\"; \
         PATH=\"$(dirname \"$CARGO_BIN\"):$PATH\" \
         RUSTUP_TOOLCHAIN={toolchain} \
         RUSTC=\"$RUSTC_BIN\" \
         RUSTDOC=\"$RUSTDOC_BIN\" \
         \"$CARGO_BIN\" {cargo_args}"
    )
}

fn normalize_public_api(raw: &str) -> String {
    raw.lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with("warning:"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn run(cmd: &str, args: &[&str]) -> Result<()> {
    let status =
        Command::new(cmd).args(args).status().with_context(|| format!("failed to spawn {cmd}"))?;
    if !status.success() {
        bail!("command failed: {cmd} {}", args.join(" "));
    }
    Ok(())
}
