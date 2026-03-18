use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use lxmf_core::Message;
use rand_core::OsRng;
use rns_core::destination::{DestinationAnnounce, DestinationName, SingleInputDestination};
use rns_core::identity::{lxmf_sign, lxmf_verify, PrivateIdentity};
use rns_core::ratchets::{
    decrypt_with_identity_into, encrypt_for_public_key, encrypt_for_public_key_into,
};
use rns_transport::destination::link::{Link, LinkHandleResult};
use rns_transport::destination::{DestinationDesc, DestinationName as TransportDestinationName};
use rns_transport::hash::AddressHash;
use rns_transport::identity_bridge::to_transport_private_identity;
use rns_transport::packet::{Packet, PacketDataBuffer, PACKET_MDU};
use rns_transport::resource::ResourceManager;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

mod client_codegen;

const INTEROP_BASELINE_PATH: &str = "docs/contracts/baselines/interop-artifacts-manifest.json";
const INTEROP_DRIFT_BASELINE_PATH: &str = "docs/contracts/baselines/interop-drift-baseline.json";
const INTEROP_MATRIX_PATH: &str = "docs/contracts/compatibility-matrix.md";
const SUPPORT_POLICY_PATH: &str = "docs/contracts/support-policy.md";
const SDK_API_STABILITY_PATH: &str = "docs/contracts/sdk-v2-api-stability.md";
const SDK_BACKENDS_CONTRACT_PATH: &str = "docs/contracts/sdk-v2-backends.md";
const SDK_FEATURE_MATRIX_PATH: &str = "docs/contracts/sdk-v2-feature-matrix.md";
const SCHEMA_CLIENT_MANIFEST_PATH: &str =
    "docs/schemas/sdk/v2/clients/client-generation-manifest.json";
const EXTENSION_REGISTRY_PATH: &str = "docs/contracts/extension-registry.md";
const EXTENSION_REGISTRY_ADR_PATH: &str = "docs/adr/0005-extension-registry-governance.md";
const UNSAFE_POLICY_PATH: &str = "docs/architecture/unsafe-code-policy.md";
const UNSAFE_INVENTORY_PATH: &str = "docs/architecture/unsafe-inventory.md";
const UNSAFE_GOVERNANCE_ADR_PATH: &str = "docs/adr/0006-unsafe-code-audit-governance.md";
const UNSAFE_AUDIT_SCRIPT_PATH: &str = "tools/scripts/check-unsafe.sh";
const ARCH_BOUNDARY_REPORT_PATH: &str = "target/architecture/boundary-report.txt";
const INTEROP_CORPUS_PATH: &str = "docs/fixtures/interop/v1/golden-corpus.json";
const RPC_CONTRACT_PATH: &str = "docs/contracts/rpc-contract.md";
const PAYLOAD_CONTRACT_PATH: &str = "docs/contracts/payload-contract.md";
const CODEOWNERS_PATH: &str = ".github/CODEOWNERS";
const SECURITY_POLICY_DOC_PATH: &str = ".github/SECURITY.md";
const SECURITY_THREAT_MODEL_PATH: &str = "docs/adr/0004-sdk-v25-threat-model.md";
const CRYPTO_AGILITY_ADR_PATH: &str = "docs/adr/0007-crypto-agility-roadmap.md";
const SECURITY_REVIEW_CHECKLIST_PATH: &str = "docs/runbooks/security-review-checklist.md";
const SDK_DOCS_CHECKLIST_PATH: &str = "docs/runbooks/sdk-docs-checklist.md";
const COMPLIANCE_PROFILES_RUNBOOK_PATH: &str = "docs/runbooks/compliance-profiles.md";
const REFERENCE_INTEGRATIONS_RUNBOOK_PATH: &str = "docs/runbooks/reference-integrations.md";
const CVE_RESPONSE_RUNBOOK_PATH: &str = "docs/runbooks/cve-response-workflow.md";
const INCIDENT_RUNBOOK_PATH: &str = "docs/runbooks/incident-response-playbooks.md";
const DISASTER_RECOVERY_RUNBOOK_PATH: &str = "docs/runbooks/disaster-recovery-drills.md";
const EMBEDDED_HIL_RUNBOOK_PATH: &str = "docs/runbooks/embedded-hil-esp32.md";
const EMBEDDED_NATIVE_LOCKFILE_PATH: &str = "docs/contracts/native-embedded-lockfile.toml";
const EMBEDDED_NATIVE_INTEROP_PROFILE_PATH: &str =
    "docs/contracts/native-embedded-interop-profile-v1.md";
const EMBEDDED_NATIVE_LAB_PROFILE_PATH: &str = "docs/contracts/native-embedded-lab-profile-v1.md";
const EMBEDDED_NATIVE_NODE_CONFIG_PATH: &str = "docs/contracts/native-embedded-node-config-v1.md";
const BLE_CAMERA_WIRE_CONTRACT_PATH: &str = "docs/contracts/ble-camera-wire-v1.md";
const BLE_TRANSPORT_RUNTIME_CONTRACT_PATH: &str =
    "docs/contracts/ble-transport-runtime-contract.md";
const EMBEDDED_NATIVE_WORKFLOW_PATH: &str = ".github/workflows/nightly-embedded-hil.yml";
const BACKUP_RESTORE_DRILL_SCRIPT_PATH: &str = "tools/scripts/backup-restore-drill.sh";
const REFERENCE_INTEGRATIONS_SMOKE_SCRIPT_PATH: &str =
    "tools/scripts/reference-integrations-smoke.sh";
const CERTIFICATION_REPORT_SCRIPT_PATH: &str = "tools/scripts/certification-report.sh";
const SOAK_REPORT_PATH: &str = "target/soak/soak-report.json";
const BENCH_SUMMARY_PATH: &str = "target/criterion/bench-summary.txt";
const PERF_BUDGET_REPORT_PATH: &str = "target/criterion/bench-budget-report.txt";
const PYTHON_IMPL_BENCH_CONFIG_PATH: &str = "tools/benchmarks/python_impl.toml";
const PYTHON_IMPL_BENCH_REPORT_PATH: &str = "target/criterion/python-impl-benchmarks.json";
const PYTHON_IMPL_COMPARE_REPORT_PATH: &str = "target/criterion/python-impl-compare.txt";
const PYTHON_IMPL_COMPARE_JSON_PATH: &str = "target/criterion/python-impl-compare.json";
const PYTHON_IMPL_ENVIRONMENT_PATH: &str = "target/criterion/python-impl-environment.json";
const PYTHON_IMPL_REPORT_DIR: &str = "target/criterion/python-impl-report";
const PYTHON_IMPL_REPORT_JSON_PATH: &str = "target/criterion/python-impl-report/report.json";
const PYTHON_IMPL_REPORT_TEXT_PATH: &str = "target/criterion/python-impl-report/report.txt";
const SUPPLY_CHAIN_SBOM_PATH: &str = "target/supply-chain/sbom/cargo-metadata.sbom.json";
const SUPPLY_CHAIN_PROVENANCE_PATH: &str =
    "target/supply-chain/provenance/artifact-provenance.json";
const SUPPLY_CHAIN_SIGNATURE_PATH: &str =
    "target/supply-chain/provenance/artifact-provenance.sha256";
const REPRODUCIBLE_BUILD_REPORT_PATH: &str =
    "target/supply-chain/reproducible/reproducible-build-report.txt";
const RELEASE_BUNDLE_OUTPUT_DIR: &str = "target/release-bundles";
const DAEMON_RELEASE_BINARIES: &[(&str, &str)] =
    &[("lxmf-cli", "lxmd"), ("reticulumd", "reticulumd")];
const CARGO_AUDIT_IGNORE_ADVISORIES: &[&str] =
    &["RUSTSEC-2024-0421", "RUSTSEC-2024-0436", "RUSTSEC-2026-0009", "RUSTSEC-2025-0134"];
const SCHEMA_CLIENT_SMOKE_REPORT_PATH: &str = "target/interop/schema-client-smoke-report.txt";
const CERTIFICATION_REPORT_PATH: &str = "target/release-readiness/certification-report.md";
const CERTIFICATION_REPORT_JSON_PATH: &str = "target/release-readiness/certification-report.json";
const EMBEDDED_FOOTPRINT_REPORT_PATH: &str = "target/embedded/footprint-report.txt";
const EMBEDDED_HIL_REPORT_PATH: &str = "target/hil/esp32-smoke-report.json";
const EMBEDDED_NATIVE_INTEROP_REPORT_PATH: &str = "target/hil/native-node-report.json";
const EMBEDDED_NATIVE_INTEROP_LOG_PATH: &str = "target/hil/native-node.log";
const EMBEDDED_NATIVE_INTEROP_SCRIPT_PATH: &str = "tools/scripts/embedded-native-interop-smoke.sh";
const LEADER_READINESS_REPORT_PATH: &str = "target/release-readiness/leader-grade-readiness.md";
const CANARY_CRITERIA_REPORT_PATH: &str = "target/release-readiness/canary-criteria-report.md";
const CANARY_CRITERIA_REPORT_JSON_PATH: &str =
    "target/release-readiness/canary-criteria-report.json";
const GENERATED_MIGRATION_NOTES_PATH: &str =
    "target/release-readiness/generated-migration-notes.md";
const PROTO_ROOT_PATH: &str = "api/proto";
const GENERATED_GRPC_RUST_DIR: &str = "generated/grpc/rust";
const GENERATED_GRPC_DESCRIPTOR_PATH: &str = "generated/grpc/lxmf-descriptor-set.bin";

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
    "/.github/SECURITY.md",
    "/docs/contracts/",
    "/docs/schemas/",
    "/docs/migrations/",
    "/docs/runbooks/",
    "/docs/architecture/unsafe-code-policy.md",
    "/docs/architecture/unsafe-inventory.md",
    "/docs/adr/0006-unsafe-code-audit-governance.md",
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
    "/tools/scripts/check-unsafe.sh",
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
        min_throughput_ops_per_sec: 17_500_000.0,
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
        min_throughput_ops_per_sec: 10_000.0,
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
    PackageDaemonBundle {
        #[arg(long)]
        version: Option<String>,
    },
    ApiDiff,
    Licenses,
    MigrationChecks,
    ArchitectureLintCheck,
    ArchitectureChecks,
    ForbiddenDeps,
    CorrectnessCheck,
    SdkConformance,
    SdkSchemaCheck,
    SdkDocsCheck,
    SdkCookbookCheck,
    SdkErgonomicsCheck,
    LxmfCliCheck,
    ReferenceIntegrationCheck,
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
    SchemaClientCheck,
    SchemaClientGenerate {
        #[arg(long)]
        check: bool,
    },
    ProtoCheck,
    ProtoGenerate,
    CompatKitCheck,
    E2eCompatibility,
    MeshSim,
    SdkProfileBuild,
    SdkExamplesCheck,
    SdkApiBreak,
    SdkMigrationCheck,
    ChangelogMigrationCheck,
    GovernanceCheck,
    ComplianceProfileCheck,
    SupportPolicyCheck,
    UnsafeAuditCheck,
    CanaryCriteriaCheck,
    ReleaseScorecardCheck,
    ExtensionRegistryCheck,
    PluginNegotiationCheck,
    CertificationReportCheck,
    LeaderReadinessCheck,
    SecurityReviewCheck,
    CryptoAgilityCheck,
    KeyManagementCheck,
    SdkSecurityCheck,
    SdkFuzzCheck,
    SdkPropertyCheck,
    SdkModelCheck,
    SdkRaceCheck,
    SdkReplayCheck,
    SdkMetricsCheck,
    SdkBenchCheck,
    SdkPerfBudgetCheck,
    PythonImplBenchCompare {
        #[arg(long, value_enum, default_value_t = PythonImplBenchProfile::Fast)]
        profile: PythonImplBenchProfile,
    },
    PythonImplBenchReport {
        #[arg(long)]
        compare_runs: Option<usize>,
        #[arg(long)]
        resource_runs: Option<usize>,
        #[arg(long)]
        resource_iterations: Option<usize>,
    },
    #[command(hide = true)]
    PythonImplBenchWorkload {
        #[arg(long, value_enum)]
        implementation: PythonImplImplementation,
        #[arg(long)]
        benchmark: String,
        #[arg(long)]
        iterations: usize,
        #[arg(long)]
        output: PathBuf,
    },
    SdkMemoryBudgetCheck,
    SdkQueuePressureCheck,
    SupplyChainCheck,
    ReproducibleBuildCheck,
    SdkMatrixCheck,
    InterfacesRequired,
    EmbeddedLinkCheck,
    EmbeddedNativeLockCheck,
    EmbeddedCoreCheck,
    EmbeddedFootprintCheck,
    EmbeddedHilCheck,
    EmbeddedNodeBuild,
    EmbeddedNodeContract,
    EmbeddedNodeFailureMatrix,
    EmbeddedNodeHil,
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
    ReferenceIntegrationCheck,
    DxBootstrapCheck,
    SdkIncidentRunbookCheck,
    SdkDrillCheck,
    SdkSoakCheck,
    InteropArtifacts,
    InteropMatrixCheck,
    InteropCorpusCheck,
    InteropDriftCheck,
    SchemaClientCheck,
    ProtoCheck,
    CompatKitCheck,
    E2eCompatibility,
    SdkProfileBuild,
    SdkExamplesCheck,
    SdkApiBreak,
    SdkMigrationCheck,
    ChangelogMigrationCheck,
    GovernanceCheck,
    ComplianceProfileCheck,
    SupportPolicyCheck,
    UnsafeAuditCheck,
    CanaryCriteriaCheck,
    ReleaseScorecardCheck,
    ExtensionRegistryCheck,
    PluginNegotiationCheck,
    CertificationReportCheck,
    LeaderReadinessCheck,
    SecurityReviewCheck,
    CryptoAgilityCheck,
    KeyManagementCheck,
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
    InterfacesRequired,
    EmbeddedLinkCheck,
    EmbeddedNativeLockCheck,
    EmbeddedCoreCheck,
    EmbeddedFootprintCheck,
    EmbeddedHilCheck,
    EmbeddedNodeBuild,
    EmbeddedNodeContract,
    EmbeddedNodeFailureMatrix,
    EmbeddedNodeHil,
    Correctness,
    MigrationChecks,
    ArchitectureLint,
    ArchitectureChecks,
    ForbiddenDeps,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum PythonImplBenchProfile {
    Fast,
    Report,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum PythonImplImplementation {
    Rust,
    Python,
}

fn main() -> Result<()> {
    let xtask = Xtask::parse();
    match xtask.command {
        XtaskCommand::Ci { stage } => run_ci(stage),
        XtaskCommand::ReleaseCheck => run_release_check(),
        XtaskCommand::PackageDaemonBundle { version } => run_package_daemon_bundle(version),
        XtaskCommand::ApiDiff => run_api_diff(),
        XtaskCommand::Licenses => run_licenses(),
        XtaskCommand::MigrationChecks => run_migration_checks(),
        XtaskCommand::ArchitectureLintCheck => run_architecture_lint_check(),
        XtaskCommand::ArchitectureChecks => run_architecture_checks(),
        XtaskCommand::ForbiddenDeps => run_forbidden_deps(),
        XtaskCommand::CorrectnessCheck => run_correctness_check(),
        XtaskCommand::SdkConformance => run_sdk_conformance(),
        XtaskCommand::SdkSchemaCheck => run_sdk_schema_check(),
        XtaskCommand::SdkDocsCheck => run_sdk_docs_check(),
        XtaskCommand::SdkCookbookCheck => run_sdk_cookbook_check(),
        XtaskCommand::SdkErgonomicsCheck => run_sdk_ergonomics_check(),
        XtaskCommand::LxmfCliCheck => run_lxmf_cli_check(),
        XtaskCommand::ReferenceIntegrationCheck => run_reference_integration_check(),
        XtaskCommand::DxBootstrapCheck => run_dx_bootstrap_check(),
        XtaskCommand::SdkIncidentRunbookCheck => run_sdk_incident_runbook_check(),
        XtaskCommand::SdkDrillCheck => run_sdk_drill_check(),
        XtaskCommand::SdkSoakCheck => run_sdk_soak_check(),
        XtaskCommand::InteropArtifacts { update } => run_interop_artifacts(update),
        XtaskCommand::InteropMatrixCheck => run_interop_matrix_check(),
        XtaskCommand::InteropCorpusCheck => run_interop_corpus_check(),
        XtaskCommand::InteropDriftCheck { update } => run_interop_drift_check(update),
        XtaskCommand::SchemaClientCheck => run_schema_client_check(),
        XtaskCommand::SchemaClientGenerate { check } => {
            run_schema_client_generate(check).map(|_| ())
        }
        XtaskCommand::ProtoCheck => run_proto_check(),
        XtaskCommand::ProtoGenerate => run_proto_generate(),
        XtaskCommand::CompatKitCheck => run_compat_kit_check(),
        XtaskCommand::E2eCompatibility => run_e2e_compatibility(),
        XtaskCommand::MeshSim => run_mesh_sim(),
        XtaskCommand::SdkProfileBuild => run_sdk_profile_build(),
        XtaskCommand::SdkExamplesCheck => run_sdk_examples_check(),
        XtaskCommand::SdkApiBreak => run_sdk_api_break(),
        XtaskCommand::SdkMigrationCheck => run_sdk_migration_check(),
        XtaskCommand::ChangelogMigrationCheck => run_changelog_migration_check(),
        XtaskCommand::GovernanceCheck => run_governance_check(),
        XtaskCommand::ComplianceProfileCheck => run_compliance_profile_check(),
        XtaskCommand::SupportPolicyCheck => run_support_policy_check(),
        XtaskCommand::UnsafeAuditCheck => run_unsafe_audit_check(),
        XtaskCommand::CanaryCriteriaCheck => run_canary_criteria_check(),
        XtaskCommand::ReleaseScorecardCheck => run_release_scorecard_check(),
        XtaskCommand::ExtensionRegistryCheck => run_extension_registry_check(),
        XtaskCommand::PluginNegotiationCheck => run_plugin_negotiation_check(),
        XtaskCommand::CertificationReportCheck => run_certification_report_check(),
        XtaskCommand::LeaderReadinessCheck => run_leader_readiness_check(),
        XtaskCommand::SecurityReviewCheck => run_security_review_check(),
        XtaskCommand::CryptoAgilityCheck => run_crypto_agility_check(),
        XtaskCommand::KeyManagementCheck => run_key_management_check(),
        XtaskCommand::SdkSecurityCheck => run_sdk_security_check(),
        XtaskCommand::SdkFuzzCheck => run_sdk_fuzz_check(),
        XtaskCommand::SdkPropertyCheck => run_sdk_property_check(),
        XtaskCommand::SdkModelCheck => run_sdk_model_check(),
        XtaskCommand::SdkRaceCheck => run_sdk_race_check(),
        XtaskCommand::SdkReplayCheck => run_sdk_replay_check(),
        XtaskCommand::SdkMetricsCheck => run_sdk_metrics_check(),
        XtaskCommand::SdkBenchCheck => run_sdk_bench_check(),
        XtaskCommand::SdkPerfBudgetCheck => run_sdk_perf_budget_check(),
        XtaskCommand::PythonImplBenchCompare { profile } => run_python_impl_bench_compare(profile),
        XtaskCommand::PythonImplBenchReport {
            compare_runs,
            resource_runs,
            resource_iterations,
        } => run_python_impl_bench_report(compare_runs, resource_runs, resource_iterations),
        XtaskCommand::PythonImplBenchWorkload { implementation, benchmark, iterations, output } => {
            run_python_impl_bench_workload(implementation, &benchmark, iterations, &output)
        }
        XtaskCommand::SdkMemoryBudgetCheck => run_sdk_memory_budget_check(),
        XtaskCommand::SdkQueuePressureCheck => run_sdk_queue_pressure_check(),
        XtaskCommand::SupplyChainCheck => run_supply_chain_check(),
        XtaskCommand::ReproducibleBuildCheck => run_reproducible_build_check(),
        XtaskCommand::SdkMatrixCheck => run_sdk_matrix_check(),
        XtaskCommand::InterfacesRequired => run_interfaces_required(),
        XtaskCommand::EmbeddedLinkCheck => run_embedded_link_check(),
        XtaskCommand::EmbeddedNativeLockCheck => run_embedded_native_lock_check(),
        XtaskCommand::EmbeddedCoreCheck => run_embedded_core_check(),
        XtaskCommand::EmbeddedFootprintCheck => run_embedded_footprint_check(),
        XtaskCommand::EmbeddedHilCheck => run_embedded_hil_check(),
        XtaskCommand::EmbeddedNodeBuild => run_embedded_node_build(),
        XtaskCommand::EmbeddedNodeContract => run_embedded_node_contract(),
        XtaskCommand::EmbeddedNodeFailureMatrix => run_embedded_node_failure_matrix(),
        XtaskCommand::EmbeddedNodeHil => run_embedded_node_hil(),
    }
}

fn run_ci(stage: Option<CiStage>) -> Result<()> {
    if let Some(stage) = stage {
        return run_ci_stage(stage);
    }

    run_pr_core_ci()
}

fn run_pr_core_ci() -> Result<()> {
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
    run("cargo", &["check", "--workspace", "--all-targets"])?;
    run("cargo", &["nextest", "run", "--workspace", "--lib", "--bins"])?;
    run("cargo", &["test", "--workspace", "--tests"])?;
    run_proto_check()?;
    run_sdk_schema_check()?;
    run("cargo", &["test", "-p", "rns-rpc", "grpc::tests"])?;
    run("cargo", &["check", "-p", "reticulumd", "-p", "rns-tools", "-p", "lxmf-grpc-client"])?;
    run("bash", &["tools/scripts/check-boundaries.sh"])?;
    run_cargo_deny_policy_check()?;
    run_cargo_audit()?;
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
            run_cargo_deny_policy_check()?;
            run_cargo_audit()?;
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
        CiStage::ReferenceIntegrationCheck => run_reference_integration_check(),
        CiStage::DxBootstrapCheck => run_dx_bootstrap_check(),
        CiStage::SdkIncidentRunbookCheck => run_sdk_incident_runbook_check(),
        CiStage::SdkDrillCheck => run_sdk_drill_check(),
        CiStage::SdkSoakCheck => run_sdk_soak_check(),
        CiStage::InteropArtifacts => run_interop_artifacts(false),
        CiStage::InteropMatrixCheck => run_interop_matrix_check(),
        CiStage::InteropCorpusCheck => run_interop_corpus_check(),
        CiStage::InteropDriftCheck => run_interop_drift_check(false),
        CiStage::SchemaClientCheck => run_schema_client_check(),
        CiStage::ProtoCheck => run_proto_check(),
        CiStage::CompatKitCheck => run_compat_kit_check(),
        CiStage::CertificationReportCheck => run_certification_report_check(),
        CiStage::E2eCompatibility => run_e2e_compatibility(),
        CiStage::SdkProfileBuild => run_sdk_profile_build(),
        CiStage::SdkExamplesCheck => run_sdk_examples_check(),
        CiStage::SdkApiBreak => run_sdk_api_break(),
        CiStage::SdkMigrationCheck => run_sdk_migration_check(),
        CiStage::ChangelogMigrationCheck => run_changelog_migration_check(),
        CiStage::GovernanceCheck => run_governance_check(),
        CiStage::ComplianceProfileCheck => run_compliance_profile_check(),
        CiStage::SupportPolicyCheck => run_support_policy_check(),
        CiStage::UnsafeAuditCheck => run_unsafe_audit_check(),
        CiStage::CanaryCriteriaCheck => run_canary_criteria_check(),
        CiStage::ReleaseScorecardCheck => run_release_scorecard_check(),
        CiStage::ExtensionRegistryCheck => run_extension_registry_check(),
        CiStage::PluginNegotiationCheck => run_plugin_negotiation_check(),
        CiStage::LeaderReadinessCheck => run_leader_readiness_check(),
        CiStage::SecurityReviewCheck => run_security_review_check(),
        CiStage::CryptoAgilityCheck => run_crypto_agility_check(),
        CiStage::KeyManagementCheck => run_key_management_check(),
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
        CiStage::InterfacesRequired => run_interfaces_required(),
        CiStage::EmbeddedLinkCheck => run_embedded_link_check(),
        CiStage::EmbeddedNativeLockCheck => run_embedded_native_lock_check(),
        CiStage::EmbeddedCoreCheck => run_embedded_core_check(),
        CiStage::EmbeddedFootprintCheck => run_embedded_footprint_check(),
        CiStage::EmbeddedHilCheck => run_embedded_hil_check(),
        CiStage::EmbeddedNodeBuild => run_embedded_node_build(),
        CiStage::EmbeddedNodeContract => run_embedded_node_contract(),
        CiStage::EmbeddedNodeFailureMatrix => run_embedded_node_failure_matrix(),
        CiStage::EmbeddedNodeHil => run_embedded_node_hil(),
        CiStage::Correctness => run_correctness_check(),
        CiStage::MigrationChecks => run_migration_checks(),
        CiStage::ArchitectureLint => run_architecture_lint_check(),
        CiStage::ArchitectureChecks => run_architecture_checks(),
        CiStage::ForbiddenDeps => run_forbidden_deps(),
    }
}

fn run_cargo_audit() -> Result<()> {
    let mut args: Vec<&str> = Vec::with_capacity(1 + CARGO_AUDIT_IGNORE_ADVISORIES.len() * 2);
    args.push("audit");
    for advisory in CARGO_AUDIT_IGNORE_ADVISORIES {
        args.push("--ignore");
        args.push(advisory);
    }
    run("cargo", &args)
}

fn run_cargo_deny_policy_check() -> Result<()> {
    run("cargo", &["deny", "check", "bans", "licenses", "sources"])
}

fn run_release_check() -> Result<()> {
    run_pr_core_ci()?;
    run_correctness_check()?;
    run("cargo", &["doc", "--workspace", "--no-deps"])?;
    run_sdk_docs_check()?;
    run_sdk_cookbook_check()?;
    run_sdk_ergonomics_check()?;
    run_lxmf_cli_check()?;
    run_reference_integration_check()?;
    run_dx_bootstrap_check()?;
    run_sdk_incident_runbook_check()?;
    run_sdk_drill_check()?;
    run_sdk_soak_check()?;
    run_interop_artifacts(false)?;
    run_interop_matrix_check()?;
    run_interop_corpus_check()?;
    run_interop_drift_check(false)?;
    run_schema_client_check()?;
    run_compat_kit_check()?;
    run_certification_report_check()?;
    run_e2e_compatibility()?;
    run_sdk_conformance()?;
    run_sdk_profile_build()?;
    run_sdk_examples_check()?;
    run_governance_check()?;
    run_interfaces_required()?;
    run_compliance_profile_check()?;
    run_support_policy_check()?;
    run_unsafe_audit_check()?;
    run_release_scorecard_check()?;
    run_canary_criteria_check()?;
    run_extension_registry_check()?;
    run_plugin_negotiation_check()?;
    run_security_review_check()?;
    run_sdk_security_check()?;
    run_sdk_api_break()?;
    run_changelog_migration_check()?;
    run_crypto_agility_check()?;
    run_key_management_check()?;
    run_supply_chain_check()?;
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
    run_embedded_native_lock_check()?;
    run_embedded_core_check()?;
    run_embedded_node_build()?;
    run_embedded_node_contract()?;
    run_embedded_node_failure_matrix()?;
    run_embedded_footprint_check()?;
    run_migration_checks()?;
    run_architecture_checks()?;
    Ok(())
}

fn run_interfaces_required() -> Result<()> {
    run("cargo", &["check", "-p", "reticulumd", "--all-targets"])?;
    run("cargo", &["check", "-p", "rns-rpc", "--all-targets"])?;
    run("cargo", &["check", "-p", "lxmf-sdk", "--all-targets"])?;
    run("cargo", &["check", "-p", "rns-transport", "--all-targets"])?;
    run("cargo", &["test", "-p", "reticulumd", "--test", "config"])?;
    run("cargo", &["test", "-p", "reticulumd", "--bin", "reticulumd"])?;
    run("cargo", &["test", "-p", "rns-transport", "serial::tests"])?;
    run("cargo", &["test", "-p", "reticulumd", "--bin", "reticulumd", "interfaces::ble::"])?;
    run("cargo", &["test", "-p", "reticulumd", "--bin", "reticulumd", "lora_state::tests"])?;
    run(
        "cargo",
        &["test", "-p", "rns-rpc", "set_interfaces_rejects_startup_only_interface_kinds"],
    )?;
    run("cargo", &["test", "-p", "rns-rpc", "reload_config_hot_applies_legacy_tcp_only_diff"])?;
    run(
        "cargo",
        &[
            "test",
            "-p",
            "rns-rpc",
            "reload_config_rejects_mixed_startup_kind_diff_without_partial_apply",
        ],
    )?;
    run("cargo", &["test", "-p", "lxmf-sdk", "--test", "mobile_ble_contract"])?;
    run(
        "cargo",
        &[
            "test",
            "-p",
            "test-support",
            "--test",
            "mobile_ble_android_conformance",
            "--test",
            "mobile_ble_ios_conformance",
        ],
    )?;
    run("bash", &["tools/scripts/check-boundaries.sh"])?;
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

fn run_reference_integration_check() -> Result<()> {
    run("bash", &[REFERENCE_INTEGRATIONS_SMOKE_SCRIPT_PATH])?;

    let runbook = fs::read_to_string(REFERENCE_INTEGRATIONS_RUNBOOK_PATH)
        .with_context(|| format!("missing {REFERENCE_INTEGRATIONS_RUNBOOK_PATH}"))?;
    for marker in [
        "# Reference Integrations",
        "## Service Host Integration (`reticulumd`)",
        "## Desktop App Integration (`lxmf-cli`)",
        "## Gateway Integration (`rns-tools`)",
        "## Reference Integration Smoke Suite",
        "cargo run -p xtask -- reference-integration-check",
        "crates/apps/reticulumd/examples/service-reference.toml",
        "crates/apps/lxmf-cli/examples/desktop-reference.toml",
        "crates/apps/rns-tools/examples/gateway-reference.toml",
    ] {
        if !runbook.contains(marker) {
            bail!(
                "reference integration runbook missing marker '{marker}' in {REFERENCE_INTEGRATIONS_RUNBOOK_PATH}"
            );
        }
    }

    Ok(())
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
            "CYCLES=1 BURST_ROUNDS=2 TIMEOUT_SECS=20 PAUSE_SECS=0 CHAOS_INTERVAL=2 CHAOS_NODES=4 CHAOS_TIMEOUT_SECS=60 MAX_FAILURES=1 REPORT_PATH=target/soak/soak-report.json ./tools/scripts/soak-rnx.sh",
        ],
    )?;
    let report =
        fs::read_to_string(SOAK_REPORT_PATH).with_context(|| format!("read {SOAK_REPORT_PATH}"))?;
    if !report.contains("\"status\": \"pass\"") {
        bail!("soak report indicates non-pass status in {SOAK_REPORT_PATH}");
    }
    if !report.contains("\"max_failures\": 1") {
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

fn run_schema_client_check() -> Result<()> {
    let report = client_codegen::run_schema_client_generate(
        Path::new("."),
        Path::new(SCHEMA_CLIENT_MANIFEST_PATH),
        client_codegen::SchemaClientMode::Check,
    )?;
    let failed = report
        .target_compile_checks
        .iter()
        .filter(|(_, status)| status.starts_with("FAIL:"))
        .collect::<Vec<_>>();
    let status = if report.missing_smoke_count == 0 && failed.is_empty() { "PASS" } else { "FAIL" };
    write_schema_client_check_report(&report, status)?;

    if report.missing_smoke_count > 0 {
        bail!("schema client smoke coverage missing {} method vectors", report.missing_smoke_count);
    }

    if !failed.is_empty() {
        let details = failed
            .into_iter()
            .map(|(language, status)| format!("{language}:{status}"))
            .collect::<Vec<_>>();
        bail!("schema client compile checks failed: {}", details.join(", "));
    }

    Ok(())
}

fn run_schema_client_generate(check: bool) -> Result<client_codegen::SchemaClientReport> {
    let mode = if check {
        client_codegen::SchemaClientMode::Check
    } else {
        client_codegen::SchemaClientMode::Write
    };

    let report = client_codegen::run_schema_client_generate(
        Path::new("."),
        Path::new(SCHEMA_CLIENT_MANIFEST_PATH),
        mode,
    )?;
    let failed = report
        .target_compile_checks
        .iter()
        .filter(|(_, status)| status.starts_with("FAIL:"))
        .collect::<Vec<_>>();
    if !failed.is_empty() {
        let details = failed
            .into_iter()
            .map(|(language, status)| format!("{language}:{status}"))
            .collect::<Vec<_>>();
        bail!("schema client compile checks failed: {}", details.join(", "));
    }

    let status = if report.missing_smoke_count == 0 { "PASS" } else { "PASS_WITH_WARNINGS" };
    write_schema_client_check_report(&report, status)?;
    Ok(report)
}

fn write_schema_client_check_report(
    report: &client_codegen::SchemaClientReport,
    status: &str,
) -> Result<()> {
    let output_parent =
        Path::new(SCHEMA_CLIENT_SMOKE_REPORT_PATH).parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(output_parent)
        .with_context(|| format!("create report directory {}", output_parent.display()))?;

    let mut lines = vec![
        format!("manifest_path={}", report.manifest_path.display()),
        format!("spec_path={}", report.spec_path.display()),
        format!("method_count={}", report.method_count),
        format!("spec_hash={}", report.spec_hash),
        format!("missing_smoke_count={}", report.missing_smoke_count),
        format!("methods={}", report.methods.join(",")),
        format!("status={status}"),
    ];
    for (language, hash) in &report.target_hashes {
        lines.push(format!("target.{language}.hash={hash}"));
    }
    for (language, status) in &report.target_compile_checks {
        lines.push(format!("target.{language}.compile={status}"));
    }

    fs::write(SCHEMA_CLIENT_SMOKE_REPORT_PATH, format!("{}\n", lines.join("\n")))
        .with_context(|| format!("write {SCHEMA_CLIENT_SMOKE_REPORT_PATH}"))?;

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

    run_sdk_api_stability_check(&current_normalized)?;

    Ok(())
}

fn run_sdk_api_stability_check(current_public_api: &str) -> Result<()> {
    let stability_doc = fs::read_to_string(SDK_API_STABILITY_PATH)
        .with_context(|| format!("missing {SDK_API_STABILITY_PATH}"))?;
    for marker in [
        "# SDK API Stability Classes",
        "## Stability Classes",
        "| Class | Match Prefix | Lifecycle Rule |",
        "## Deprecation Workflow",
    ] {
        if !stability_doc.contains(marker) {
            bail!("stability contract missing marker '{marker}' in {SDK_API_STABILITY_PATH}");
        }
    }

    let rows =
        parse_markdown_table_rows(&stability_doc, &["Class", "Match Prefix", "Lifecycle Rule"])?;
    if rows.is_empty() {
        bail!("stability contract must contain at least one classification row");
    }

    let mut rules = Vec::<(String, String)>::new();
    for row in rows {
        if row.len() < 3 {
            continue;
        }
        let class = row[0].trim().trim_matches('`').to_ascii_lowercase();
        let prefix = row[1].trim().trim_matches('`').to_string();
        let lifecycle = row[2].trim().trim_matches('`');
        if class.is_empty() || prefix.is_empty() || lifecycle.is_empty() {
            continue;
        }
        if !matches!(class.as_str(), "stable" | "experimental" | "internal") {
            bail!("invalid stability class '{class}' in {SDK_API_STABILITY_PATH}");
        }
        rules.push((class, prefix));
    }
    if rules.is_empty() {
        bail!("stability contract has no usable classification rules");
    }

    let mut unmatched = Vec::new();
    let mut matched_rule_indexes = BTreeSet::new();

    for line in current_public_api.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("pub ") || !trimmed.contains("lxmf_sdk::") {
            continue;
        }

        let matched = rules.iter().enumerate().find(|(_, (_, prefix))| trimmed.contains(prefix));

        if let Some((idx, _)) = matched {
            matched_rule_indexes.insert(idx);
        } else {
            unmatched.push(trimmed.to_string());
        }
    }

    if !unmatched.is_empty() {
        let first = unmatched[0].clone();
        bail!(
            "unclassified sdk public api entry '{first}' (update {SDK_API_STABILITY_PATH} rules)"
        );
    }

    for (idx, (_, prefix)) in rules.iter().enumerate() {
        if !matched_rule_indexes.contains(&idx) {
            bail!(
                "stability rule prefix '{prefix}' in {SDK_API_STABILITY_PATH} is stale (matches no public API entries)"
            );
        }
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

fn run_changelog_migration_check() -> Result<()> {
    let migration_contract_path = "docs/contracts/sdk-v2-migration.md";
    let migration_contract = fs::read_to_string(migration_contract_path)
        .with_context(|| format!("missing {migration_contract_path}"))?;
    for marker in [
        "## Machine-Checkable Migration Gates",
        "cargo xtask sdk-migration-check",
        "cargo xtask sdk-api-break",
        "cargo xtask changelog-migration-check",
        GENERATED_MIGRATION_NOTES_PATH,
    ] {
        if !migration_contract.contains(marker) {
            bail!(
                "migration contract missing required marker '{marker}' in {migration_contract_path}"
            );
        }
    }

    let sdk_contract_path = "docs/contracts/sdk-v2.md";
    let sdk_contract = fs::read_to_string(sdk_contract_path)
        .with_context(|| format!("missing {sdk_contract_path}"))?;
    let contract_release = extract_backtick_value(&sdk_contract, "Contract release:")
        .with_context(|| format!("unable to parse contract release from {sdk_contract_path}"))?;
    let schema_namespace = extract_backtick_value(&sdk_contract, "Schema namespace:")
        .with_context(|| format!("unable to parse schema namespace from {sdk_contract_path}"))?;

    let output = format!(
        "# Generated Migration Notes\n\n\
         This file is generated by `cargo xtask changelog-migration-check`.\n\n\
         ## Contract Snapshot\n\n\
         - Contract release: `{contract_release}`\n\
         - Schema namespace: `{schema_namespace}`\n\n\
         ## Required Migration Gates\n\n\
         - `cargo xtask sdk-migration-check`\n\
         - `cargo xtask sdk-api-break`\n\
         - `cargo xtask sdk-schema-check`\n\
         - `cargo xtask sdk-conformance`\n\n\
         ## Release Operator Checklist\n\n\
         1. Validate cutover map ownership and replacement classification.\n\
         2. Confirm alias/deprecation timelines in `docs/contracts/sdk-v2-migration.md`.\n\
         3. Attach this generated note artifact to release readiness evidence.\n"
    );

    if let Some(parent) = Path::new(GENERATED_MIGRATION_NOTES_PATH).parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(GENERATED_MIGRATION_NOTES_PATH, output)
        .with_context(|| format!("write {GENERATED_MIGRATION_NOTES_PATH}"))?;

    let generated = fs::read_to_string(GENERATED_MIGRATION_NOTES_PATH)
        .with_context(|| format!("missing {GENERATED_MIGRATION_NOTES_PATH}"))?;
    for marker in [
        "# Generated Migration Notes",
        "## Contract Snapshot",
        "## Required Migration Gates",
        "## Release Operator Checklist",
        "cargo xtask sdk-migration-check",
        "cargo xtask sdk-api-break",
    ] {
        if !generated.contains(marker) {
            bail!(
                "generated migration notes missing marker '{marker}' in {GENERATED_MIGRATION_NOTES_PATH}"
            );
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

    let security_policy = fs::read_to_string(SECURITY_POLICY_DOC_PATH)
        .with_context(|| format!("missing {SECURITY_POLICY_DOC_PATH}"))?;
    for marker in [
        "# Security Policy",
        "## Reporting a Vulnerability",
        "## Coordinated Disclosure Workflow",
        "docs/runbooks/cve-response-workflow.md",
    ] {
        if !security_policy.contains(marker) {
            bail!("security policy missing marker '{marker}' in {SECURITY_POLICY_DOC_PATH}");
        }
    }

    let cve_runbook = fs::read_to_string(CVE_RESPONSE_RUNBOOK_PATH)
        .with_context(|| format!("missing {CVE_RESPONSE_RUNBOOK_PATH}"))?;
    for marker in [
        "# CVE Disclosure and Response Workflow",
        "## Intake and Triage",
        "## Severity Classification",
        "## Patch and Backport Process",
        "## Advisory Publication",
        "## Evidence Checklist",
    ] {
        if !cve_runbook.contains(marker) {
            bail!("cve runbook missing marker '{marker}' in {CVE_RESPONSE_RUNBOOK_PATH}");
        }
    }

    let release_readiness = fs::read_to_string("docs/runbooks/release-readiness.md")
        .context("missing docs/runbooks/release-readiness.md")?;
    if !release_readiness.contains("docs/runbooks/cve-response-workflow.md") {
        bail!("release readiness runbook must reference docs/runbooks/cve-response-workflow.md");
    }

    Ok(())
}

fn run_compliance_profile_check() -> Result<()> {
    let runbook = fs::read_to_string(COMPLIANCE_PROFILES_RUNBOOK_PATH)
        .with_context(|| format!("missing {COMPLIANCE_PROFILES_RUNBOOK_PATH}"))?;
    for marker in [
        "# Compliance Deployment Profiles",
        "## Objectives",
        "## Profile: Regulated Baseline",
        "## Profile: Regulated Strict",
        "## Audit Logging and Evidence",
        "## Release Gate Mapping",
        "## Operational Checklist",
        "regulated-baseline",
        "regulated-strict",
        "key-management-check",
    ] {
        if !runbook.contains(marker) {
            bail!(
                "compliance profiles runbook missing marker '{marker}' in {COMPLIANCE_PROFILES_RUNBOOK_PATH}"
            );
        }
    }

    let matrix = fs::read_to_string(SDK_FEATURE_MATRIX_PATH)
        .with_context(|| format!("missing {SDK_FEATURE_MATRIX_PATH}"))?;
    for marker in [
        "## Compliance Deployment Profiles",
        "docs/runbooks/compliance-profiles.md",
        "regulated-baseline",
        "regulated-strict",
    ] {
        if !matrix.contains(marker) {
            bail!(
                "feature matrix missing compliance marker '{marker}' in {SDK_FEATURE_MATRIX_PATH}"
            );
        }
    }

    let release_readiness = fs::read_to_string("docs/runbooks/release-readiness.md")
        .context("missing docs/runbooks/release-readiness.md")?;
    for marker in [
        "compliance-profile-check",
        "cargo run -p xtask -- compliance-profile-check",
        "docs/runbooks/compliance-profiles.md",
    ] {
        if !release_readiness.contains(marker) {
            bail!("release readiness runbook missing compliance marker '{marker}'");
        }
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

    Ok(())
}

fn run_unsafe_audit_check() -> Result<()> {
    let policy = fs::read_to_string(UNSAFE_POLICY_PATH)
        .with_context(|| format!("missing {UNSAFE_POLICY_PATH}"))?;
    for marker in [
        "# Unsafe Code Policy",
        "## Guardrails",
        "## Inventory Process",
        "## Reviewer Requirements",
        "## CI Gate",
        "tools/scripts/check-unsafe.sh",
        "cargo xtask ci --stage unsafe-audit-check",
    ] {
        if !policy.contains(marker) {
            bail!("unsafe policy missing required marker '{marker}' in {UNSAFE_POLICY_PATH}");
        }
    }

    let inventory = fs::read_to_string(UNSAFE_INVENTORY_PATH)
        .with_context(|| format!("missing {UNSAFE_INVENTORY_PATH}"))?;
    for marker in [
        "# Unsafe Inventory",
        "## Active Unsafe Entries",
        "| Id | File | Line | Safety Invariant | Owner | Last Reviewed |",
    ] {
        if !inventory.contains(marker) {
            bail!("unsafe inventory missing required marker '{marker}' in {UNSAFE_INVENTORY_PATH}");
        }
    }

    let adr = fs::read_to_string(UNSAFE_GOVERNANCE_ADR_PATH)
        .with_context(|| format!("missing {UNSAFE_GOVERNANCE_ADR_PATH}"))?;
    for marker in [
        "# ADR 0006: Unsafe Code Audit Governance",
        "- Status: Accepted",
        "tools/scripts/check-unsafe.sh",
    ] {
        if !adr.contains(marker) {
            bail!("unsafe governance adr missing required marker '{marker}' in {UNSAFE_GOVERNANCE_ADR_PATH}");
        }
    }

    run("bash", &[UNSAFE_AUDIT_SCRIPT_PATH])?;

    let codeowners = fs::read_to_string(CODEOWNERS_PATH)
        .with_context(|| format!("missing {CODEOWNERS_PATH}"))?;
    for entry in [
        "/docs/architecture/unsafe-code-policy.md @FreeTAKTeam",
        "/docs/architecture/unsafe-inventory.md @FreeTAKTeam",
        "/docs/adr/0006-unsafe-code-audit-governance.md @FreeTAKTeam",
        "/tools/scripts/check-unsafe.sh @FreeTAKTeam",
    ] {
        if !codeowners.contains(entry) {
            bail!("CODEOWNERS missing unsafe governance entry '{entry}'");
        }
    }

    Ok(())
}

fn run_release_scorecard_check() -> Result<()> {
    run_sdk_perf_budget_check()?;
    run_sdk_soak_check()?;
    run_supply_chain_check()?;
    run("bash", &["-lc", "SCORECARD_MAX_SOAK_FAILURES=1 tools/scripts/release-scorecard.sh"])?;

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

fn run_canary_criteria_check() -> Result<()> {
    run_release_scorecard_check()?;
    run(
        "bash",
        &[
            "-lc",
            "CANARY_MAX_SOAK_FAILURES=1 CANARY_MAX_MESH_FAILURES=1 tools/scripts/canary-criteria-check.sh",
        ],
    )?;

    let markdown = fs::read_to_string(CANARY_CRITERIA_REPORT_PATH).with_context(|| {
        format!("missing generated canary report markdown at {CANARY_CRITERIA_REPORT_PATH}")
    })?;
    let json = fs::read_to_string(CANARY_CRITERIA_REPORT_JSON_PATH).with_context(|| {
        format!("missing generated canary report json at {CANARY_CRITERIA_REPORT_JSON_PATH}")
    })?;

    for marker in ["# Canary Criteria Report", "## Rollback Triggers"] {
        if !markdown.contains(marker) {
            bail!("generated canary report missing marker '{marker}' in {CANARY_CRITERIA_REPORT_PATH}");
        }
    }
    for marker in ["\"status\"", "\"criteria\"", "\"rollback_triggers\""] {
        if !json.contains(marker) {
            bail!("generated canary report json missing marker '{marker}' in {CANARY_CRITERIA_REPORT_JSON_PATH}");
        }
    }

    let release_readiness = fs::read_to_string("docs/runbooks/release-readiness.md")
        .context("missing docs/runbooks/release-readiness.md")?;
    for marker in ["canary-criteria-check", "Canary Lane and Rollback Criteria"] {
        if !release_readiness.contains(marker) {
            bail!(
                "release readiness runbook missing marker '{marker}' for canary criteria workflow"
            );
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

    Ok(())
}

fn run_plugin_negotiation_check() -> Result<()> {
    run("cargo", &["test", "-p", "lxmf-sdk", "plugin_negotiation", "--", "--nocapture"])?;

    let backends = fs::read_to_string(SDK_BACKENDS_CONTRACT_PATH)
        .with_context(|| format!("missing {SDK_BACKENDS_CONTRACT_PATH}"))?;
    for marker in [
        "## Extension and Plugin Model",
        "PluginDescriptor",
        "PluginState",
        "negotiate_plugins",
        "plugin-negotiation-check",
    ] {
        if !backends.contains(marker) {
            bail!(
                "backend contract missing plugin marker '{marker}' in {SDK_BACKENDS_CONTRACT_PATH}"
            );
        }
    }

    let feature_matrix = fs::read_to_string(SDK_FEATURE_MATRIX_PATH)
        .with_context(|| format!("missing {SDK_FEATURE_MATRIX_PATH}"))?;
    if !feature_matrix.contains("sdk.capability.plugin_host") {
        bail!("feature matrix must include sdk.capability.plugin_host capability row");
    }

    let adr = fs::read_to_string("docs/adr/0008-plugin-extension-model.md")
        .context("missing docs/adr/0008-plugin-extension-model.md")?;
    for marker in [
        "# ADR 0008: Extension and Plugin Contract Model",
        "- Status: Accepted",
        "negotiate_plugins",
    ] {
        if !adr.contains(marker) {
            bail!("plugin extension ADR missing marker '{marker}'");
        }
    }

    Ok(())
}

fn run_certification_report_check() -> Result<()> {
    run(
        "cargo",
        &["test", "-p", "test-support", "sdk_conformance_certification", "--", "--nocapture"],
    )?;
    run("bash", &[CERTIFICATION_REPORT_SCRIPT_PATH])?;

    let matrix = fs::read_to_string("docs/contracts/compatibility-matrix.md")
        .context("missing docs/contracts/compatibility-matrix.md")?;
    for marker in [
        "## Third-Party Conformance Certification",
        "| Bronze |",
        "| Silver |",
        "| Gold |",
        "cargo run -p xtask -- certification-report-check",
    ] {
        if !matrix.contains(marker) {
            bail!("compatibility matrix missing certification marker '{marker}'");
        }
    }

    let report = fs::read_to_string(CERTIFICATION_REPORT_PATH)
        .with_context(|| format!("missing generated report at {CERTIFICATION_REPORT_PATH}"))?;
    if !report.contains("# Certification Report") || !report.contains("status: `PASS`") {
        bail!("certification report missing required markers in {CERTIFICATION_REPORT_PATH}");
    }

    let report_json = fs::read_to_string(CERTIFICATION_REPORT_JSON_PATH)
        .with_context(|| format!("missing generated report at {CERTIFICATION_REPORT_JSON_PATH}"))?;
    for marker in ["\"status\": \"PASS\"", "\"bronze\": \"PASS\"", "\"gold\": \"PASS\""] {
        if !report_json.contains(marker) {
            bail!(
                "certification report json missing marker '{marker}' in {CERTIFICATION_REPORT_JSON_PATH}"
            );
        }
    }

    Ok(())
}

fn run_leader_readiness_check() -> Result<()> {
    run_release_check()?;

    let scorecard_json = fs::read_to_string("target/release-scorecard/release-scorecard.json")
        .context("missing target/release-scorecard/release-scorecard.json after release check")?;
    let scorecard: serde_json::Value =
        serde_json::from_str(&scorecard_json).context("invalid release scorecard json")?;
    let overall_status =
        scorecard.get("overall_status").and_then(|value| value.as_str()).unwrap_or("UNKNOWN");
    if overall_status != "PASS" {
        bail!("leader readiness requires scorecard overall_status=PASS, found '{overall_status}'");
    }

    let soak_json = fs::read_to_string(SOAK_REPORT_PATH)
        .with_context(|| format!("missing {SOAK_REPORT_PATH} after release check"))?;
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
This report certifies that release checks, compatibility checks, and release scorecard\n\
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

fn run_crypto_agility_check() -> Result<()> {
    let rpc_contract = fs::read_to_string(RPC_CONTRACT_PATH)
        .with_context(|| format!("read {RPC_CONTRACT_PATH}"))?;
    for marker in [
        "## Cryptographic Agility Policy",
        "algorithm_set_id",
        "supported_algorithm_sets",
        "selected_algorithm_set",
        "rns-a1",
        "rns-a2",
    ] {
        if !rpc_contract.contains(marker) {
            bail!("rpc contract missing crypto agility marker '{marker}' in {RPC_CONTRACT_PATH}");
        }
    }

    let payload_contract = fs::read_to_string(PAYLOAD_CONTRACT_PATH)
        .with_context(|| format!("read {PAYLOAD_CONTRACT_PATH}"))?;
    for marker in ["## Cryptographic Agility Metadata", "algorithm_set_id", "fail closed", "rns-a1"]
    {
        if !payload_contract.contains(marker) {
            bail!(
                "payload contract missing crypto agility marker '{marker}' in {PAYLOAD_CONTRACT_PATH}"
            );
        }
    }

    let crypto_adr = fs::read_to_string(CRYPTO_AGILITY_ADR_PATH)
        .with_context(|| format!("read {CRYPTO_AGILITY_ADR_PATH}"))?;
    for marker in [
        "# ADR 0007: Cryptographic Agility and Algorithm Negotiation Roadmap",
        "- Status: Accepted",
        "rns-a1",
        "selected_algorithm_set",
    ] {
        if !crypto_adr.contains(marker) {
            bail!("crypto agility adr missing marker '{marker}' in {CRYPTO_AGILITY_ADR_PATH}");
        }
    }

    run(
        "cargo",
        &["test", "-p", "test-support", "sdk_conformance_crypto_agility", "--", "--nocapture"],
    )?;

    Ok(())
}

fn run_key_management_check() -> Result<()> {
    run("cargo", &["test", "-p", "rns-core", "key_manager", "--", "--nocapture"])?;
    run(
        "cargo",
        &["test", "-p", "test-support", "sdk_conformance_key_management", "--", "--nocapture"],
    )?;

    let backends = fs::read_to_string(SDK_BACKENDS_CONTRACT_PATH)
        .with_context(|| format!("missing {SDK_BACKENDS_CONTRACT_PATH}"))?;
    for marker in [
        "## Key Management Backend Contract",
        "sdk.capability.key_management",
        "OsKeyStoreHook",
        "HsmKeyStoreHook",
        "FallbackKeyManager<Primary, Secondary>",
        "cargo run -p xtask -- key-management-check",
    ] {
        if !backends.contains(marker) {
            bail!(
                "backend contract missing key-management marker '{marker}' in {SDK_BACKENDS_CONTRACT_PATH}"
            );
        }
    }

    let matrix = fs::read_to_string(SDK_FEATURE_MATRIX_PATH)
        .with_context(|| format!("missing {SDK_FEATURE_MATRIX_PATH}"))?;
    if !matrix.contains("sdk.capability.key_management") {
        bail!("feature matrix must include sdk.capability.key_management capability row");
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

fn run_correctness_check() -> Result<()> {
    run(
        "cargo",
        &[
            "clippy",
            "-p",
            "lxmf-sdk",
            "-p",
            "rns-rpc",
            "--lib",
            "--all-features",
            "--no-deps",
            "--",
            "-D",
            "clippy::manual_assert",
            "-D",
            "clippy::redundant_clone",
            "-D",
            "clippy::iter_cloned_collect",
        ],
    )?;

    let miri_toolchain =
        std::env::var("SDK_CORRECTNESS_MIRI_TOOLCHAIN").unwrap_or_else(|_| "nightly".to_string());
    let miri_command =
        toolchain_cargo_command(&miri_toolchain, "miri test -p lxmf-core --lib -- --nocapture");
    run("bash", &["-lc", &miri_command])?;

    run(
        "cargo",
        &[
            "test",
            "-p",
            "lxmf-sdk",
            "--test",
            "loom_lifecycle",
            "--features",
            "loom-tests",
            "--",
            "--nocapture",
        ],
    )
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

#[derive(Debug, Deserialize, Serialize, Clone)]
struct PythonBenchReport {
    benchmarks: Vec<PythonBenchmark>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct PythonBenchmark {
    name: String,
    iterations: usize,
    mean_ns: f64,
    p50_ns: f64,
    p95_ns: f64,
    p99_ns: f64,
    throughput_ops_per_sec: f64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct BenchStats {
    iterations: usize,
    sample_count: usize,
    mean_ns: f64,
    p50_ns: f64,
    p95_ns: f64,
    p99_ns: f64,
    throughput_ops_per_sec: f64,
}

#[derive(Debug, Deserialize)]
struct PythonImplBenchConfig {
    profiles: PythonImplBenchProfiles,
    comparisons: Vec<PythonImplComparison>,
}

#[derive(Debug, Deserialize)]
struct PythonImplBenchProfiles {
    fast: PythonImplBenchProfileConfig,
    report: PythonImplBenchProfileConfig,
}

impl PythonImplBenchProfiles {
    fn get(&self, profile: PythonImplBenchProfile) -> &PythonImplBenchProfileConfig {
        match profile {
            PythonImplBenchProfile::Fast => &self.fast,
            PythonImplBenchProfile::Report => &self.report,
        }
    }
}

#[derive(Debug, Deserialize)]
struct PythonImplBenchProfileConfig {
    criterion: PythonImplCriterionConfig,
    python: PythonImplPythonConfig,
    report: PythonImplReportConfig,
}

#[derive(Debug, Deserialize)]
struct PythonImplCriterionConfig {
    sample_size: usize,
    warm_up_time_seconds: f64,
    measurement_time_seconds: f64,
}

#[derive(Debug, Deserialize)]
struct PythonImplPythonConfig {
    iterations: usize,
}

#[derive(Debug, Deserialize)]
struct PythonImplReportConfig {
    compare_runs: usize,
    resource_runs: usize,
    resource_iterations: usize,
    resource_min_duration_seconds: f64,
}

#[derive(Debug, Deserialize, Serialize)]
struct PythonImplComparison {
    label: String,
    rust_benchmark: String,
    python_benchmark: String,
    #[serde(default)]
    workload_class: Option<String>,
    #[serde(default)]
    payload_size_bytes: Option<usize>,
    #[serde(default)]
    batch_size: Option<usize>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct BenchContext {
    workload_class: Option<String>,
    payload_size_bytes: Option<usize>,
    batch_size: Option<usize>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct BenchAdvantage {
    mean_speedup: f64,
    p50_speedup: f64,
    p95_speedup: f64,
    p99_speedup: f64,
    throughput_gain: f64,
    mean_latency_reduction: f64,
    p50_latency_reduction: f64,
    p95_latency_reduction: f64,
    p99_latency_reduction: f64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct PythonImplEnvironment {
    rustc_version: String,
    cargo_version: String,
    python_version: String,
    python_rns_module: String,
    python_lxmf_module: String,
    uname: String,
    git_commit: String,
    benchmark_config_path: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct PythonImplComparisonRow {
    label: String,
    rust_benchmark: String,
    python_benchmark: String,
    context: BenchContext,
    rust: BenchStats,
    python: BenchStats,
    rust_speedup_vs_python: BenchStats,
    rust_advantage_vs_python: BenchAdvantage,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct PythonImplComparisonReport {
    environment: PythonImplEnvironment,
    comparisons: Vec<PythonImplComparisonRow>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ResourceStats {
    runs: usize,
    iterations_per_run: usize,
    mean_peak_rss_bytes: f64,
    median_peak_rss_bytes: u64,
    max_peak_rss_bytes: u64,
    mean_user_cpu_seconds: f64,
    median_user_cpu_seconds: f64,
    mean_sys_cpu_seconds: f64,
    median_sys_cpu_seconds: f64,
    mean_cpu_seconds_per_1k_ops: f64,
    median_cpu_seconds_per_1k_ops: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ResourceAdvantage {
    rss_reduction: f64,
    cpu_time_reduction: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ResourceMeasurement {
    peak_rss_bytes: u64,
    user_cpu_seconds: f64,
    sys_cpu_seconds: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ResourceMeasurementSet {
    iterations_per_run: usize,
    measurements: Vec<ResourceMeasurement>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct PythonImplReportComparison {
    label: String,
    rust_benchmark: String,
    python_benchmark: String,
    context: BenchContext,
    rust: BenchStats,
    python: BenchStats,
    rust_advantage_vs_python: BenchAdvantage,
    rust_resources: ResourceStats,
    python_resources: ResourceStats,
    rust_resource_advantage_vs_python: ResourceAdvantage,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct PythonImplReportSummary {
    profile: String,
    compare_runs: usize,
    resource_runs: usize,
    resource_iterations: usize,
    environment: PythonImplEnvironment,
    comparisons: Vec<PythonImplReportComparison>,
}

struct PythonImplOutputPaths<'a> {
    python_report_path: &'a Path,
    environment_path: &'a Path,
    compare_report_path: &'a Path,
    compare_json_path: &'a Path,
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

fn run_python_impl_bench_compare(profile: PythonImplBenchProfile) -> Result<()> {
    let config = load_python_impl_bench_config()?;
    let profile_config = config.profiles.get(profile);
    let paths = default_python_impl_output_paths();
    run_python_impl_bench_compare_with_paths(&config, profile_config, &paths)
}

fn run_python_impl_bench_report(
    compare_runs_override: Option<usize>,
    resource_runs_override: Option<usize>,
    resource_iterations_override: Option<usize>,
) -> Result<()> {
    let config = load_python_impl_bench_config()?;
    let profile = PythonImplBenchProfile::Report;
    let profile_config = config.profiles.get(profile);
    let compare_runs = compare_runs_override.unwrap_or(profile_config.report.compare_runs);
    let resource_runs = resource_runs_override.unwrap_or(profile_config.report.resource_runs);
    let resource_iterations =
        resource_iterations_override.unwrap_or(profile_config.report.resource_iterations);
    if compare_runs == 0 {
        bail!("python-impl-bench-report requires compare_runs > 0");
    }
    if resource_runs == 0 {
        bail!("python-impl-bench-report requires resource_runs > 0");
    }
    if resource_iterations == 0 {
        bail!("python-impl-bench-report requires resource_iterations > 0");
    }
    let report_root = Path::new(PYTHON_IMPL_REPORT_DIR);
    if report_root.exists() {
        fs::remove_dir_all(report_root)
            .with_context(|| format!("remove {}", report_root.display()))?;
    }
    fs::create_dir_all(report_root).with_context(|| format!("create {}", report_root.display()))?;

    let runs_root = report_root.join("runs");
    fs::create_dir_all(&runs_root).with_context(|| format!("create {}", runs_root.display()))?;
    let mut per_run_reports = Vec::new();

    for run_index in 0..compare_runs {
        let run_dir = runs_root.join(format!("run-{run_index:02}"));
        fs::create_dir_all(&run_dir).with_context(|| format!("create {}", run_dir.display()))?;
        let python_report_path = run_dir.join("python-impl-benchmarks.json");
        let environment_path = run_dir.join("python-impl-environment.json");
        let compare_report_path = run_dir.join("python-impl-compare.txt");
        let compare_json_path = run_dir.join("python-impl-compare.json");
        let paths = PythonImplOutputPaths {
            python_report_path: &python_report_path,
            environment_path: &environment_path,
            compare_report_path: &compare_report_path,
            compare_json_path: &compare_json_path,
        };
        run_python_impl_bench_compare_with_paths(&config, profile_config, &paths)
            .with_context(|| format!("benchmark report run {}", run_index + 1))?;
        per_run_reports.push(load_python_impl_compare_report(paths.compare_json_path)?);
    }

    let resource_measurements = collect_python_impl_resource_measurements(
        &config,
        &per_run_reports,
        resource_runs,
        resource_iterations,
        profile_config.report.resource_min_duration_seconds,
        report_root,
    )?;

    let summary = aggregate_python_impl_report(
        &per_run_reports,
        &config.comparisons,
        &resource_measurements,
        profile,
        compare_runs,
        resource_runs,
        resource_iterations,
    )?;
    write_python_impl_report_summary(&summary)?;
    println!("python implementation benchmark report written to {}", PYTHON_IMPL_REPORT_TEXT_PATH);
    Ok(())
}

fn run_python_impl_bench_workload(
    implementation: PythonImplImplementation,
    benchmark: &str,
    iterations: usize,
    output: &Path,
) -> Result<()> {
    let benchmark = match implementation {
        PythonImplImplementation::Rust => run_rust_python_impl_benchmark(benchmark, iterations)?,
        PythonImplImplementation::Python => {
            bail!("python workloads must be run via tools/scripts/python_impl_benchmarks.py")
        }
    };
    write_python_benchmark_report(output, &[benchmark])
}

fn default_python_impl_output_paths() -> PythonImplOutputPaths<'static> {
    PythonImplOutputPaths {
        python_report_path: Path::new(PYTHON_IMPL_BENCH_REPORT_PATH),
        environment_path: Path::new(PYTHON_IMPL_ENVIRONMENT_PATH),
        compare_report_path: Path::new(PYTHON_IMPL_COMPARE_REPORT_PATH),
        compare_json_path: Path::new(PYTHON_IMPL_COMPARE_JSON_PATH),
    }
}

fn run_python_impl_bench_compare_with_paths(
    config: &PythonImplBenchConfig,
    profile_config: &PythonImplBenchProfileConfig,
    paths: &PythonImplOutputPaths<'_>,
) -> Result<()> {
    let sample_size = profile_config.criterion.sample_size.to_string();
    let warm_up_time = profile_config.criterion.warm_up_time_seconds.to_string();
    let measurement_time = profile_config.criterion.measurement_time_seconds.to_string();
    let python_iterations = profile_config.python.iterations.to_string();

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
            &sample_size,
            "--warm-up-time",
            &warm_up_time,
            "--measurement-time",
            &measurement_time,
        ],
    )?;
    run(
        "cargo",
        &[
            "bench",
            "-p",
            "rns-core",
            "--bench",
            "parity_hotpaths",
            "--",
            "--sample-size",
            &sample_size,
            "--warm-up-time",
            &warm_up_time,
            "--measurement-time",
            &measurement_time,
        ],
    )?;
    run(
        "cargo",
        &[
            "bench",
            "-p",
            "rns-transport",
            "--bench",
            "link_hotpaths",
            "--",
            "--sample-size",
            &sample_size,
            "--warm-up-time",
            &warm_up_time,
            "--measurement-time",
            &measurement_time,
        ],
    )?;
    run(
        "python3",
        &[
            "tools/scripts/python_impl_benchmarks.py",
            "--iterations",
            &python_iterations,
            "--output",
            paths
                .python_report_path
                .to_str()
                .context("python benchmark output path must be utf-8")?,
        ],
    )?;
    write_python_impl_compare_report(config, paths)
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

fn write_python_impl_compare_report(
    config: &PythonImplBenchConfig,
    paths: &PythonImplOutputPaths<'_>,
) -> Result<()> {
    let python_raw = fs::read_to_string(paths.python_report_path)
        .with_context(|| format!("read {}", paths.python_report_path.display()))?;
    let python_report: PythonBenchReport = serde_json::from_str(&python_raw)
        .with_context(|| format!("parse {}", paths.python_report_path.display()))?;
    let environment = capture_python_impl_environment()?;
    fs::write(
        paths.environment_path,
        serde_json::to_string_pretty(&environment)
            .context("serialize python benchmark environment")?,
    )
    .with_context(|| format!("write {}", paths.environment_path.display()))?;

    let python_stats = python_report
        .benchmarks
        .into_iter()
        .map(|entry| {
            (
                entry.name,
                BenchStats {
                    iterations: entry.iterations,
                    sample_count: entry.iterations,
                    mean_ns: entry.mean_ns,
                    p50_ns: entry.p50_ns,
                    p95_ns: entry.p95_ns,
                    p99_ns: entry.p99_ns,
                    throughput_ops_per_sec: entry.throughput_ops_per_sec,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();

    let mut comparisons = Vec::new();
    let mut lines = Vec::new();
    lines.push("# Python Implementation Benchmark Comparison".to_string());
    lines.push(String::new());
    lines.push(
        "Workloads compare Rust core paths against canonical Python `RNS` and `LXMF` implementations."
            .to_string(),
    );
    lines.push(String::new());
    lines.push(format!("- Config: `{}`", PYTHON_IMPL_BENCH_CONFIG_PATH));
    lines.push(format!("- Environment: `{}`", paths.environment_path.display()));
    lines.push(String::new());

    for comparison in &config.comparisons {
        let rust = load_criterion_stats(&comparison.rust_benchmark)?;
        let python = python_stats.get(&comparison.python_benchmark).with_context(|| {
            format!(
                "missing python benchmark `{}` in {}",
                comparison.python_benchmark, PYTHON_IMPL_BENCH_REPORT_PATH
            )
        })?;
        let speedup = BenchStats {
            iterations: python.iterations.min(rust.iterations),
            sample_count: python.sample_count.min(rust.sample_count),
            mean_ns: ratio(python.mean_ns, rust.mean_ns),
            p50_ns: ratio(python.p50_ns, rust.p50_ns),
            p95_ns: ratio(python.p95_ns, rust.p95_ns),
            p99_ns: ratio(python.p99_ns, rust.p99_ns),
            throughput_ops_per_sec: ratio(
                rust.throughput_ops_per_sec,
                python.throughput_ops_per_sec,
            ),
        };
        comparisons.push(PythonImplComparisonRow {
            label: comparison.label.clone(),
            rust_benchmark: comparison.rust_benchmark.clone(),
            python_benchmark: comparison.python_benchmark.clone(),
            context: BenchContext {
                workload_class: comparison.workload_class.clone(),
                payload_size_bytes: comparison.payload_size_bytes,
                batch_size: comparison.batch_size,
            },
            rust: BenchStats {
                iterations: rust.iterations,
                sample_count: rust.sample_count,
                mean_ns: rust.mean_ns,
                p50_ns: rust.p50_ns,
                p95_ns: rust.p95_ns,
                p99_ns: rust.p99_ns,
                throughput_ops_per_sec: rust.throughput_ops_per_sec,
            },
            python: BenchStats {
                iterations: python.iterations,
                sample_count: python.sample_count,
                mean_ns: python.mean_ns,
                p50_ns: python.p50_ns,
                p95_ns: python.p95_ns,
                p99_ns: python.p99_ns,
                throughput_ops_per_sec: python.throughput_ops_per_sec,
            },
            rust_speedup_vs_python: BenchStats {
                iterations: speedup.iterations,
                sample_count: speedup.sample_count,
                mean_ns: speedup.mean_ns,
                p50_ns: speedup.p50_ns,
                p95_ns: speedup.p95_ns,
                p99_ns: speedup.p99_ns,
                throughput_ops_per_sec: speedup.throughput_ops_per_sec,
            },
            rust_advantage_vs_python: BenchAdvantage {
                mean_speedup: speedup.mean_ns,
                p50_speedup: speedup.p50_ns,
                p95_speedup: speedup.p95_ns,
                p99_speedup: speedup.p99_ns,
                throughput_gain: speedup.throughput_ops_per_sec,
                mean_latency_reduction: reduction(python.mean_ns, rust.mean_ns),
                p50_latency_reduction: reduction(python.p50_ns, rust.p50_ns),
                p95_latency_reduction: reduction(python.p95_ns, rust.p95_ns),
                p99_latency_reduction: reduction(python.p99_ns, rust.p99_ns),
            },
        });
        lines.push(format!("## {}", comparison.label));
        let mut context_parts = Vec::new();
        if let Some(workload_class) = &comparison.workload_class {
            context_parts.push(format!("workload_class={workload_class}"));
        }
        if let Some(payload_size_bytes) = comparison.payload_size_bytes {
            context_parts.push(format!("payload_size_bytes={payload_size_bytes}"));
        }
        if let Some(batch_size) = comparison.batch_size {
            context_parts.push(format!("batch_size={batch_size}"));
        }
        if !context_parts.is_empty() {
            lines.push(format!("- Context: {}", context_parts.join(" ")));
        }
        lines.push(format!(
            "- Rust `{}`: iterations={} samples={} mean_ns={:.2} p50_ns={:.2} p95_ns={:.2} p99_ns={:.2} throughput_ops_per_sec={:.2}",
            comparison.rust_benchmark,
            rust.iterations,
            rust.sample_count,
            rust.mean_ns,
            rust.p50_ns,
            rust.p95_ns,
            rust.p99_ns,
            rust.throughput_ops_per_sec
        ));
        lines.push(format!(
            "- Python `{}`: iterations={} samples={} mean_ns={:.2} p50_ns={:.2} p95_ns={:.2} p99_ns={:.2} throughput_ops_per_sec={:.2}",
            comparison.python_benchmark,
            python.iterations,
            python.sample_count,
            python.mean_ns,
            python.p50_ns,
            python.p95_ns,
            python.p99_ns,
            python.throughput_ops_per_sec
        ));
        lines.push(format!(
            "- Rust advantage vs Python: mean={:.2}x p50={:.2}x p95={:.2}x p99={:.2}x throughput={:.2}x mean_latency_reduction={:.2}% p50_latency_reduction={:.2}% p95_latency_reduction={:.2}% p99_latency_reduction={:.2}%",
            speedup.mean_ns,
            speedup.p50_ns,
            speedup.p95_ns,
            speedup.p99_ns,
            speedup.throughput_ops_per_sec,
            reduction(python.mean_ns, rust.mean_ns) * 100.0,
            reduction(python.p50_ns, rust.p50_ns) * 100.0,
            reduction(python.p95_ns, rust.p95_ns) * 100.0,
            reduction(python.p99_ns, rust.p99_ns) * 100.0,
        ));
        lines.push(String::new());
    }

    lines.push(format!(
        "Generated by `cargo run -p xtask -- python-impl-bench-compare`; raw python data lives at `{}`.",
        paths.python_report_path.display()
    ));

    fs::write(paths.compare_report_path, lines.join("\n"))
        .with_context(|| format!("write {}", paths.compare_report_path.display()))?;
    fs::write(
        paths.compare_json_path,
        serde_json::to_string_pretty(&PythonImplComparisonReport { environment, comparisons })
            .context("serialize python implementation comparison report")?,
    )
    .with_context(|| format!("write {}", paths.compare_json_path.display()))?;
    println!("python implementation comparison written to {}", paths.compare_report_path.display());
    Ok(())
}

fn load_python_impl_compare_report(path: &Path) -> Result<PythonImplComparisonReport> {
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))
}

fn load_criterion_stats(benchmark: &str) -> Result<BenchStats> {
    let sample_path = Path::new("target/criterion").join(benchmark).join("new").join("sample.json");
    let raw = fs::read_to_string(&sample_path)
        .with_context(|| format!("read sample data {}", sample_path.display()))?;
    let sample: CriterionSample =
        serde_json::from_str(&raw).with_context(|| format!("parse {}", sample_path.display()))?;
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
    let mean_ns = latency_ns.iter().sum::<f64>() / latency_ns.len() as f64;
    let p50_ns = percentile(&latency_ns, 0.50);
    let p95_ns = percentile(&tail_latencies, 0.95);
    let p99_ns = percentile(&tail_latencies, 0.99);
    let throughput_ops_per_sec = 1_000_000_000.0 / p50_ns.max(1.0);

    Ok(BenchStats {
        iterations: sample.iters.iter().map(|iters| *iters as usize).sum(),
        sample_count: latency_ns.len(),
        mean_ns,
        p50_ns,
        p95_ns,
        p99_ns,
        throughput_ops_per_sec,
    })
}

fn ratio(lhs: f64, rhs: f64) -> f64 {
    lhs / rhs.max(1.0)
}

fn reduction(baseline: f64, improved: f64) -> f64 {
    if baseline <= 0.0 {
        return 0.0;
    }
    (1.0 - (improved / baseline)).clamp(-1.0, 1.0)
}

fn write_python_benchmark_report(output: &Path, benchmarks: &[PythonBenchmark]) -> Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let payload = PythonBenchReport { benchmarks: benchmarks.to_vec() };
    fs::write(
        output,
        serde_json::to_string_pretty(&payload).context("serialize benchmark payload")? + "\n",
    )
    .with_context(|| format!("write {}", output.display()))
}

fn run_rust_python_impl_benchmark(name: &str, iterations: usize) -> Result<PythonBenchmark> {
    let mut samples = Vec::with_capacity(iterations);
    match name {
        "lxmf_core_message_from_wire" => {
            let (wire, _) = rust_sample_wire_payload();
            for _ in 0..iterations {
                let started = Instant::now();
                let decoded =
                    Message::from_wire(black_box(&wire)).context("decode should succeed")?;
                black_box(decoded);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "lxmf_core_message_to_wire" => {
            for _ in 0..iterations {
                let started = Instant::now();
                let mut message = Message::new();
                message.destination_hash = Some([0x44; 16]);
                message.source_hash = Some([0x55; 16]);
                message.signature = Some([0x66; 64]);
                message.timestamp = Some(1_770_000_001.0);
                message.set_title_from_string("wire-title");
                message.set_content_from_string("wire-content");
                let wire = message.to_wire(None).context("encode should succeed")?;
                black_box(wire);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "lxmf_core_large_message_from_wire" => {
            let (wire, _) = rust_sample_large_wire_payload();
            for _ in 0..iterations {
                let started = Instant::now();
                let decoded =
                    Message::from_wire(black_box(&wire)).context("decode should succeed")?;
                black_box(decoded);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "lxmf_core_large_message_to_wire" => {
            let content = "x".repeat(2048);
            for _ in 0..iterations {
                let started = Instant::now();
                let mut message = Message::new();
                message.destination_hash = Some([0xa4; 16]);
                message.source_hash = Some([0xb5; 16]);
                message.signature = Some([0xc6; 64]);
                message.timestamp = Some(1_770_000_101.0);
                message.set_title_from_string("wire-large-title");
                message.set_content_from_string(black_box(&content));
                let wire = message.to_wire(None).context("encode should succeed")?;
                black_box(wire);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "rns_core_announce_create" => {
            let mut destination = rust_sample_destination();
            for _ in 0..iterations {
                let started = Instant::now();
                let packet = destination
                    .announce(OsRng, black_box(Some(b"rust-announce-app-data".as_slice())))
                    .map_err(|err| anyhow!("announce should succeed: {err:?}"))?;
                black_box(packet);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "rns_core_announce_validate" => {
            let mut destination = rust_sample_destination();
            let packet = destination
                .announce(OsRng, Some(b"rust-announce-app-data".as_slice()))
                .map_err(|err| anyhow!("announce should succeed: {err:?}"))?;
            for _ in 0..iterations {
                let started = Instant::now();
                let info = DestinationAnnounce::validate(black_box(&packet))
                    .map_err(|err| anyhow!("announce validation should succeed: {err:?}"))?;
                black_box(info);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "rns_core_announce_validate_batch_64" => {
            let packets = rust_announce_batch_packets()?;
            let mut signed_data = [0u8; rns_core::packet::PACKET_MDU];
            for _ in 0..iterations {
                let started = Instant::now();
                let mut validated = 0usize;
                for packet in &packets {
                    let info = DestinationAnnounce::validate_with_buffer(
                        black_box(packet),
                        black_box(&mut signed_data),
                    )
                    .map_err(|err| anyhow!("announce validation should succeed: {err:?}"))?;
                    validated += info.app_data.len();
                }
                black_box(validated);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "rns_core_identity_sign" => {
            let identity = PrivateIdentity::new_from_rand(OsRng);
            let message = vec![0x5a; 2048];
            for _ in 0..iterations {
                let started = Instant::now();
                let signature = lxmf_sign(black_box(&identity), black_box(&message));
                black_box(signature);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "rns_core_identity_verify" => {
            let identity = PrivateIdentity::new_from_rand(OsRng);
            let public_identity = *identity.as_identity();
            let message = vec![0x5a; 2048];
            let signature = lxmf_sign(&identity, &message);
            for _ in 0..iterations {
                let started = Instant::now();
                let valid = lxmf_verify(
                    black_box(&public_identity),
                    black_box(&message),
                    black_box(&signature),
                );
                black_box(valid);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "rns_core_identity_encrypt" => {
            let recipient = PrivateIdentity::new_from_rand(OsRng);
            let public_identity = *recipient.as_identity();
            let plaintext = vec![0x42; 2048];
            let salt = public_identity.address_hash.as_slice().to_vec();
            let mut out = vec![0u8; 32 + plaintext.len() + 128];
            for _ in 0..iterations {
                let started = Instant::now();
                let ciphertext = encrypt_for_public_key_into(
                    black_box(&public_identity.public_key),
                    black_box(salt.as_slice()),
                    black_box(&plaintext),
                    black_box(out.as_mut_slice()),
                    OsRng,
                )
                .map_err(|err| anyhow!("encryption should succeed: {err:?}"))?;
                black_box(ciphertext);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "rns_core_identity_decrypt" => {
            let recipient = PrivateIdentity::new_from_rand(OsRng);
            let public_identity = *recipient.as_identity();
            let plaintext = vec![0x42; 2048];
            let salt = public_identity.address_hash.as_slice().to_vec();
            let ciphertext = encrypt_for_public_key(
                &public_identity.public_key,
                salt.as_slice(),
                &plaintext,
                OsRng,
            )
            .map_err(|err| anyhow!("encryption should succeed: {err:?}"))?;
            let mut out = vec![0u8; ciphertext.len()];
            for _ in 0..iterations {
                let started = Instant::now();
                let decrypted = decrypt_with_identity_into(
                    black_box(&recipient),
                    black_box(salt.as_slice()),
                    black_box(&ciphertext),
                    black_box(out.as_mut_slice()),
                )
                .map_err(|err| anyhow!("decryption should succeed: {err:?}"))?;
                black_box(decrypted);
                samples.push(started.elapsed().as_nanos() as f64);
            }
        }
        "rns_transport_resource_manager_request_window_reuse" => {
            let (mut sender_link, mut manager, plain_request) =
                rust_resource_manager_request_fixture()?;
            let mut responses = Vec::new();
            for _ in 0..iterations {
                let started = Instant::now();
                manager.handle_packet_into(
                    black_box(&plain_request),
                    black_box(&mut sender_link),
                    black_box(&mut responses),
                );
                black_box(responses.len());
                samples.push(started.elapsed().as_nanos() as f64);
                responses.clear();
            }
        }
        _ => bail!("unsupported rust benchmark workload `{name}`"),
    }

    Ok(python_benchmark_from_samples(name.to_string(), iterations, samples))
}

fn python_benchmark_from_samples(
    name: String,
    iterations: usize,
    mut samples: Vec<f64>,
) -> PythonBenchmark {
    samples.sort_by(f64::total_cmp);
    let tail_samples = trimmed_tail_sample(&samples);
    let mean_ns = samples.iter().sum::<f64>() / samples.len() as f64;
    let p50_ns = percentile(&samples, 0.50);
    let p95_ns = percentile(&tail_samples, 0.95);
    let p99_ns = percentile(&tail_samples, 0.99);
    let throughput_ops_per_sec = 1_000_000_000.0 / p50_ns.max(1.0);
    PythonBenchmark { name, iterations, mean_ns, p50_ns, p95_ns, p99_ns, throughput_ops_per_sec }
}

fn rust_sample_wire_payload() -> (Vec<u8>, [u8; 16]) {
    let mut message = Message::new();
    let destination = [0x11; 16];
    let source = [0x22; 16];
    message.destination_hash = Some(destination);
    message.source_hash = Some(source);
    message.signature = Some([0x33; 64]);
    message.timestamp = Some(1_770_000_000.0);
    message.set_title_from_string("bench-title");
    message.set_content_from_string("bench-content-payload");
    let wire = message.to_wire(None).expect("sample message must encode");
    (wire, destination)
}

fn rust_sample_large_wire_payload() -> (Vec<u8>, [u8; 16]) {
    let mut message = Message::new();
    let destination = [0x77; 16];
    let source = [0x88; 16];
    message.destination_hash = Some(destination);
    message.source_hash = Some(source);
    message.signature = Some([0x99; 64]);
    message.timestamp = Some(1_770_000_100.0);
    message.set_title_from_string("bench-large-title");
    message.set_content_from_string(&"x".repeat(2048));
    let wire = message.to_wire(None).expect("large sample message must encode");
    (wire, destination)
}

fn rust_sample_destination() -> SingleInputDestination {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    SingleInputDestination::new(
        identity,
        DestinationName::new("example_utilities", "announcesample.fruits"),
    )
}

fn rust_announce_batch_packets() -> Result<Vec<rns_core::Packet>> {
    const ANNOUNCE_BATCH_SIZE: usize = 64;
    let mut packets = Vec::with_capacity(ANNOUNCE_BATCH_SIZE);
    for index in 0..ANNOUNCE_BATCH_SIZE {
        let mut destination = rust_sample_destination();
        let app_data = format!("rust-announce-app-data-{index}");
        let packet = destination
            .announce(OsRng, Some(app_data.as_bytes()))
            .map_err(|err| anyhow!("announce should succeed: {err:?}"))?;
        packets.push(packet);
    }
    Ok(packets)
}

fn rust_active_link_pair() -> Result<(Link, Link, Vec<u8>)> {
    let sender = PrivateIdentity::new_from_rand(OsRng);
    let receiver = PrivateIdentity::new_from_rand(OsRng);

    let _sender = to_transport_private_identity(&sender);
    let receiver = to_transport_private_identity(&receiver);

    let destination = DestinationDesc {
        identity: *receiver.as_identity(),
        address_hash: *receiver.address_hash(),
        name: TransportDestinationName::new("lxmf", "delivery"),
    };

    let (tx, _) = tokio::sync::broadcast::channel(16);
    let mut outbound = Link::new(destination, tx.clone());
    let request = outbound.request();

    let mut inbound =
        Link::new_from_request(&request, receiver.sign_key().clone(), destination, tx)
            .map_err(|err| anyhow!("input link: {err:?}"))?;
    let proof = inbound.prove();
    let proof_iface = AddressHash::new_from_rand(OsRng);
    if !matches!(outbound.handle_packet(&proof, proof_iface), LinkHandleResult::Activated) {
        bail!("link activation did not succeed");
    }

    let payload = vec![0x2a; 128];
    Ok((outbound, inbound, payload))
}

fn rust_decrypt_resource_packet(link: &Link, packet: &Packet) -> Result<Packet> {
    let mut plain_packet = *packet;
    let mut buffer = PacketDataBuffer::new();
    let plain_len = {
        let plaintext = link
            .decrypt(packet.data.as_slice(), buffer.accuire_buf_max())
            .map_err(|err| anyhow!("decrypt should succeed: {err:?}"))?;
        plaintext.len()
    };
    buffer.resize(plain_len);
    plain_packet.data = buffer;
    Ok(plain_packet)
}

fn rust_resource_manager_request_fixture() -> Result<(Link, ResourceManager, Packet)> {
    let (sender_link, mut receiver_link, _) = rust_active_link_pair()?;
    let mut sender_manager = ResourceManager::new();
    let mut receiver_manager = ResourceManager::new();
    let resource_data = vec![0x5a; PACKET_MDU * 6];

    let (_, advertisement_packet) = sender_manager
        .start_send(&sender_link, resource_data, None)
        .map_err(|err| anyhow!("resource send should succeed: {err:?}"))?;
    let plain_advertisement = rust_decrypt_resource_packet(&receiver_link, &advertisement_packet)?;

    let mut responses = Vec::new();
    receiver_manager.handle_packet_into(&plain_advertisement, &mut receiver_link, &mut responses);
    let request_packet = responses.pop().context("resource request packet")?;
    let plain_request = rust_decrypt_resource_packet(&sender_link, &request_packet)?;

    Ok((sender_link, sender_manager, plain_request))
}

fn collect_python_impl_resource_measurements(
    config: &PythonImplBenchConfig,
    per_run_reports: &[PythonImplComparisonReport],
    runs: usize,
    baseline_iterations: usize,
    min_duration_seconds: f64,
    report_root: &Path,
) -> Result<BTreeMap<String, ResourceMeasurementSet>> {
    let release_xtask = ensure_release_xtask_binary()?;
    let resources_root = report_root.join("resources");
    fs::create_dir_all(&resources_root)
        .with_context(|| format!("create {}", resources_root.display()))?;
    let time_command = detect_time_command()?;
    let mut measurements = BTreeMap::new();
    let median_rows = aggregate_report_rows_by_label(per_run_reports)?;

    for comparison in &config.comparisons {
        let rust_key = format!("rust:{}", comparison.rust_benchmark);
        let python_key = format!("python:{}", comparison.python_benchmark);
        let median_row = median_rows
            .get(&comparison.label)
            .with_context(|| format!("missing median row for `{}`", comparison.label))?;
        let rust_iterations = resource_iterations_for_duration(
            baseline_iterations,
            median_row.rust.p50_ns,
            min_duration_seconds,
        );
        let python_iterations = resource_iterations_for_duration(
            baseline_iterations,
            median_row.python.p50_ns,
            min_duration_seconds,
        );
        let rust_entries = collect_resource_measurements_for_workload(
            &time_command,
            &release_xtask,
            PythonImplImplementation::Rust,
            &comparison.rust_benchmark,
            runs,
            rust_iterations,
            &resources_root,
        )?;
        measurements.insert(
            rust_key,
            ResourceMeasurementSet {
                iterations_per_run: rust_iterations,
                measurements: rust_entries,
            },
        );

        let python_entries = collect_resource_measurements_for_workload(
            &time_command,
            &release_xtask,
            PythonImplImplementation::Python,
            &comparison.python_benchmark,
            runs,
            python_iterations,
            &resources_root,
        )?;
        measurements.insert(
            python_key,
            ResourceMeasurementSet {
                iterations_per_run: python_iterations,
                measurements: python_entries,
            },
        );
    }

    Ok(measurements)
}

#[derive(Copy, Clone)]
enum TimeCommandFlavor {
    Bsd,
    Gnu,
}

struct TimeCommand {
    program: &'static str,
    flavor: TimeCommandFlavor,
}

fn detect_time_command() -> Result<TimeCommand> {
    let program = "/usr/bin/time";
    if Command::new(program)
        .args(["-l", "true"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .is_some()
    {
        return Ok(TimeCommand { program, flavor: TimeCommandFlavor::Bsd });
    }
    if Command::new(program)
        .args(["-v", "true"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .is_some()
    {
        return Ok(TimeCommand { program, flavor: TimeCommandFlavor::Gnu });
    }
    bail!("unable to find a supported `/usr/bin/time` implementation")
}

fn ensure_release_xtask_binary() -> Result<PathBuf> {
    run("cargo", &["build", "-p", "xtask", "--release"])?;
    let path = Path::new("target").join("release").join(executable_name("xtask"));
    if !path.exists() {
        bail!("expected release xtask binary at {}", path.display());
    }
    Ok(path)
}

fn executable_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

fn collect_resource_measurements_for_workload(
    time_command: &TimeCommand,
    current_exe: &Path,
    implementation: PythonImplImplementation,
    benchmark: &str,
    runs: usize,
    iterations: usize,
    resources_root: &Path,
) -> Result<Vec<ResourceMeasurement>> {
    let mut measurements = Vec::with_capacity(runs);
    for run_index in 0..runs {
        let impl_name = match implementation {
            PythonImplImplementation::Rust => "rust",
            PythonImplImplementation::Python => "python",
        };
        let safe_name = benchmark.replace('/', "_");
        let output_path =
            resources_root.join(format!("{impl_name}-{safe_name}-run-{run_index:02}.json"));
        let (program, args) = match implementation {
            PythonImplImplementation::Rust => (
                current_exe.to_string_lossy().to_string(),
                vec![
                    "python-impl-bench-workload".to_string(),
                    "--implementation".to_string(),
                    "rust".to_string(),
                    "--benchmark".to_string(),
                    benchmark.to_string(),
                    "--iterations".to_string(),
                    iterations.to_string(),
                    "--output".to_string(),
                    output_path.to_string_lossy().to_string(),
                ],
            ),
            PythonImplImplementation::Python => (
                "python3".to_string(),
                vec![
                    "tools/scripts/python_impl_benchmarks.py".to_string(),
                    "--iterations".to_string(),
                    iterations.to_string(),
                    "--benchmark".to_string(),
                    benchmark.to_string(),
                    "--output".to_string(),
                    output_path.to_string_lossy().to_string(),
                ],
            ),
        };
        let measurement = run_timed_command(time_command, &program, &args)
            .with_context(|| format!("measure resources for `{benchmark}` ({impl_name})"))?;
        measurements.push(measurement);
    }
    Ok(measurements)
}

fn run_timed_command(
    time_command: &TimeCommand,
    program: &str,
    args: &[String],
) -> Result<ResourceMeasurement> {
    let mut command = Command::new(time_command.program);
    match time_command.flavor {
        TimeCommandFlavor::Bsd => {
            command.arg("-l");
        }
        TimeCommandFlavor::Gnu => {
            command.arg("-v");
        }
    }
    let output = command
        .arg(program)
        .args(args)
        .output()
        .with_context(|| format!("spawn timed command `{program}`"))?;
    if !output.status.success() {
        bail!("timed command `{program}` failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    parse_time_output(time_command.flavor, &String::from_utf8_lossy(&output.stderr))
}

fn parse_time_output(flavor: TimeCommandFlavor, stderr: &str) -> Result<ResourceMeasurement> {
    match flavor {
        TimeCommandFlavor::Bsd => parse_bsd_time_output(stderr),
        TimeCommandFlavor::Gnu => parse_gnu_time_output(stderr),
    }
}

fn parse_bsd_time_output(stderr: &str) -> Result<ResourceMeasurement> {
    let mut user_cpu_seconds = None;
    let mut sys_cpu_seconds = None;
    let mut peak_rss_bytes = None;
    for line in stderr.lines() {
        let trimmed = line.trim();
        if trimmed.contains(" real ") && trimmed.contains(" user ") && trimmed.contains(" sys") {
            let parts = trimmed.split_whitespace().collect::<Vec<_>>();
            if parts.len() >= 6 {
                user_cpu_seconds = parts.get(2).and_then(|value| value.parse::<f64>().ok());
                sys_cpu_seconds = parts.get(4).and_then(|value| value.parse::<f64>().ok());
            }
        } else if trimmed.ends_with("maximum resident set size") {
            peak_rss_bytes =
                trimmed.split_whitespace().next().and_then(|value| value.parse::<u64>().ok());
        }
    }
    Ok(ResourceMeasurement {
        peak_rss_bytes: peak_rss_bytes.context("bsd time output missing peak rss")?,
        user_cpu_seconds: user_cpu_seconds.context("bsd time output missing user cpu")?,
        sys_cpu_seconds: sys_cpu_seconds.context("bsd time output missing sys cpu")?,
    })
}

fn parse_gnu_time_output(stderr: &str) -> Result<ResourceMeasurement> {
    let mut user_cpu_seconds = None;
    let mut sys_cpu_seconds = None;
    let mut peak_rss_bytes = None;
    for line in stderr.lines() {
        if let Some(value) = line.strip_prefix("\tUser time (seconds): ") {
            user_cpu_seconds = value.trim().parse::<f64>().ok();
        } else if let Some(value) = line.strip_prefix("\tSystem time (seconds): ") {
            sys_cpu_seconds = value.trim().parse::<f64>().ok();
        } else if let Some(value) = line.strip_prefix("\tMaximum resident set size (kbytes): ") {
            peak_rss_bytes = value.trim().parse::<u64>().ok().map(|kb| kb * 1024);
        }
    }
    Ok(ResourceMeasurement {
        peak_rss_bytes: peak_rss_bytes.context("gnu time output missing peak rss")?,
        user_cpu_seconds: user_cpu_seconds.context("gnu time output missing user cpu")?,
        sys_cpu_seconds: sys_cpu_seconds.context("gnu time output missing sys cpu")?,
    })
}

fn aggregate_python_impl_report(
    per_run_reports: &[PythonImplComparisonReport],
    comparisons: &[PythonImplComparison],
    resource_measurements: &BTreeMap<String, ResourceMeasurementSet>,
    profile: PythonImplBenchProfile,
    compare_runs: usize,
    resource_runs: usize,
    baseline_resource_iterations: usize,
) -> Result<PythonImplReportSummary> {
    let environment = per_run_reports
        .first()
        .context("at least one compare run is required")?
        .environment
        .clone();
    let mut aggregated = Vec::new();

    for comparison in comparisons {
        let matching_rows = per_run_reports
            .iter()
            .map(|report| {
                report
                    .comparisons
                    .iter()
                    .find(|row| row.label == comparison.label)
                    .cloned()
                    .with_context(|| format!("missing comparison row `{}`", comparison.label))
            })
            .collect::<Result<Vec<_>>>()?;
        let rust = median_bench_stats(
            &matching_rows.iter().map(|row| row.rust.clone()).collect::<Vec<_>>(),
        );
        let python = median_bench_stats(
            &matching_rows.iter().map(|row| row.python.clone()).collect::<Vec<_>>(),
        );
        let rust_resources = aggregate_resource_stats(
            resource_measurements
                .get(&format!("rust:{}", comparison.rust_benchmark))
                .with_context(|| {
                    format!(
                        "missing rust resource measurements for `{}`",
                        comparison.rust_benchmark
                    )
                })?,
        );
        let python_resources = aggregate_resource_stats(
            resource_measurements
                .get(&format!("python:{}", comparison.python_benchmark))
                .with_context(|| {
                    format!(
                        "missing python resource measurements for `{}`",
                        comparison.python_benchmark
                    )
                })?,
        );
        aggregated.push(PythonImplReportComparison {
            label: comparison.label.clone(),
            rust_benchmark: comparison.rust_benchmark.clone(),
            python_benchmark: comparison.python_benchmark.clone(),
            context: BenchContext {
                workload_class: comparison.workload_class.clone(),
                payload_size_bytes: comparison.payload_size_bytes,
                batch_size: comparison.batch_size,
            },
            rust: rust.clone(),
            python: python.clone(),
            rust_advantage_vs_python: bench_advantage(&rust, &python),
            rust_resources: rust_resources.clone(),
            python_resources: python_resources.clone(),
            rust_resource_advantage_vs_python: ResourceAdvantage {
                rss_reduction: reduction(
                    python_resources.median_peak_rss_bytes as f64,
                    rust_resources.median_peak_rss_bytes as f64,
                ),
                cpu_time_reduction: reduction(
                    python_resources.median_cpu_seconds_per_1k_ops,
                    rust_resources.median_cpu_seconds_per_1k_ops,
                ),
            },
        });
    }

    Ok(PythonImplReportSummary {
        profile: match profile {
            PythonImplBenchProfile::Fast => "fast".to_string(),
            PythonImplBenchProfile::Report => "report".to_string(),
        },
        compare_runs,
        resource_runs,
        resource_iterations: baseline_resource_iterations,
        environment,
        comparisons: aggregated,
    })
}

fn aggregate_report_rows_by_label(
    per_run_reports: &[PythonImplComparisonReport],
) -> Result<BTreeMap<String, PythonImplComparisonRow>> {
    let mut rows = BTreeMap::new();
    let comparisons =
        &per_run_reports.first().context("at least one compare run is required")?.comparisons;
    for comparison in comparisons {
        let matching_rows = per_run_reports
            .iter()
            .map(|report| {
                report
                    .comparisons
                    .iter()
                    .find(|row| row.label == comparison.label)
                    .cloned()
                    .with_context(|| format!("missing comparison row `{}`", comparison.label))
            })
            .collect::<Result<Vec<_>>>()?;
        rows.insert(
            comparison.label.clone(),
            PythonImplComparisonRow {
                label: comparison.label.clone(),
                rust_benchmark: comparison.rust_benchmark.clone(),
                python_benchmark: comparison.python_benchmark.clone(),
                context: comparison.context.clone(),
                rust: median_bench_stats(
                    &matching_rows.iter().map(|row| row.rust.clone()).collect::<Vec<_>>(),
                ),
                python: median_bench_stats(
                    &matching_rows.iter().map(|row| row.python.clone()).collect::<Vec<_>>(),
                ),
                rust_speedup_vs_python: median_bench_stats(
                    &matching_rows
                        .iter()
                        .map(|row| row.rust_speedup_vs_python.clone())
                        .collect::<Vec<_>>(),
                ),
                rust_advantage_vs_python: comparison.rust_advantage_vs_python.clone(),
            },
        );
    }
    Ok(rows)
}

fn resource_iterations_for_duration(
    baseline_iterations: usize,
    p50_ns: f64,
    min_duration_seconds: f64,
) -> usize {
    let target_iterations = ((min_duration_seconds * 1_000_000_000.0) / p50_ns.max(1.0)).ceil();
    baseline_iterations.max(target_iterations as usize)
}

fn median_bench_stats(values: &[BenchStats]) -> BenchStats {
    BenchStats {
        iterations: median_usize(values.iter().map(|entry| entry.iterations).collect()),
        sample_count: median_usize(values.iter().map(|entry| entry.sample_count).collect()),
        mean_ns: median_f64(values.iter().map(|entry| entry.mean_ns).collect()),
        p50_ns: median_f64(values.iter().map(|entry| entry.p50_ns).collect()),
        p95_ns: median_f64(values.iter().map(|entry| entry.p95_ns).collect()),
        p99_ns: median_f64(values.iter().map(|entry| entry.p99_ns).collect()),
        throughput_ops_per_sec: median_f64(
            values.iter().map(|entry| entry.throughput_ops_per_sec).collect(),
        ),
    }
}

fn aggregate_resource_stats(resource_set: &ResourceMeasurementSet) -> ResourceStats {
    let measurements = &resource_set.measurements;
    let peak_rss_values = measurements.iter().map(|entry| entry.peak_rss_bytes).collect::<Vec<_>>();
    let user_values = measurements.iter().map(|entry| entry.user_cpu_seconds).collect::<Vec<_>>();
    let sys_values = measurements.iter().map(|entry| entry.sys_cpu_seconds).collect::<Vec<_>>();
    let cpu_per_k_values = measurements
        .iter()
        .map(|entry| {
            ((entry.user_cpu_seconds + entry.sys_cpu_seconds) * 1000.0)
                / resource_set.iterations_per_run as f64
        })
        .collect::<Vec<_>>();
    ResourceStats {
        runs: measurements.len(),
        iterations_per_run: resource_set.iterations_per_run,
        mean_peak_rss_bytes: peak_rss_values.iter().map(|value| *value as f64).sum::<f64>()
            / peak_rss_values.len() as f64,
        median_peak_rss_bytes: median_u64(peak_rss_values.clone()),
        max_peak_rss_bytes: peak_rss_values.into_iter().max().unwrap_or(0),
        mean_user_cpu_seconds: user_values.iter().sum::<f64>() / user_values.len() as f64,
        median_user_cpu_seconds: median_f64(user_values),
        mean_sys_cpu_seconds: sys_values.iter().sum::<f64>() / sys_values.len() as f64,
        median_sys_cpu_seconds: median_f64(sys_values),
        mean_cpu_seconds_per_1k_ops: cpu_per_k_values.iter().sum::<f64>()
            / cpu_per_k_values.len() as f64,
        median_cpu_seconds_per_1k_ops: median_f64(cpu_per_k_values),
    }
}

fn bench_advantage(rust: &BenchStats, python: &BenchStats) -> BenchAdvantage {
    BenchAdvantage {
        mean_speedup: ratio(python.mean_ns, rust.mean_ns),
        p50_speedup: ratio(python.p50_ns, rust.p50_ns),
        p95_speedup: ratio(python.p95_ns, rust.p95_ns),
        p99_speedup: ratio(python.p99_ns, rust.p99_ns),
        throughput_gain: ratio(rust.throughput_ops_per_sec, python.throughput_ops_per_sec),
        mean_latency_reduction: reduction(python.mean_ns, rust.mean_ns),
        p50_latency_reduction: reduction(python.p50_ns, rust.p50_ns),
        p95_latency_reduction: reduction(python.p95_ns, rust.p95_ns),
        p99_latency_reduction: reduction(python.p99_ns, rust.p99_ns),
    }
}

fn write_python_impl_report_summary(summary: &PythonImplReportSummary) -> Result<()> {
    fs::create_dir_all(PYTHON_IMPL_REPORT_DIR)
        .with_context(|| format!("create {}", PYTHON_IMPL_REPORT_DIR))?;
    fs::write(
        PYTHON_IMPL_REPORT_JSON_PATH,
        serde_json::to_string_pretty(summary).context("serialize benchmark report summary")?,
    )
    .with_context(|| format!("write {PYTHON_IMPL_REPORT_JSON_PATH}"))?;

    let mut lines = Vec::new();
    lines.push("# Python Implementation Benchmark Report".to_string());
    lines.push(String::new());
    lines.push(format!("- Profile: `{}`", summary.profile));
    lines.push(format!("- Compare runs: {}", summary.compare_runs));
    lines.push(format!("- Resource runs: {}", summary.resource_runs));
    lines.push(format!("- Resource iterations per run: {}", summary.resource_iterations));
    lines.push(format!("- Git commit: `{}`", summary.environment.git_commit));
    lines.push(format!("- Host: `{}`", summary.environment.uname));
    lines.push(String::new());
    for comparison in &summary.comparisons {
        lines.push(format!("## {}", comparison.label));
        let mut context_parts = Vec::new();
        if let Some(workload_class) = &comparison.context.workload_class {
            context_parts.push(format!("workload_class={workload_class}"));
        }
        if let Some(payload_size_bytes) = comparison.context.payload_size_bytes {
            context_parts.push(format!("payload_size_bytes={payload_size_bytes}"));
        }
        if let Some(batch_size) = comparison.context.batch_size {
            context_parts.push(format!("batch_size={batch_size}"));
        }
        if !context_parts.is_empty() {
            lines.push(format!("- Context: {}", context_parts.join(" ")));
        }
        lines.push(format!(
            "- Timing: rust_p50_ns={:.2} python_p50_ns={:.2} rust_speedup={:.2}x throughput_gain={:.2}x",
            comparison.rust.p50_ns,
            comparison.python.p50_ns,
            comparison.rust_advantage_vs_python.p50_speedup,
            comparison.rust_advantage_vs_python.throughput_gain
        ));
        lines.push(format!(
            "- Resources: rust_peak_rss_bytes={} python_peak_rss_bytes={} rss_reduction={:.2}% rust_cpu_seconds_per_1k_ops={:.6} python_cpu_seconds_per_1k_ops={:.6} cpu_reduction={:.2}%",
            comparison.rust_resources.median_peak_rss_bytes,
            comparison.python_resources.median_peak_rss_bytes,
            comparison.rust_resource_advantage_vs_python.rss_reduction * 100.0,
            comparison.rust_resources.median_cpu_seconds_per_1k_ops,
            comparison.python_resources.median_cpu_seconds_per_1k_ops,
            comparison.rust_resource_advantage_vs_python.cpu_time_reduction * 100.0
        ));
        lines.push(String::new());
    }
    fs::write(PYTHON_IMPL_REPORT_TEXT_PATH, lines.join("\n"))
        .with_context(|| format!("write {PYTHON_IMPL_REPORT_TEXT_PATH}"))
}

fn median_f64(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn median_u64(mut values: Vec<u64>) -> u64 {
    values.sort_unstable();
    values[values.len() / 2]
}

fn median_usize(mut values: Vec<usize>) -> usize {
    values.sort_unstable();
    values[values.len() / 2]
}

fn load_python_impl_bench_config() -> Result<PythonImplBenchConfig> {
    let raw = fs::read_to_string(PYTHON_IMPL_BENCH_CONFIG_PATH)
        .with_context(|| format!("read {PYTHON_IMPL_BENCH_CONFIG_PATH}"))?;
    toml::from_str(&raw).with_context(|| format!("parse {PYTHON_IMPL_BENCH_CONFIG_PATH}"))
}

fn capture_python_impl_environment() -> Result<PythonImplEnvironment> {
    let git_commit = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    Ok(PythonImplEnvironment {
        rustc_version: capture_command_stdout("rustc", &["--version"])?,
        cargo_version: capture_command_stdout("cargo", &["--version"])?,
        python_version: capture_command_stdout("python3", &["--version"])?,
        python_rns_module: capture_command_stdout(
            "python3",
            &["-c", "import RNS; print(getattr(RNS, '__file__', 'unknown'))"],
        )?,
        python_lxmf_module: capture_command_stdout(
            "python3",
            &["-c", "import LXMF; print(getattr(LXMF, '__file__', 'unknown'))"],
        )?,
        uname: capture_platform_descriptor()?,
        git_commit,
        benchmark_config_path: PYTHON_IMPL_BENCH_CONFIG_PATH.to_string(),
    })
}

fn capture_platform_descriptor() -> Result<String> {
    #[cfg(target_family = "windows")]
    {
        let release = capture_command_stdout("cmd", &["/C", "ver"]).unwrap_or_else(|_| {
            format!(
                "Windows ({})",
                std::env::var("OS").unwrap_or_else(|_| std::env::consts::OS.to_string())
            )
        });
        let arch = std::env::var("PROCESSOR_ARCHITECTURE")
            .unwrap_or_else(|_| std::env::consts::ARCH.to_string());
        return Ok(format!("{release}; arch={arch}"));
    }

    #[cfg(not(target_family = "windows"))]
    {
        capture_command_stdout("uname", &["-a"])
    }
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

fn run_package_daemon_bundle(version: Option<String>) -> Result<()> {
    let version = release_version_label(version)?;
    let bundle_stem = format!("lxmd-daemon-{version}-{}", release_platform_label());
    let output_dir = Path::new(RELEASE_BUNDLE_OUTPUT_DIR);
    fs::create_dir_all(output_dir).with_context(|| format!("create {}", output_dir.display()))?;

    for (package, binary) in DAEMON_RELEASE_BINARIES {
        run("cargo", &["build", "--release", "-p", package, "--bin", binary])?;
    }

    let staging_dir = output_dir.join(&bundle_stem);
    if staging_dir.exists() {
        fs::remove_dir_all(&staging_dir)
            .with_context(|| format!("remove {}", staging_dir.display()))?;
    }
    fs::create_dir_all(&staging_dir)
        .with_context(|| format!("create {}", staging_dir.display()))?;

    for (_, binary) in DAEMON_RELEASE_BINARIES {
        let binary_name = executable_name(binary);
        let source = Path::new("target").join("release").join(&binary_name);
        let destination = staging_dir.join(&binary_name);
        fs::copy(&source, &destination).with_context(|| {
            format!("copy bundled binary {} -> {}", source.display(), destination.display())
        })?;
    }

    let lxmd_path = Path::new("target").join("release").join(executable_name("lxmd"));
    let example_config = capture_command_stdout(
        lxmd_path.to_str().ok_or_else(|| anyhow!("invalid lxmd path: {}", lxmd_path.display()))?,
        &["--exampleconfig"],
    )?;
    let example_config_path = staging_dir.join("lxmd.example.config");
    fs::write(&example_config_path, example_config.as_bytes())
        .with_context(|| format!("write {}", example_config_path.display()))?;

    let readme_path = staging_dir.join("README.md");
    fs::copy("README.md", &readme_path)
        .with_context(|| format!("copy README.md -> {}", readme_path.display()))?;

    let archive_path = create_release_archive(output_dir, &staging_dir, &bundle_stem)?;
    let archive_bytes = fs::read(&archive_path)
        .with_context(|| format!("read archive {}", archive_path.display()))?;
    let archive_name = archive_path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow!("invalid archive filename: {}", archive_path.display()))?;
    let sha_path = output_dir.join(format!("{archive_name}.sha256"));
    let checksum_line = format!("{}  {archive_name}\n", sha256_hex(&archive_bytes));
    fs::write(&sha_path, checksum_line.as_bytes())
        .with_context(|| format!("write {}", sha_path.display()))?;

    fs::remove_dir_all(&staging_dir)
        .with_context(|| format!("remove {}", staging_dir.display()))?;

    println!("created {}", archive_path.display());
    println!("created {}", sha_path.display());
    Ok(())
}

fn release_version_label(version: Option<String>) -> Result<String> {
    if let Some(version) = version.map(|value| value.trim().to_string()) {
        if !version.is_empty() {
            return Ok(version.replace('/', "-"));
        }
    }

    if let Ok(tag) = capture_command_stdout("git", &["describe", "--tags", "--exact-match"]) {
        if !tag.is_empty() {
            return Ok(tag.replace('/', "-"));
        }
    }

    let manifest = fs::read_to_string("crates/apps/lxmf-cli/Cargo.toml")
        .context("read crates/apps/lxmf-cli/Cargo.toml for release version")?;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("version = ") {
            return Ok(value.trim_matches('"').replace('/', "-"));
        }
    }

    bail!("unable to determine release version for daemon bundle")
}

fn release_platform_label() -> String {
    let os = std::env::consts::OS;
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "x86",
        "arm" => "arm",
        other => other,
    };
    format!("{os}-{arch}")
}

fn create_release_archive(
    output_dir: &Path,
    staging_dir: &Path,
    bundle_stem: &str,
) -> Result<PathBuf> {
    if cfg!(windows) {
        let archive = output_dir.join(format!("{bundle_stem}.zip"));
        if archive.exists() {
            fs::remove_file(&archive).with_context(|| format!("remove {}", archive.display()))?;
        }
        let staging_arg = staging_dir
            .to_str()
            .ok_or_else(|| anyhow!("invalid staging path: {}", staging_dir.display()))?;
        let archive_arg = archive
            .to_str()
            .ok_or_else(|| anyhow!("invalid archive path: {}", archive.display()))?;
        run("tar", &["-a", "-c", "-f", archive_arg, staging_arg])?;
        return Ok(archive);
    }

    let archive = output_dir.join(format!("{bundle_stem}.tar.gz"));
    if archive.exists() {
        fs::remove_file(&archive).with_context(|| format!("remove {}", archive.display()))?;
    }
    let archive_arg =
        archive.to_str().ok_or_else(|| anyhow!("invalid archive path: {}", archive.display()))?;
    let staging_name = staging_dir
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow!("invalid staging path: {}", staging_dir.display()))?;
    run("tar", &["-C", RELEASE_BUNDLE_OUTPUT_DIR, "-czf", archive_arg, staging_name])?;
    Ok(archive)
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

fn run_embedded_native_lock_check() -> Result<()> {
    let lockfile = fs::read_to_string(EMBEDDED_NATIVE_LOCKFILE_PATH)
        .with_context(|| format!("missing {EMBEDDED_NATIVE_LOCKFILE_PATH}"))?;
    let required_markers = [
        "contract_ble_camera_wire_ref =",
        "contract_ble_transport_runtime_ref =",
        "contract_native_embedded_interop_ref =",
        "firmware_repo =",
        "firmware_ref =",
        "owners = [",
        "ci_workflow =",
        "xtask_gate = \"embedded-native-lock-check\"",
    ];
    for marker in required_markers {
        if !lockfile.contains(marker) {
            bail!("embedded native lockfile missing marker '{marker}' in {EMBEDDED_NATIVE_LOCKFILE_PATH}");
        }
    }
    for forbidden in ["<set-me>", "TODO", "TBD"] {
        if lockfile.contains(forbidden) {
            bail!("embedded native lockfile contains unresolved placeholder '{forbidden}'");
        }
    }

    let interop_profile = fs::read_to_string(EMBEDDED_NATIVE_INTEROP_PROFILE_PATH)
        .with_context(|| format!("missing {EMBEDDED_NATIVE_INTEROP_PROFILE_PATH}"))?;
    for marker in [
        "# Native Embedded Interop Profile v1",
        "## Lab Profile Reference",
        "## Normative Encoding Rules",
        "## Transport Invariants",
        "## Canonical Transport Parameters",
        "## Lifecycle Ownership",
        "## Success Response Schemas",
        "## Error Code Mapping",
        "## Fixture Set",
    ] {
        if !interop_profile.contains(marker) {
            bail!(
                "embedded native interop profile missing marker '{marker}' in {EMBEDDED_NATIVE_INTEROP_PROFILE_PATH}"
            );
        }
    }

    for path in [
        BLE_CAMERA_WIRE_CONTRACT_PATH,
        BLE_TRANSPORT_RUNTIME_CONTRACT_PATH,
        EMBEDDED_NATIVE_LAB_PROFILE_PATH,
        EMBEDDED_NATIVE_NODE_CONFIG_PATH,
        EMBEDDED_NATIVE_WORKFLOW_PATH,
    ] {
        if !Path::new(path).exists() {
            bail!("required path missing for embedded native lock check: {path}");
        }
    }

    let lab_profile = fs::read_to_string(EMBEDDED_NATIVE_LAB_PROFILE_PATH)
        .with_context(|| format!("missing {EMBEDDED_NATIVE_LAB_PROFILE_PATH}"))?;
    for marker in [
        "# Native Embedded Lab Profile v1",
        "## Hardware",
        "## Network Profiles",
        "### LAN profile",
        "### Internet-shaped profile",
        "## Measurement Rules",
    ] {
        if !lab_profile.contains(marker) {
            bail!(
                "embedded native lab profile missing marker '{marker}' in {EMBEDDED_NATIVE_LAB_PROFILE_PATH}"
            );
        }
    }

    let node_config = fs::read_to_string(EMBEDDED_NATIVE_NODE_CONFIG_PATH)
        .with_context(|| format!("missing {EMBEDDED_NATIVE_NODE_CONFIG_PATH}"))?;
    for marker in [
        "# Native Embedded Node Config v1",
        "## Schema Version",
        "## Stored Fields",
        "### Node mode",
        "### Wi-Fi",
        "### TCP client",
        "### TCP server",
        "## Lifecycle coupling",
    ] {
        if !node_config.contains(marker) {
            bail!(
                "embedded native node config missing marker '{marker}' in {EMBEDDED_NATIVE_NODE_CONFIG_PATH}"
            );
        }
    }

    for marker in [
        "contract_native_embedded_lab_profile_ref =",
        "contract_native_embedded_node_config_ref =",
        "release_revision_mode = \"pinned\"",
        "tcp_read_timeout_secs = 8",
        "tcp_heartbeat_interval_ms = 30000",
        "capture_hard_max_bytes = 2097152",
    ] {
        if !lockfile.contains(marker) {
            bail!("embedded native lockfile missing marker '{marker}' in {EMBEDDED_NATIVE_LOCKFILE_PATH}");
        }
    }

    Ok(())
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

fn run_embedded_core_check() -> Result<()> {
    run(
        "cargo",
        &["check", "-p", "rns-embedded-core", "--no-default-features", "--features", "alloc"],
    )?;
    run("cargo", &["check", "-p", "rns-embedded-core", "--features", "std"])?;
    run("cargo", &["check", "-p", "rns-embedded-ffi", "--features", "std"])?;
    run(
        "cargo",
        &["check", "-p", "rns-embedded-runtime", "--no-default-features", "--features", "alloc"],
    )?;
    run("cargo", &["check", "-p", "rns-embedded-runtime", "--features", "std"])?;
    run("cargo", &["check", "-p", "lxmf-core", "--no-default-features", "--features", "alloc"])?;
    run("cargo", &["check", "-p", "rns-core", "--no-default-features", "--features", "alloc"])?;
    run("cargo", &["test", "-p", "rns-embedded-core"])?;
    run("cargo", &["test", "-p", "rns-embedded-ffi"])?;
    run("cargo", &["test", "-p", "rns-embedded-runtime"])?;

    let matrix = fs::read_to_string("docs/contracts/sdk-v2-feature-matrix.md")
        .context("missing docs/contracts/sdk-v2-feature-matrix.md")?;
    for marker in [
        "| `lxmf-core` |",
        "| `rns-core` |",
        "| `rns-embedded-ffi` |",
        "| `rns-embedded-runtime` |",
        "`alloc-ready`",
        "`wire_fields` JSON bridge only (`std`-gated module)",
    ] {
        if !matrix.contains(marker) {
            bail!("embedded feature matrix is missing required marker '{marker}'");
        }
    }

    Ok(())
}

fn run_embedded_footprint_check() -> Result<()> {
    run_sdk_memory_budget_check()?;
    run("bash", &["tools/scripts/embedded-footprint-check.sh"])?;

    let report = fs::read_to_string(EMBEDDED_FOOTPRINT_REPORT_PATH)
        .with_context(|| format!("missing {EMBEDDED_FOOTPRINT_REPORT_PATH}"))?;
    for marker in [
        "# Embedded Footprint Report",
        "example_binary_bytes=",
        "embedded_heap_budget_bytes=8388608",
        "embedded_event_queue_budget_bytes=2097152",
        "embedded_attachment_spool_budget_bytes=16777216",
    ] {
        if !report.contains(marker) {
            bail!(
                "embedded footprint report missing required marker '{marker}' in {EMBEDDED_FOOTPRINT_REPORT_PATH}"
            );
        }
    }
    Ok(())
}

fn run_embedded_hil_check() -> Result<()> {
    let runbook = fs::read_to_string(EMBEDDED_HIL_RUNBOOK_PATH)
        .with_context(|| format!("missing {EMBEDDED_HIL_RUNBOOK_PATH}"))?;
    for marker in [
        "# Embedded HIL ESP32 Smoke Runbook",
        "## Required Environment",
        "HIL_SERIAL_PORT",
        "HIL_SEND_SOURCE",
        "HIL_SEND_DESTINATION",
        "## Artifacts",
        "target/hil/esp32-smoke.log",
        "target/hil/esp32-smoke-report.json",
    ] {
        if !runbook.contains(marker) {
            bail!(
                "embedded HIL runbook missing required marker '{marker}' in {EMBEDDED_HIL_RUNBOOK_PATH}"
            );
        }
    }

    run("bash", &["tools/scripts/hil-esp32-smoke.sh"])?;

    let report = fs::read_to_string(EMBEDDED_HIL_REPORT_PATH)
        .with_context(|| format!("missing {EMBEDDED_HIL_REPORT_PATH}"))?;
    if !report.contains("\"status\":\"pass\"") {
        bail!("embedded HIL report does not contain passing status in {EMBEDDED_HIL_REPORT_PATH}");
    }

    Ok(())
}

fn run_embedded_node_build() -> Result<()> {
    run(
        "cargo",
        &["check", "-p", "rns-embedded-core", "--no-default-features", "--features", "alloc"],
    )?;
    run("cargo", &["check", "-p", "rns-embedded-core", "--features", "std"])?;
    run("cargo", &["check", "-p", "rns-tools", "--bin", "rnx"])?;
    run("cargo", &["check", "-p", "reticulumd", "--bin", "reticulumd"])?;
    Ok(())
}

fn run_embedded_node_contract() -> Result<()> {
    run_embedded_native_lock_check()?;

    let profile = fs::read_to_string(EMBEDDED_NATIVE_INTEROP_PROFILE_PATH)
        .with_context(|| format!("missing {EMBEDDED_NATIVE_INTEROP_PROFILE_PATH}"))?;
    for marker in [
        "# Native Embedded Interop Profile v1",
        "## Normative Encoding Rules",
        "## Transport Invariants",
        "## Error Code Mapping",
        "## Fixture Set",
    ] {
        if !profile.contains(marker) {
            bail!(
                "embedded interop profile missing required marker '{marker}' in {EMBEDDED_NATIVE_INTEROP_PROFILE_PATH}"
            );
        }
    }
    Ok(())
}

fn run_embedded_node_failure_matrix() -> Result<()> {
    let failure_matrix = fs::read_to_string("docs/contracts/failure-injection-matrix.md")
        .context("missing docs/contracts/failure-injection-matrix.md")?;
    let sdk_errors = fs::read_to_string("docs/contracts/sdk-v2-errors.md")
        .context("missing docs/contracts/sdk-v2-errors.md")?;

    let required_codes = [
        "SDK_RUNTIME_INVALID_CURSOR",
        "SDK_RUNTIME_NOT_FOUND",
        "SDK_VALIDATION_INVALID_ARGUMENT",
        "SDK_VALIDATION_CHECKSUM_MISMATCH",
        "SDK_VALIDATION_IDEMPOTENCY_CONFLICT",
        "SDK_RUNTIME_SEQ_GAP",
        "SDK_RUNTIME_DISCONNECTED",
        "SDK_RUNTIME_BACKPRESSURE_TIMEOUT",
    ];
    for code in required_codes {
        if !failure_matrix.contains(code) {
            bail!("failure matrix missing required machine code '{code}'");
        }
        if !sdk_errors.contains(code) {
            bail!("sdk-v2-errors contract missing failure-matrix code '{code}'");
        }
    }

    for marker in ["## Required Matrix", "## Test Artifact Requirement"] {
        if !failure_matrix.contains(marker) {
            bail!("failure matrix contract missing required section '{marker}'");
        }
    }
    Ok(())
}

fn run_embedded_node_hil() -> Result<()> {
    run("bash", &[EMBEDDED_NATIVE_INTEROP_SCRIPT_PATH])?;

    let report = fs::read_to_string(EMBEDDED_NATIVE_INTEROP_REPORT_PATH)
        .with_context(|| format!("missing {EMBEDDED_NATIVE_INTEROP_REPORT_PATH}"))?;
    for marker in ["\"status\":\"pass\"", "\"announce_ok\":true", "\"tiny_message_ok\":true"] {
        if !report.contains(marker) {
            bail!(
                "embedded native interop report missing marker '{marker}' in {EMBEDDED_NATIVE_INTEROP_REPORT_PATH}"
            );
        }
    }

    if !Path::new(EMBEDDED_NATIVE_INTEROP_LOG_PATH).exists() {
        bail!("missing embedded native interop log at {EMBEDDED_NATIVE_INTEROP_LOG_PATH}");
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
    run("cargo", &["build", "-p", "reticulumd", "--bin", "reticulumd"])?;
    run("cargo", &["run", "-p", "rns-tools", "--bin", "rnx", "--", "e2e", "--timeout-secs", "20"])
}

fn run_mesh_sim() -> Result<()> {
    run("cargo", &["build", "-p", "reticulumd", "--bin", "reticulumd"])?;
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
        &["-lc", "! grep -RInE 'crates/(lxmf|reticulum|reticulum-daemon)/' README.md .github/workflows || exit 1"],
    )?;
    Ok(())
}

fn run_architecture_checks() -> Result<()> {
    run_architecture_lint_check()?;
    run_module_size_check()
}

fn run_forbidden_deps() -> Result<()> {
    let enforce_legacy_imports =
        std::env::var("ENFORCE_LEGACY_APP_IMPORTS").unwrap_or("1".to_string());
    let enforce_legacy_shims =
        std::env::var("ENFORCE_RETM_LEGACY_SHIMS").unwrap_or("1".to_string());
    run_boundary_checks(&enforce_legacy_imports, &enforce_legacy_shims)
}

fn run_architecture_lint_check() -> Result<()> {
    run_forbidden_deps()?;

    let report = fs::read_to_string(ARCH_BOUNDARY_REPORT_PATH).with_context(|| {
        format!("missing architecture boundary report at {ARCH_BOUNDARY_REPORT_PATH}")
    })?;
    for marker in [
        "# Architecture Boundary Report",
        "## Allowed library edges",
        "## Actual library edges",
        "## Allowed app edges",
        "## Actual app edges",
    ] {
        if !report.contains(marker) {
            bail!("architecture boundary report missing marker '{marker}'");
        }
    }

    Ok(())
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

fn extract_backtick_value(document: &str, marker: &str) -> Option<String> {
    for line in document.lines() {
        if !line.contains(marker) {
            continue;
        }
        let start = line.find('`')?;
        let rest = &line[start + 1..];
        let end = rest.find('`')?;
        return Some(rest[..end].trim().to_string());
    }
    None
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

fn run_proto_check() -> Result<()> {
    compile_proto_tree(None)
}

fn run_proto_generate() -> Result<()> {
    let output_dir = Path::new(GENERATED_GRPC_RUST_DIR);
    if output_dir.exists() {
        fs::remove_dir_all(output_dir)
            .with_context(|| format!("remove {}", output_dir.display()))?;
    }
    compile_proto_tree(Some(output_dir))
}

fn compile_proto_tree(output_dir: Option<&Path>) -> Result<()> {
    let proto_root = Path::new(PROTO_ROOT_PATH);
    let mut proto_files = Vec::new();
    collect_proto_files(proto_root, &mut proto_files)?;
    if proto_files.is_empty() {
        bail!("no proto files found under {}", proto_root.display());
    }

    let out_dir = match output_dir {
        Some(path) => path.to_path_buf(),
        None => std::env::temp_dir().join("lxmf-rs-proto-check"),
    };

    if out_dir.exists() && output_dir.is_none() {
        fs::remove_dir_all(&out_dir).with_context(|| format!("remove {}", out_dir.display()))?;
    }
    fs::create_dir_all(&out_dir).with_context(|| format!("create {}", out_dir.display()))?;

    let descriptor_path = if output_dir.is_some() {
        let descriptor = Path::new(GENERATED_GRPC_DESCRIPTOR_PATH);
        if let Some(parent) = descriptor.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        descriptor.to_path_buf()
    } else {
        out_dir.join("lxmf-descriptor-set.bin")
    };

    let protoc = protoc_bin_vendored::protoc_bin_path().context("resolve vendored protoc")?;
    std::env::set_var("PROTOC", &protoc);
    tonic_prost_build::configure()
        .build_client(true)
        .build_server(true)
        .build_transport(true)
        .out_dir(&out_dir)
        .file_descriptor_set_path(&descriptor_path)
        .protoc_arg("--experimental_allow_proto3_optional")
        .compile_protos(&proto_files, &[proto_root.to_path_buf()])
        .with_context(|| format!("compile proto tree rooted at {}", proto_root.display()))?;

    Ok(())
}

fn collect_proto_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    let mut entries = fs::read_dir(dir)
        .with_context(|| format!("read proto directory {}", dir.display()))?
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("read entries in {}", dir.display()))?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_proto_files(&path, files)?;
        } else if path.extension().is_some_and(|ext| ext == "proto") {
            files.push(path);
        }
    }

    Ok(())
}
