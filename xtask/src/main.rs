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
const SUPPORT_POLICY_PATH: &str = "docs/contracts/support-policy.md";
const EXTENSION_REGISTRY_PATH: &str = "docs/contracts/extension-registry.md";
const EXTENSION_REGISTRY_ADR_PATH: &str = "docs/adr/0005-extension-registry-governance.md";
const INTEROP_CORPUS_PATH: &str = "docs/fixtures/interop/v1/golden-corpus.json";
const RPC_CONTRACT_PATH: &str = "docs/contracts/rpc-contract.md";
const PAYLOAD_CONTRACT_PATH: &str = "docs/contracts/payload-contract.md";
const CODEOWNERS_PATH: &str = ".github/CODEOWNERS";
const CI_WORKFLOW_PATH: &str = ".github/workflows/ci.yml";
const SECURITY_THREAT_MODEL_PATH: &str = "docs/adr/0004-sdk-v25-threat-model.md";
const SECURITY_REVIEW_CHECKLIST_PATH: &str = "docs/runbooks/security-review-checklist.md";
const SDK_DOCS_CHECKLIST_PATH: &str = "docs/runbooks/sdk-docs-checklist.md";
const INCIDENT_RUNBOOK_PATH: &str = "docs/runbooks/incident-response-playbooks.md";
const DISASTER_RECOVERY_RUNBOOK_PATH: &str = "docs/runbooks/disaster-recovery-drills.md";
const BACKUP_RESTORE_DRILL_SCRIPT_PATH: &str = "tools/scripts/backup-restore-drill.sh";
const SOAK_REPORT_PATH: &str = "target/soak/soak-report.json";
const BENCH_SUMMARY_PATH: &str = "target/criterion/bench-summary.txt";
const PERF_BUDGET_REPORT_PATH: &str = "target/criterion/bench-budget-report.txt";
const SUPPLY_CHAIN_SBOM_PATH: &str = "target/supply-chain/sbom/cargo-metadata.sbom.json";
const SUPPLY_CHAIN_PROVENANCE_PATH: &str =
    "target/supply-chain/provenance/artifact-provenance.json";
const SUPPLY_CHAIN_SIGNATURE_PATH: &str =
    "target/supply-chain/provenance/artifact-provenance.sha256";
const REPRODUCIBLE_BUILD_REPORT_PATH: &str =
    "target/supply-chain/reproducible/reproducible-build-report.txt";
const LEADER_READINESS_REPORT_PATH: &str = "target/release-readiness/leader-grade-readiness.md";

const RELEASE_BINARIES: &[&str] = &[
    "lxmf-cli",
    "reticulumd",
    "rncp",
    "rnid",
    "rnir",
    "rnodeconf",
    "rnpath",
    "rnpkg",
    "rnprobe",
    "rnsd",
    "rnstatus",
    "rnx",
];

const GOVERNANCE_REQUIRED_CODEOWNER_PATHS: &[&str] = &[
    "/SECURITY.md",
    "/docs/contracts/",
    "/docs/schemas/",
    "/docs/migrations/",
    "/docs/runbooks/",
    "/crates/libs/lxmf-core/",
    "/crates/libs/lxmf-sdk/",
    "/crates/libs/rns-core/",
    "/crates/libs/rns-transport/",
    "/crates/libs/rns-rpc/",
    "/crates/libs/test-support/",
    "/crates/apps/lxmf-cli/",
    "/crates/apps/reticulumd/",
    "/crates/apps/rns-tools/",
    "/.github/workflows/",
    "/xtask/",
    "/tools/scripts/",
];

const GOVERNANCE_FORBIDDEN_CODEOWNER_PATHS: &[&str] =
    &["/crates/libs/lxmf-router/", "/crates/libs/lxmf-runtime/"];

#[derive(Copy, Clone)]
struct PerfBudget {
    benchmark: &'static str,
    max_p50_ns: f64,
    max_p95_ns: f64,
    max_p99_ns: f64,
    min_throughput_ops_per_sec: f64,
}

struct RequiredSdkDoc {
    path: &'static str,
    headings: &'static [&'static str],
}

const REQUIRED_SDK_DOCS: &[RequiredSdkDoc] = &[
    RequiredSdkDoc {
        path: "docs/sdk/README.md",
        headings: &["# SDK Integration Guide", "## Reading Order", "## Core Concepts"],
    },
    RequiredSdkDoc {
        path: "docs/sdk/quickstart.md",
        headings: &[
            "# SDK Quickstart",
            "## Prerequisites",
            "## Start `reticulumd`",
            "## Minimal SDK Client",
            "## Send and Poll Events",
        ],
    },
    RequiredSdkDoc {
        path: "docs/sdk/configuration-profiles.md",
        headings: &[
            "# SDK Configuration and Profiles",
            "## Profile Selection",
            "## Security Baselines",
            "## Event Stream and Backpressure",
        ],
    },
    RequiredSdkDoc {
        path: "docs/sdk/lifecycle-and-events.md",
        headings: &[
            "# SDK Lifecycle and Event Flow",
            "## Lifecycle State Machine",
            "## Cursor Polling Pattern",
            "## Event Handling Guidance",
        ],
    },
    RequiredSdkDoc {
        path: "docs/sdk/advanced-embedding.md",
        headings: &[
            "# SDK Advanced Embedding",
            "## Capability-Negotiated Feature Use",
            "## Idempotency and Cancellation",
            "## Embedded and Manual Tick Integration",
        ],
    },
];

const REQUIRED_SDK_DOC_CHECKLIST_ITEMS: &[&str] = &[
    "- [x] docs/sdk/README.md",
    "- [x] docs/sdk/quickstart.md",
    "- [x] docs/sdk/configuration-profiles.md",
    "- [x] docs/sdk/lifecycle-and-events.md",
    "- [x] docs/sdk/advanced-embedding.md",
    "- [x] README.md includes SDK guide links",
    "- [x] docs/architecture/overview.md links to SDK guide index",
];

const PERF_BUDGETS: &[PerfBudget] = &[
    PerfBudget {
        benchmark: "lxmf_core_message_from_wire",
        max_p50_ns: 1_500.0,
        max_p95_ns: 2_500.0,
        max_p99_ns: 3_500.0,
        min_throughput_ops_per_sec: 500_000.0,
    },
    PerfBudget {
        benchmark: "lxmf_core_decode_inbound_message",
        max_p50_ns: 5_000.0,
        max_p95_ns: 9_000.0,
        max_p99_ns: 12_000.0,
        min_throughput_ops_per_sec: 150_000.0,
    },
    PerfBudget {
        benchmark: "lxmf_core_message_to_wire",
        max_p50_ns: 2_000.0,
        max_p95_ns: 3_000.0,
        max_p99_ns: 4_000.0,
        min_throughput_ops_per_sec: 350_000.0,
    },
    PerfBudget {
        benchmark: "lxmf_sdk_start",
        max_p50_ns: 15_000.0,
        max_p95_ns: 25_000.0,
        max_p99_ns: 35_000.0,
        min_throughput_ops_per_sec: 30_000.0,
    },
    PerfBudget {
        benchmark: "lxmf_sdk_send",
        max_p50_ns: 2_000.0,
        max_p95_ns: 3_000.0,
        max_p99_ns: 4_500.0,
        min_throughput_ops_per_sec: 350_000.0,
    },
    PerfBudget {
        benchmark: "lxmf_sdk_poll_events",
        max_p50_ns: 300.0,
        max_p95_ns: 450.0,
        max_p99_ns: 650.0,
        min_throughput_ops_per_sec: 20_000_000.0,
    },
    PerfBudget {
        benchmark: "lxmf_sdk_snapshot",
        max_p50_ns: 1_500.0,
        max_p95_ns: 2_000.0,
        max_p99_ns: 2_500.0,
        min_throughput_ops_per_sec: 600_000.0,
    },
    PerfBudget {
        benchmark: "rns_rpc_send_message_v2",
        max_p50_ns: 100_000.0,
        max_p95_ns: 150_000.0,
        max_p99_ns: 220_000.0,
        min_throughput_ops_per_sec: 25_000.0,
    },
    PerfBudget {
        benchmark: "rns_rpc_sdk_poll_events_v2",
        max_p50_ns: 15_000.0,
        max_p95_ns: 20_000.0,
        max_p99_ns: 25_000.0,
        min_throughput_ops_per_sec: 90_000.0,
    },
    PerfBudget {
        benchmark: "rns_rpc_sdk_snapshot_v2",
        max_p50_ns: 25_000.0,
        max_p95_ns: 35_000.0,
        max_p99_ns: 45_000.0,
        min_throughput_ops_per_sec: 45_000.0,
    },
    PerfBudget {
        benchmark: "rns_rpc_sdk_topic_create_v2",
        max_p50_ns: 70_000.0,
        max_p95_ns: 95_000.0,
        max_p99_ns: 130_000.0,
        min_throughput_ops_per_sec: 14_000.0,
    },
];

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
    SdkDocsCheck,
    SdkCookbookCheck,
    SdkErgonomicsCheck,
    LxmfCliCheck,
    DxBootstrapCheck,
    SdkIncidentRunbookCheck,
    SdkDrillCheck,
    SdkSoakCheck,
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
    CompatKitCheck,
    E2eCompatibility,
    MeshSim,
    SdkProfileBuild,
    SdkExamplesCheck,
    SdkApiBreak,
    SdkMigrationCheck,
    GovernanceCheck,
    SupportPolicyCheck,
    ReleaseScorecardCheck,
    ExtensionRegistryCheck,
    LeaderReadinessCheck,
    SecurityReviewCheck,
    SdkSecurityCheck,
    SdkFuzzCheck,
    SdkPropertyCheck,
    SdkModelCheck,
    SdkRaceCheck,
    SdkReplayCheck,
    SdkMetricsCheck,
    SdkBenchCheck,
    SdkPerfBudgetCheck,
    SdkMemoryBudgetCheck,
    SdkQueuePressureCheck,
    SupplyChainCheck,
    ReproducibleBuildCheck,
    SdkMatrixCheck,
    EmbeddedLinkCheck,
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
    SdkDocsCheck,
    SdkCookbookCheck,
    SdkErgonomicsCheck,
    LxmfCliCheck,
    DxBootstrapCheck,
    SdkIncidentRunbookCheck,
    SdkDrillCheck,
    SdkSoakCheck,
    InteropArtifacts,
    InteropMatrixCheck,
    InteropCorpusCheck,
    InteropDriftCheck,
    CompatKitCheck,
    E2eCompatibility,
    SdkProfileBuild,
    SdkExamplesCheck,
    SdkApiBreak,
    SdkMigrationCheck,
    GovernanceCheck,
    SupportPolicyCheck,
    ReleaseScorecardCheck,
    ExtensionRegistryCheck,
    LeaderReadinessCheck,
    SecurityReviewCheck,
    SdkSecurityCheck,
    SdkFuzzCheck,
    SdkPropertyCheck,
    SdkModelCheck,
    SdkRaceCheck,
    SdkReplayCheck,
    SdkMetricsCheck,
    SdkBenchCheck,
    SdkPerfBudgetCheck,
    SdkMemoryBudgetCheck,
    SdkQueuePressureCheck,
    SupplyChainCheck,
    ReproducibleBuildCheck,
    SdkMatrixCheck,
    EmbeddedLinkCheck,
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
        XtaskCommand::SdkDocsCheck => run_sdk_docs_check(),
        XtaskCommand::SdkCookbookCheck => run_sdk_cookbook_check(),
        XtaskCommand::SdkErgonomicsCheck => run_sdk_ergonomics_check(),
        XtaskCommand::LxmfCliCheck => run_lxmf_cli_check(),
        XtaskCommand::DxBootstrapCheck => run_dx_bootstrap_check(),
        XtaskCommand::SdkIncidentRunbookCheck => run_sdk_incident_runbook_check(),
        XtaskCommand::SdkDrillCheck => run_sdk_drill_check(),
        XtaskCommand::SdkSoakCheck => run_sdk_soak_check(),
        XtaskCommand::InteropArtifacts { update } => run_interop_artifacts(update),
        XtaskCommand::InteropMatrixCheck => run_interop_matrix_check(),
        XtaskCommand::InteropCorpusCheck => run_interop_corpus_check(),
        XtaskCommand::InteropDriftCheck { update } => run_interop_drift_check(update),
        XtaskCommand::CompatKitCheck => run_compat_kit_check(),
        XtaskCommand::E2eCompatibility => run_e2e_compatibility(),
        XtaskCommand::MeshSim => run_mesh_sim(),
        XtaskCommand::SdkProfileBuild => run_sdk_profile_build(),
        XtaskCommand::SdkExamplesCheck => run_sdk_examples_check(),
        XtaskCommand::SdkApiBreak => run_sdk_api_break(),
        XtaskCommand::SdkMigrationCheck => run_sdk_migration_check(),
        XtaskCommand::GovernanceCheck => run_governance_check(),
        XtaskCommand::SupportPolicyCheck => run_support_policy_check(),
        XtaskCommand::ReleaseScorecardCheck => run_release_scorecard_check(),
        XtaskCommand::ExtensionRegistryCheck => run_extension_registry_check(),
        XtaskCommand::LeaderReadinessCheck => run_leader_readiness_check(),
        XtaskCommand::SecurityReviewCheck => run_security_review_check(),
        XtaskCommand::SdkSecurityCheck => run_sdk_security_check(),
        XtaskCommand::SdkFuzzCheck => run_sdk_fuzz_check(),
        XtaskCommand::SdkPropertyCheck => run_sdk_property_check(),
        XtaskCommand::SdkModelCheck => run_sdk_model_check(),
        XtaskCommand::SdkRaceCheck => run_sdk_race_check(),
        XtaskCommand::SdkReplayCheck => run_sdk_replay_check(),
        XtaskCommand::SdkMetricsCheck => run_sdk_metrics_check(),
        XtaskCommand::SdkBenchCheck => run_sdk_bench_check(),
        XtaskCommand::SdkPerfBudgetCheck => run_sdk_perf_budget_check(),
        XtaskCommand::SdkMemoryBudgetCheck => run_sdk_memory_budget_check(),
        XtaskCommand::SdkQueuePressureCheck => run_sdk_queue_pressure_check(),
        XtaskCommand::SupplyChainCheck => run_supply_chain_check(),
        XtaskCommand::ReproducibleBuildCheck => run_reproducible_build_check(),
        XtaskCommand::SdkMatrixCheck => run_sdk_matrix_check(),
        XtaskCommand::EmbeddedLinkCheck => run_embedded_link_check(),
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
    run_sdk_docs_check()?;
    run_sdk_cookbook_check()?;
    run_sdk_ergonomics_check()?;
    run_lxmf_cli_check()?;
    run_dx_bootstrap_check()?;
    run_sdk_incident_runbook_check()?;
    run_sdk_drill_check()?;
    run_sdk_soak_check()?;
    run_sdk_schema_check()?;
    run_interop_artifacts(false)?;
    run_interop_matrix_check()?;
    run_interop_corpus_check()?;
    run_interop_drift_check(false)?;
    run_compat_kit_check()?;
    run_e2e_compatibility()?;
    run_sdk_conformance()?;
    run_sdk_profile_build()?;
    run_sdk_examples_check()?;
    run_governance_check()?;
    run_support_policy_check()?;
    run_release_scorecard_check()?;
    run_extension_registry_check()?;
    run_security_review_check()?;
    run_sdk_security_check()?;
    run_sdk_fuzz_check()?;
    run_sdk_property_check()?;
    run_sdk_model_check()?;
    run_sdk_race_check()?;
    run_sdk_replay_check()?;
    run_sdk_metrics_check()?;
    run_sdk_perf_budget_check()?;
    run_sdk_memory_budget_check()?;
    run_sdk_queue_pressure_check()?;
    run_reproducible_build_check()?;
    run_sdk_matrix_check()?;
    run_embedded_link_check()?;
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
            run("cargo", &["audit"])?;
            run_security_review_check()
        }
        CiStage::UnusedDeps => run_unused_deps(),
        CiStage::ApiSurfaceCheck => run_api_diff(),
        CiStage::SdkConformance => run_sdk_conformance(),
        CiStage::SdkSchemaCheck => run_sdk_schema_check(),
        CiStage::SdkDocsCheck => run_sdk_docs_check(),
        CiStage::SdkCookbookCheck => run_sdk_cookbook_check(),
        CiStage::SdkErgonomicsCheck => run_sdk_ergonomics_check(),
        CiStage::LxmfCliCheck => run_lxmf_cli_check(),
        CiStage::DxBootstrapCheck => run_dx_bootstrap_check(),
        CiStage::SdkIncidentRunbookCheck => run_sdk_incident_runbook_check(),
        CiStage::SdkDrillCheck => run_sdk_drill_check(),
        CiStage::SdkSoakCheck => run_sdk_soak_check(),
        CiStage::InteropArtifacts => run_interop_artifacts(false),
        CiStage::InteropMatrixCheck => run_interop_matrix_check(),
        CiStage::InteropCorpusCheck => run_interop_corpus_check(),
        CiStage::InteropDriftCheck => run_interop_drift_check(false),
        CiStage::CompatKitCheck => run_compat_kit_check(),
        CiStage::E2eCompatibility => run_e2e_compatibility(),
        CiStage::SdkProfileBuild => run_sdk_profile_build(),
        CiStage::SdkExamplesCheck => run_sdk_examples_check(),
        CiStage::SdkApiBreak => run_sdk_api_break(),
        CiStage::SdkMigrationCheck => run_sdk_migration_check(),
        CiStage::GovernanceCheck => run_governance_check(),
        CiStage::SupportPolicyCheck => run_support_policy_check(),
        CiStage::ReleaseScorecardCheck => run_release_scorecard_check(),
        CiStage::ExtensionRegistryCheck => run_extension_registry_check(),
        CiStage::LeaderReadinessCheck => run_leader_readiness_check(),
        CiStage::SecurityReviewCheck => run_security_review_check(),
        CiStage::SdkSecurityCheck => run_sdk_security_check(),
        CiStage::SdkFuzzCheck => run_sdk_fuzz_check(),
        CiStage::SdkPropertyCheck => run_sdk_property_check(),
        CiStage::SdkModelCheck => run_sdk_model_check(),
        CiStage::SdkRaceCheck => run_sdk_race_check(),
        CiStage::SdkReplayCheck => run_sdk_replay_check(),
        CiStage::SdkMetricsCheck => run_sdk_metrics_check(),
        CiStage::SdkBenchCheck => run_sdk_bench_check(),
        CiStage::SdkPerfBudgetCheck => run_sdk_perf_budget_check(),
        CiStage::SdkMemoryBudgetCheck => run_sdk_memory_budget_check(),
        CiStage::SdkQueuePressureCheck => run_sdk_queue_pressure_check(),
        CiStage::SupplyChainCheck => run_supply_chain_check(),
        CiStage::ReproducibleBuildCheck => run_reproducible_build_check(),
        CiStage::SdkMatrixCheck => run_sdk_matrix_check(),
        CiStage::EmbeddedLinkCheck => run_embedded_link_check(),
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
    run_compat_kit_check()?;
    run_support_policy_check()?;
    run_release_scorecard_check()?;
    run_extension_registry_check()?;
    run_sdk_api_break()?;
    run_supply_chain_check()?;
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

fn run_sdk_docs_check() -> Result<()> {
    let checklist = fs::read_to_string(SDK_DOCS_CHECKLIST_PATH)
        .with_context(|| format!("read {SDK_DOCS_CHECKLIST_PATH}"))?;
    for item in REQUIRED_SDK_DOC_CHECKLIST_ITEMS {
        if !checklist.contains(item) {
            bail!("missing checklist item in {SDK_DOCS_CHECKLIST_PATH}: {item}");
        }
    }

    for required in REQUIRED_SDK_DOCS {
        let doc =
            fs::read_to_string(required.path).with_context(|| format!("read {}", required.path))?;
        for heading in required.headings {
            if !doc.contains(heading) {
                bail!("missing required heading in {}: {heading}", required.path);
            }
        }
    }
    Ok(())
}

fn run_sdk_cookbook_check() -> Result<()> {
    run("cargo", &["test", "-p", "test-support", "sdk_cookbook", "--", "--nocapture"])
}

fn run_sdk_ergonomics_check() -> Result<()> {
    for test_name in [
        "start_request_builder_defaults_and_customization_validate",
        "send_request_builder_sets_optional_fields_and_extensions",
        "sdk_config_default_profiles_validate",
        "sdk_config_remote_auth_helpers_apply_valid_security_modes",
        "config_patch_builder_accumulates_mutations",
    ] {
        run("cargo", &["test", "-p", "lxmf-sdk", test_name, "--", "--nocapture"])?;
    }
    run("cargo", &["test", "-p", "lxmf-sdk", "--examples", "--no-run"])
}

fn run_lxmf_cli_check() -> Result<()> {
    run("cargo", &["test", "-p", "lxmf-cli"])?;
    run("cargo", &["run", "-p", "lxmf-cli", "--", "--help"])?;
    run("bash", &["-lc", "cargo run -p lxmf-cli -- completions --shell bash > /dev/null"])
}

fn run_dx_bootstrap_check() -> Result<()> {
    run("bash", &["tools/scripts/bootstrap-dev.sh", "--check", "--skip-tools", "--skip-smoke"])
}

fn run_sdk_incident_runbook_check() -> Result<()> {
    let runbook = fs::read_to_string(INCIDENT_RUNBOOK_PATH)
        .with_context(|| format!("read {INCIDENT_RUNBOOK_PATH}"))?;
    for heading in [
        "# Incident Response Playbooks",
        "## Incident Severity and Escalation",
        "## P0: RPC Auth Failure Spike",
        "## P0: Event Stream Degraded or Cursor Expired",
        "## P1: Message Delivery Stall",
        "## P1: Durable Store Corruption or Restart Loop",
        "## Post-Incident Review and Follow-up",
    ] {
        if !runbook.contains(heading) {
            bail!("missing incident runbook heading in {INCIDENT_RUNBOOK_PATH}: {heading}");
        }
    }
    let playbook_count = runbook.lines().filter(|line| line.starts_with("## P")).count();
    if playbook_count < 4 {
        bail!(
            "incident runbook must define at least 4 playbook sections in {INCIDENT_RUNBOOK_PATH}"
        );
    }
    Ok(())
}

fn run_sdk_drill_check() -> Result<()> {
    let runbook = fs::read_to_string(DISASTER_RECOVERY_RUNBOOK_PATH)
        .with_context(|| format!("read {DISASTER_RECOVERY_RUNBOOK_PATH}"))?;
    for heading in [
        "# Disaster Recovery Drills",
        "## Objectives",
        "## Automated Drill",
        "## Migration Rollback Readiness",
        "## Evidence to Attach",
    ] {
        if !runbook.contains(heading) {
            bail!(
                "missing disaster recovery runbook heading in {DISASTER_RECOVERY_RUNBOOK_PATH}: {heading}"
            );
        }
    }
    run("bash", &[BACKUP_RESTORE_DRILL_SCRIPT_PATH])
}

fn run_sdk_soak_check() -> Result<()> {
    run(
        "bash",
        &[
            "-lc",
            "CYCLES=1 BURST_ROUNDS=2 TIMEOUT_SECS=20 PAUSE_SECS=0 CHAOS_INTERVAL=2 CHAOS_NODES=4 CHAOS_TIMEOUT_SECS=60 MAX_FAILURES=0 REPORT_PATH=target/soak/soak-report.json ./tools/scripts/soak-rnx.sh",
        ],
    )?;
    let report =
        fs::read_to_string(SOAK_REPORT_PATH).with_context(|| format!("read {SOAK_REPORT_PATH}"))?;
    if !report.contains("\"status\": \"pass\"") {
        bail!("soak report indicates non-pass status in {SOAK_REPORT_PATH}");
    }
    if !report.contains("\"max_failures\": 0") {
        bail!("soak report must include enforced regression threshold in {SOAK_REPORT_PATH}");
    }
    Ok(())
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

fn run_governance_check() -> Result<()> {
    let codeowners = fs::read_to_string(CODEOWNERS_PATH)
        .with_context(|| format!("missing {CODEOWNERS_PATH}"))?;

    let parsed_lines: Vec<(&str, Vec<&str>)> = codeowners
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let mut fields = line.split_whitespace();
            let path = fields.next().unwrap_or_default();
            let owners = fields.collect::<Vec<_>>();
            (path, owners)
        })
        .collect();

    for forbidden in GOVERNANCE_FORBIDDEN_CODEOWNER_PATHS {
        if parsed_lines.iter().any(|(path, _)| path == forbidden) {
            bail!("CODEOWNERS contains deprecated path '{forbidden}'");
        }
    }

    for required_path in GOVERNANCE_REQUIRED_CODEOWNER_PATHS {
        let owners = parsed_lines
            .iter()
            .find_map(|(path, owners)| (*path == *required_path).then_some(owners))
            .with_context(|| {
                format!("CODEOWNERS missing required ownership entry '{required_path}'")
            })?;

        if owners.is_empty() {
            bail!("CODEOWNERS entry '{required_path}' must declare at least one owner");
        }
        if !owners.iter().any(|owner| owner.starts_with('@')) {
            bail!("CODEOWNERS entry '{required_path}' must use explicit GitHub owner handles");
        }
        if !owners.contains(&"@FreeTAKTeam") {
            bail!("CODEOWNERS entry '{required_path}' must include @FreeTAKTeam");
        }
    }

    let workflow = fs::read_to_string(CI_WORKFLOW_PATH)
        .with_context(|| format!("missing {CI_WORKFLOW_PATH}"))?;
    if !workflow.contains("governance-check:") {
        bail!("ci workflow must include a 'governance-check' job");
    }
    if !workflow.contains("cargo xtask ci --stage governance-check") {
        bail!("ci workflow must execute `cargo xtask ci --stage governance-check`");
    }

    Ok(())
}

fn run_support_policy_check() -> Result<()> {
    let support_policy = fs::read_to_string(SUPPORT_POLICY_PATH)
        .with_context(|| format!("missing {SUPPORT_POLICY_PATH}"))?;
    for marker in [
        "# Version Support and LTS Policy",
        "## Release Channels",
        "## LTS Selection Rules",
        "## Deprecation and Removal Policy",
        "## Compliance Gates",
        "| `Current (N)` |",
        "| `Maintenance (N-1)` |",
        "| `LTS` |",
        "| `EOL` |",
    ] {
        if !support_policy.contains(marker) {
            bail!("support policy missing required marker '{marker}' in {SUPPORT_POLICY_PATH}");
        }
    }

    let readme = fs::read_to_string("README.md").context("missing README.md")?;
    if !readme.contains("docs/contracts/support-policy.md") {
        bail!("README.md must reference docs/contracts/support-policy.md");
    }

    let migration = fs::read_to_string("docs/contracts/sdk-v2-migration.md")
        .context("missing docs/contracts/sdk-v2-migration.md")?;
    if !migration.contains("docs/contracts/support-policy.md") {
        bail!("sdk-v2 migration contract must reference docs/contracts/support-policy.md");
    }

    let release_readiness = fs::read_to_string("docs/runbooks/release-readiness.md")
        .context("missing docs/runbooks/release-readiness.md")?;
    if !release_readiness.contains("support-policy-check") {
        bail!("release readiness checklist must include support-policy-check gate");
    }

    let workflow = fs::read_to_string(CI_WORKFLOW_PATH)
        .with_context(|| format!("missing {CI_WORKFLOW_PATH}"))?;
    if !workflow.contains("support-policy-check:") {
        bail!("ci workflow must include a 'support-policy-check' job");
    }
    if !workflow.contains("cargo xtask ci --stage support-policy-check") {
        bail!("ci workflow must execute `cargo xtask ci --stage support-policy-check`");
    }

    Ok(())
}

fn run_release_scorecard_check() -> Result<()> {
    run_sdk_perf_budget_check()?;
    run_sdk_soak_check()?;
    run_supply_chain_check()?;
    run("bash", &["tools/scripts/release-scorecard.sh"])?;

    let markdown_path = "target/release-scorecard/release-scorecard.md";
    let json_path = "target/release-scorecard/release-scorecard.json";
    let markdown = fs::read_to_string(markdown_path)
        .with_context(|| format!("missing generated scorecard markdown at {markdown_path}"))?;
    let json = fs::read_to_string(json_path)
        .with_context(|| format!("missing generated scorecard json at {json_path}"))?;

    for marker in ["# Release Scorecard", "| Overall status |", "| Performance budget status |"] {
        if !markdown.contains(marker) {
            bail!("generated scorecard missing marker '{marker}' in {markdown_path}");
        }
    }
    for marker in ["\"overall_status\"", "\"performance_status\"", "\"soak_status\""] {
        if !json.contains(marker) {
            bail!("generated scorecard json missing marker '{marker}' in {json_path}");
        }
    }

    Ok(())
}

fn run_extension_registry_check() -> Result<()> {
    let registry = fs::read_to_string(EXTENSION_REGISTRY_PATH)
        .with_context(|| format!("missing {EXTENSION_REGISTRY_PATH}"))?;
    for marker in [
        "# Protocol Extension Registry",
        "## Namespace Rules",
        "## Registry Entries",
        "| Extension ID | Scope | Status | Owner | Introduced in | Notes |",
        "`rpc.`",
        "`payload.`",
        "`event.`",
        "`domain.`",
    ] {
        if !registry.contains(marker) {
            bail!("extension registry missing marker '{marker}' in {EXTENSION_REGISTRY_PATH}");
        }
    }

    let active_rows =
        registry.lines().filter(|line| line.contains("| `") && line.contains("| active |")).count();
    if active_rows < 4 {
        bail!("extension registry requires at least 4 active entries, found {active_rows}");
    }

    let rpc_contract = fs::read_to_string(RPC_CONTRACT_PATH)
        .with_context(|| format!("missing {RPC_CONTRACT_PATH}"))?;
    if !rpc_contract.contains("docs/contracts/extension-registry.md") {
        bail!("rpc contract must reference docs/contracts/extension-registry.md");
    }

    let payload_contract = fs::read_to_string(PAYLOAD_CONTRACT_PATH)
        .with_context(|| format!("missing {PAYLOAD_CONTRACT_PATH}"))?;
    if !payload_contract.contains("docs/contracts/extension-registry.md") {
        bail!("payload contract must reference docs/contracts/extension-registry.md");
    }

    let adr = fs::read_to_string(EXTENSION_REGISTRY_ADR_PATH)
        .with_context(|| format!("missing {EXTENSION_REGISTRY_ADR_PATH}"))?;
    if !adr.contains("ADR 0005") {
        bail!("extension registry ADR must include identifier ADR 0005");
    }

    let workflow = fs::read_to_string(CI_WORKFLOW_PATH)
        .with_context(|| format!("missing {CI_WORKFLOW_PATH}"))?;
    if !workflow.contains("extension-registry-check:") {
        bail!("ci workflow must include an 'extension-registry-check' job");
    }
    if !workflow.contains("cargo xtask ci --stage extension-registry-check") {
        bail!("ci workflow must execute `cargo xtask ci --stage extension-registry-check`");
    }

    Ok(())
}

fn run_leader_readiness_check() -> Result<()> {
    run_ci(None)?;

    let scorecard_json = fs::read_to_string("target/release-scorecard/release-scorecard.json")
        .context("missing target/release-scorecard/release-scorecard.json after full CI run")?;
    let scorecard: serde_json::Value =
        serde_json::from_str(&scorecard_json).context("invalid release scorecard json")?;
    let overall_status =
        scorecard.get("overall_status").and_then(|value| value.as_str()).unwrap_or("UNKNOWN");
    if overall_status != "PASS" {
        bail!("leader readiness requires scorecard overall_status=PASS, found '{overall_status}'");
    }

    let soak_json = fs::read_to_string(SOAK_REPORT_PATH)
        .with_context(|| format!("missing {SOAK_REPORT_PATH} after full CI run"))?;
    let soak: serde_json::Value =
        serde_json::from_str(&soak_json).context("invalid soak report json")?;
    let soak_status = soak.get("status").and_then(|value| value.as_str()).unwrap_or("unknown");
    if soak_status != "pass" {
        bail!("leader readiness requires soak status=pass, found '{soak_status}'");
    }

    let compatibility_matrix = fs::read_to_string(INTEROP_MATRIX_PATH)
        .with_context(|| format!("missing {INTEROP_MATRIX_PATH}"))?;
    for client in ["Sideband", "RCH", "Columba"] {
        if !compatibility_matrix.contains(client) {
            bail!("compatibility matrix missing required client row '{client}'");
        }
    }

    let git_commit = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_string());

    if let Some(parent) = Path::new(LEADER_READINESS_REPORT_PATH).parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("failed to create leader readiness report directory {}", parent.display())
        })?;
    }

    let report = format!(
        "# Leader-Grade Readiness Certification\n\n\
Generated by `cargo run -p xtask -- leader-readiness-check`.\n\n\
- commit: `{git_commit}`\n\
- ci_full_run: `PASS`\n\
- scorecard_overall_status: `{overall_status}`\n\
- soak_status: `{soak_status}`\n\
- compatibility_clients_checked: `Sideband`, `RCH`, `Columba`\n\
- security_review_source: `{SECURITY_REVIEW_CHECKLIST_PATH}`\n\
- compatibility_matrix_source: `{INTEROP_MATRIX_PATH}`\n\n\
This report certifies that full CI, compatibility checks, and release scorecard\n\
inputs are aligned for leader-grade release readiness.\n"
    );
    fs::write(LEADER_READINESS_REPORT_PATH, report)
        .with_context(|| format!("failed to write {LEADER_READINESS_REPORT_PATH}"))?;

    Ok(())
}

fn run_security_review_check() -> Result<()> {
    let threat_model = fs::read_to_string(SECURITY_THREAT_MODEL_PATH)
        .with_context(|| format!("missing {SECURITY_THREAT_MODEL_PATH}"))?;
    for marker in [
        "## STRIDE Threat Inventory",
        "| Spoofing |",
        "| Tampering |",
        "| Repudiation |",
        "| Information Disclosure |",
        "| Denial of Service |",
        "| Elevation of Privilege |",
        "## Mitigation Map",
    ] {
        if !threat_model.contains(marker) {
            bail!(
                "security threat model missing required marker '{marker}' in {SECURITY_THREAT_MODEL_PATH}"
            );
        }
    }

    let checklist = fs::read_to_string(SECURITY_REVIEW_CHECKLIST_PATH)
        .with_context(|| format!("missing {SECURITY_REVIEW_CHECKLIST_PATH}"))?;
    if !checklist.contains("## Checklist") {
        bail!(
            "security review checklist missing `## Checklist` heading in {SECURITY_REVIEW_CHECKLIST_PATH}"
        );
    }
    if checklist.contains("| FAIL |") || checklist.contains("| TODO |") {
        bail!(
            "security review checklist contains non-pass statuses in {SECURITY_REVIEW_CHECKLIST_PATH}"
        );
    }
    let pass_rows = checklist.lines().filter(|line| line.contains("| PASS |")).count();
    if pass_rows < 6 {
        bail!(
            "security review checklist requires at least 6 PASS controls in {SECURITY_REVIEW_CHECKLIST_PATH}"
        );
    }
    Ok(())
}

fn run_sdk_security_check() -> Result<()> {
    run("cargo", &["test", "-p", "rns-rpc", "sdk_security", "--", "--nocapture"])
}

fn run_sdk_fuzz_check() -> Result<()> {
    run("cargo", &["check", "--manifest-path", "crates/libs/rns-rpc/fuzz/Cargo.toml"])?;
    run("cargo", &["check", "--manifest-path", "crates/libs/lxmf-sdk/fuzz/Cargo.toml"])?;
    run(
        "cargo",
        &[
            "test",
            "-p",
            "rns-rpc",
            "fuzz_smoke_rpc_frame_and_http_parsers_do_not_panic",
            "--",
            "--nocapture",
        ],
    )?;
    run(
        "cargo",
        &[
            "test",
            "-p",
            "lxmf-sdk",
            "fuzz_smoke_sdk_json_decoders_do_not_panic",
            "--",
            "--nocapture",
        ],
    )
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

fn run_sdk_race_check() -> Result<()> {
    run("cargo", &["test", "-p", "lxmf-sdk", "race_idempot", "--", "--nocapture"])?;
    run("cargo", &["test", "-p", "rns-rpc", "sdk_race", "--", "--nocapture"])
}

fn run_sdk_replay_check() -> Result<()> {
    run(
        "cargo",
        &[
            "test",
            "-p",
            "rns-rpc",
            "replay_fixture_trace_executes_successfully",
            "--",
            "--nocapture",
        ],
    )?;
    run(
        "cargo",
        &[
            "run",
            "-p",
            "rns-tools",
            "--bin",
            "rnx",
            "--",
            "replay",
            "--trace",
            "docs/fixtures/sdk-v2/rpc/replay_known_send_cancel.v1.json",
        ],
    )
}

fn run_sdk_metrics_check() -> Result<()> {
    run("cargo", &["test", "-p", "rns-rpc", "rpc::http::tests", "--", "--nocapture"])
}

fn run_sdk_bench_check() -> Result<()> {
    run(
        "cargo",
        &[
            "bench",
            "-p",
            "lxmf-core",
            "--bench",
            "core_message_paths",
            "--",
            "--sample-size",
            "10",
            "--warm-up-time",
            "0.1",
            "--measurement-time",
            "0.2",
        ],
    )?;
    run(
        "cargo",
        &[
            "bench",
            "-p",
            "lxmf-sdk",
            "--bench",
            "sdk_client_paths",
            "--",
            "--sample-size",
            "10",
            "--warm-up-time",
            "0.1",
            "--measurement-time",
            "0.2",
        ],
    )?;
    run(
        "cargo",
        &[
            "bench",
            "-p",
            "rns-rpc",
            "--bench",
            "rpc_hotpaths",
            "--",
            "--sample-size",
            "10",
            "--warm-up-time",
            "0.1",
            "--measurement-time",
            "0.2",
        ],
    )?;
    write_bench_summary()
}

#[derive(Debug, Deserialize)]
struct CriterionSample {
    iters: Vec<f64>,
    times: Vec<f64>,
}

fn run_sdk_perf_budget_check() -> Result<()> {
    run_sdk_bench_check()?;
    if let Err(first_err) = evaluate_perf_budgets() {
        eprintln!(
            "initial performance budget evaluation failed ({first_err:#}); retrying benchmarks once"
        );
        run_sdk_bench_check()?;
        return evaluate_perf_budgets().with_context(|| {
            format!("performance budgets still failing after retry: {first_err:#}")
        });
    }
    Ok(())
}

fn evaluate_perf_budgets() -> Result<()> {
    let criterion_root = Path::new("target/criterion");
    let mut report_lines = Vec::new();
    report_lines.push("# SDK Perf Budget Report".to_string());
    report_lines.push(String::new());
    let mut failures = Vec::new();

    for budget in PERF_BUDGETS {
        let sample_path = criterion_root.join(budget.benchmark).join("new").join("sample.json");
        let raw = fs::read_to_string(&sample_path)
            .with_context(|| format!("read sample data {}", sample_path.display()))?;
        let sample: CriterionSample = serde_json::from_str(&raw)
            .with_context(|| format!("parse {}", sample_path.display()))?;
        if sample.iters.len() != sample.times.len() || sample.iters.is_empty() {
            bail!("invalid sample data in {}", sample_path.display());
        }

        let mut latency_ns = sample
            .times
            .iter()
            .zip(sample.iters.iter())
            .filter_map(|(time, iters)| (*iters > 0.0).then_some(*time / *iters))
            .collect::<Vec<_>>();
        if latency_ns.is_empty() {
            bail!("sample data contains zero iteration counts in {}", sample_path.display());
        }
        latency_ns.sort_by(f64::total_cmp);
        let tail_latencies = trimmed_tail_sample(&latency_ns);

        let p50 = percentile(&latency_ns, 0.50);
        let p95 = percentile(&tail_latencies, 0.95);
        let p99 = percentile(&tail_latencies, 0.99);
        let throughput = 1_000_000_000.0 / p50.max(1.0);

        report_lines.push(format!(
            "- `{}` p50_ns={:.2} p95_ns={:.2} p99_ns={:.2} throughput_ops_per_sec={:.2}",
            budget.benchmark, p50, p95, p99, throughput
        ));

        if p50 > budget.max_p50_ns {
            failures.push(format!(
                "{} exceeded p50 budget ({:.2} > {:.2})",
                budget.benchmark, p50, budget.max_p50_ns
            ));
        }
        if p95 > budget.max_p95_ns {
            failures.push(format!(
                "{} exceeded p95 budget ({:.2} > {:.2})",
                budget.benchmark, p95, budget.max_p95_ns
            ));
        }
        if p99 > budget.max_p99_ns {
            failures.push(format!(
                "{} exceeded p99 budget ({:.2} > {:.2})",
                budget.benchmark, p99, budget.max_p99_ns
            ));
        }
        if throughput < budget.min_throughput_ops_per_sec {
            failures.push(format!(
                "{} throughput below budget ({:.2} < {:.2})",
                budget.benchmark, throughput, budget.min_throughput_ops_per_sec
            ));
        }
    }

    report_lines.push(String::new());
    if failures.is_empty() {
        report_lines.push("Status: PASS".to_string());
    } else {
        report_lines.push("Status: FAIL".to_string());
        report_lines.extend(failures.iter().map(|entry| format!("- {entry}")));
    }
    fs::write(PERF_BUDGET_REPORT_PATH, report_lines.join("\n"))
        .with_context(|| format!("write {PERF_BUDGET_REPORT_PATH}"))?;
    println!("performance budget report written to {PERF_BUDGET_REPORT_PATH}");

    if failures.is_empty() {
        Ok(())
    } else {
        bail!("performance budget regressions detected: {}", failures.join("; "));
    }
}

fn percentile(values: &[f64], p: f64) -> f64 {
    let index = ((values.len() as f64 - 1.0) * p).round() as usize;
    values[index.min(values.len() - 1)]
}

fn trimmed_tail_sample(values: &[f64]) -> Vec<f64> {
    if values.len() < 8 {
        return values.to_vec();
    }
    let trim = (values.len() / 20).max(1);
    if values.len() <= trim * 2 {
        return values.to_vec();
    }
    values[trim..values.len() - trim].to_vec()
}

fn run_sdk_memory_budget_check() -> Result<()> {
    run("cargo", &["test", "-p", "test-support", "sdk_memory_budget", "--", "--nocapture"])
}

fn write_bench_summary() -> Result<()> {
    let criterion_root = Path::new("target/criterion");
    if !criterion_root.exists() {
        bail!("criterion output is missing at {}", criterion_root.display());
    }

    let mut estimate_files = Vec::new();
    collect_estimate_files(criterion_root, &mut estimate_files)?;
    if estimate_files.is_empty() {
        bail!("no benchmark estimate files were generated under {}", criterion_root.display());
    }
    estimate_files.sort();

    let mut lines = Vec::new();
    lines.push("# SDK Benchmark Summary".to_string());
    lines.push(String::new());
    for path in estimate_files {
        let rel = path.strip_prefix(criterion_root).unwrap_or(path.as_path());
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("read benchmark estimate file {}", path.display()))?;
        let parsed: serde_json::Value =
            serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
        let mean_ns = parsed
            .get("mean")
            .and_then(|value| value.get("point_estimate"))
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0);
        let median_ns = parsed
            .get("median")
            .and_then(|value| value.get("point_estimate"))
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0);
        lines.push(format!(
            "- `{}` mean_ns={:.2} median_ns={:.2}",
            rel.display(),
            mean_ns,
            median_ns
        ));
    }
    lines.push(String::new());
    lines.push("Generated by `cargo run -p xtask -- sdk-bench-check`.".to_string());

    fs::write(BENCH_SUMMARY_PATH, lines.join("\n"))
        .with_context(|| format!("write {BENCH_SUMMARY_PATH}"))?;
    println!("benchmark summary written to {BENCH_SUMMARY_PATH}");
    Ok(())
}

fn collect_estimate_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_estimate_files(&path, out)?;
            continue;
        }
        if path.file_name().and_then(|name| name.to_str()) == Some("estimates.json") {
            out.push(path);
        }
    }
    Ok(())
}

fn run_sdk_queue_pressure_check() -> Result<()> {
    run(
        "cargo",
        &[
            "test",
            "-p",
            "rns-rpc",
            "sdk_event_queues_remain_bounded_under_sustained_load",
            "--",
            "--nocapture",
        ],
    )
}

#[derive(Debug, Serialize)]
struct SupplyChainProvenance {
    schema_version: u32,
    generated_at_unix_secs: u64,
    git_commit: String,
    rustc_version: String,
    cargo_version: String,
    lockfile_sha256: String,
    artifacts: Vec<SupplyChainArtifact>,
}

#[derive(Debug, Serialize)]
struct SupplyChainArtifact {
    name: String,
    path: String,
    bytes: u64,
    sha256: String,
}

fn run_supply_chain_check() -> Result<()> {
    let metadata_output = Command::new("cargo")
        .args(["metadata", "--locked", "--format-version", "1"])
        .output()
        .context("run cargo metadata for sbom export")?;
    if !metadata_output.status.success() {
        let stderr = String::from_utf8_lossy(&metadata_output.stderr);
        bail!("cargo metadata failed for sbom export: {stderr}");
    }
    write_bytes(SUPPLY_CHAIN_SBOM_PATH, &metadata_output.stdout)?;

    run("cargo", &["build", "--release", "--workspace", "--bins"])?;

    let lockfile = fs::read("Cargo.lock").context("read Cargo.lock for provenance digest")?;
    let lockfile_sha256 = sha256_hex(&lockfile);
    let git_commit = capture_command_stdout("git", &["rev-parse", "HEAD"])?;
    let rustc_version = capture_command_stdout("rustc", &["--version"])?;
    let cargo_version = capture_command_stdout("cargo", &["--version"])?;
    let generated_at_unix_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);

    let mut artifacts = Vec::with_capacity(RELEASE_BINARIES.len());
    for name in RELEASE_BINARIES {
        let path = Path::new("target/release").join(name);
        if !path.exists() {
            bail!("release artifact missing: {}", path.display());
        }
        let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        artifacts.push(SupplyChainArtifact {
            name: (*name).to_string(),
            path: path.to_string_lossy().replace('\\', "/"),
            bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            sha256: sha256_hex(&bytes),
        });
    }

    let provenance = SupplyChainProvenance {
        schema_version: 1,
        generated_at_unix_secs,
        git_commit,
        rustc_version,
        cargo_version,
        lockfile_sha256,
        artifacts,
    };
    let bytes = serde_json::to_vec_pretty(&provenance).context("serialize supply-chain report")?;
    write_bytes(SUPPLY_CHAIN_PROVENANCE_PATH, &bytes)?;
    let digest = sha256_hex(&bytes);
    let provenance_name = Path::new(SUPPLY_CHAIN_PROVENANCE_PATH)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            anyhow::anyhow!("invalid provenance path: {SUPPLY_CHAIN_PROVENANCE_PATH}")
        })?;
    let signature_payload = format!("{digest}  {provenance_name}\n");
    write_bytes(SUPPLY_CHAIN_SIGNATURE_PATH, signature_payload.as_bytes())?;
    Ok(())
}

fn run_reproducible_build_check() -> Result<()> {
    run("bash", &["tools/scripts/reproducible-build-check.sh"])?;
    if !Path::new(REPRODUCIBLE_BUILD_REPORT_PATH).exists() {
        bail!("reproducible build report is missing at {REPRODUCIBLE_BUILD_REPORT_PATH}");
    }
    Ok(())
}

fn write_bytes(path: &str, bytes: &[u8]) -> Result<()> {
    let path = Path::new(path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(path, bytes).with_context(|| format!("write {}", path.display()))
}

fn capture_command_stdout(command: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(command)
        .args(args)
        .output()
        .with_context(|| format!("run {command} {}", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{command} {} failed: {stderr}", args.join(" "));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn run_sdk_matrix_check() -> Result<()> {
    run("cargo", &["test", "-p", "test-support", "sdk_matrix", "--", "--nocapture"])
}

fn run_embedded_link_check() -> Result<()> {
    run("cargo", &["test", "-p", "rns-transport", "--test", "embedded_link_contract", "--no-run"])?;

    let backends = fs::read_to_string("docs/contracts/sdk-v2-backends.md")
        .context("missing docs/contracts/sdk-v2-backends.md")?;
    for marker in [
        "## Embedded Link Adapter Contract",
        "EmbeddedLinkAdapter",
        "send_frame",
        "poll_frame",
        "FrameTooLarge",
    ] {
        if !backends.contains(marker) {
            bail!("backend contract missing embedded-link marker '{marker}'");
        }
    }

    let rpc_contract = fs::read_to_string(RPC_CONTRACT_PATH)
        .with_context(|| format!("missing {RPC_CONTRACT_PATH}"))?;
    if !rpc_contract.contains("Embedded link adapters (serial/BLE/LoRa)") {
        bail!("rpc contract must document embedded link adapter compatibility note");
    }

    Ok(())
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

fn run_compat_kit_check() -> Result<()> {
    run("bash", &["tools/scripts/compatibility-kit.sh", "--dry-run"])
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
    run_forbidden_deps()?;
    run_module_size_check()
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

fn run_module_size_check() -> Result<()> {
    run("bash", &["tools/scripts/check-module-size.sh"])
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
