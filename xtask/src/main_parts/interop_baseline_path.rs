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

const BLOCKING_SLEEP_SCAN_ROOTS: &[&str] = &[
    "crates/apps/reticulumd/src",
    "crates/libs/lxmf-sdk/src",
    "crates/libs/rns-rpc/src",
    "crates/libs/rns-transport/src",
];

const BLOCKING_SLEEP_ALLOWLIST: &[(&str, &str)] =
    &[("crates/libs/lxmf-sdk/src/app/control.rs", "std::thread::sleep")];

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

const DAEMON_RELEASE_BINARIES: &[(&str, &str)] = &[
    ("lxmf-cli", "lxmd"),
    ("lxmf-cli", "lxmf"),
    ("lxmf-cli", "lxmf-cli"),
    ("reticulumd", "reticulumd"),
    ("reticulumd", "lxm-interchange"),
    ("rns-tools", "rnsd"),
    ("rns-tools", "rnstatus-rs"),
    ("rns-tools", "rnx"),
];

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

const RELEASE_BINARIES: &[&str] = &[
    "lxmd",
    "lxmf",
    "lxmf-cli",
    "reticulumd",
    "lxm-interchange",
    "rnsd",
    "rnstatus-rs",
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

#[derive(Copy, Clone, Debug)]
struct PublishedCrate {
    package: &'static str,
    manifest_path: &'static str,
}

const WAVE1_PUBLIC_CRATES: &[PublishedCrate] = &[
    PublishedCrate {
        package: "lxmf-reference",
        manifest_path: "crates/libs/lxmf-reference/Cargo.toml",
    },
    PublishedCrate {
        package: "reticulum-rs-core",
        manifest_path: "crates/libs/rns-core/Cargo.toml",
    },
    PublishedCrate { package: "lxmf-wire", manifest_path: "crates/libs/lxmf-core/Cargo.toml" },
    PublishedCrate {
        package: "reticulum-rs-transport",
        manifest_path: "crates/libs/rns-transport/Cargo.toml",
    },
    PublishedCrate { package: "reticulum-rs-rpc", manifest_path: "crates/libs/rns-rpc/Cargo.toml" },
    PublishedCrate { package: "lxmf-sdk", manifest_path: "crates/libs/lxmf-sdk/Cargo.toml" },
];

const FACADE_PUBLIC_CRATES: &[PublishedCrate] = &[
    PublishedCrate {
        package: "reticulum-rs",
        manifest_path: "crates/libs/reticulum-rs/Cargo.toml",
    },
    PublishedCrate { package: "lxmf", manifest_path: "crates/libs/lxmf/Cargo.toml" },
];

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
        path: "docs/sdk/polling-to-events-migration.md",
        headings: &[
            "# Polling to Events Migration",
            "## Migration Target",
            "## Before: Periodic Polling",
            "## After: Native Event Stream",
            "## Recovery Fallback",
            "## Delivery State Changes",
            "## Shutdown Changes",
        ],
    },
    RequiredSdkDoc {
        path: "docs/sdk/remote-mtls.md",
        headings: &[
            "# SDK Remote mTLS Example",
            "## When to Use mTLS",
            "## Certificate Inputs",
            "## Start `reticulumd`",
            "## Configure the SDK Client",
            "## Event Streams",
            "## Rotation and Recovery",
        ],
    },
    RequiredSdkDoc {
        path: "docs/sdk/error-handling.md",
        headings: &[
            "# SDK Error Handling Guide",
            "## Error Shape",
            "## Retry Policy",
            "## Idempotency",
            "## Queue Pressure",
            "## Connectivity and Runtime Failures",
            "## Security Failures",
        ],
    },
    RequiredSdkDoc {
        path: "docs/sdk/delivery-states.md",
        headings: &[
            "# SDK Delivery State Guide",
            "## State Model",
            "## State Meanings",
            "## Terminality",
            "## Send Acceptance Versus Delivery",
            "## Event Ordering",
            "## Recovery and Reconciliation",
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
    "- [x] docs/sdk/polling-to-events-migration.md",
    "- [x] docs/sdk/remote-mtls.md",
    "- [x] docs/sdk/delivery-states.md",
    "- [x] docs/sdk/error-handling.md",
    "- [x] docs/sdk/advanced-embedding.md",
    "- [x] README.md includes SDK guide links",
    "- [x] docs/architecture/overview.md links to SDK guide index",
];

const PERF_BUDGETS: &[PerfBudget] = &[
    PerfBudget {
        benchmark: "lxmf_core_message_from_wire",
        max_p50_ns: 2_500.0,
        max_p95_ns: 3_500.0,
        max_p99_ns: 4_500.0,
        min_throughput_ops_per_sec: 300_000.0,
    },
    PerfBudget {
        benchmark: "lxmf_core_decode_inbound_message",
        max_p50_ns: 15_000.0,
        max_p95_ns: 25_000.0,
        max_p99_ns: 30_000.0,
        min_throughput_ops_per_sec: 60_000.0,
    },
    PerfBudget {
        benchmark: "lxmf_core_message_to_wire",
        max_p50_ns: 2_500.0,
        max_p95_ns: 4_000.0,
        max_p99_ns: 5_000.0,
        min_throughput_ops_per_sec: 300_000.0,
    },
    PerfBudget {
        benchmark: "lxmf_sdk_start",
        max_p50_ns: 20_000.0,
        max_p95_ns: 25_000.0,
        max_p99_ns: 35_000.0,
        min_throughput_ops_per_sec: 30_000.0,
    },
    PerfBudget {
        benchmark: "lxmf_sdk_send",
        max_p50_ns: 5_000.0,
        max_p95_ns: 10_000.0,
        max_p99_ns: 12_000.0,
        min_throughput_ops_per_sec: 200_000.0,
    },
    PerfBudget {
        benchmark: "lxmf_sdk_poll_events",
        max_p50_ns: 40_000.0,
        max_p95_ns: 50_000.0,
        max_p99_ns: 60_000.0,
        min_throughput_ops_per_sec: 25_000.0,
    },
    PerfBudget {
        benchmark: "lxmf_sdk_snapshot",
        max_p50_ns: 4_000.0,
        max_p95_ns: 6_000.0,
        max_p99_ns: 7_000.0,
        min_throughput_ops_per_sec: 250_000.0,
    },
    PerfBudget {
        benchmark: "rns_rpc_send_message_v2",
        max_p50_ns: 600_000.0,
        max_p95_ns: 700_000.0,
        max_p99_ns: 800_000.0,
        min_throughput_ops_per_sec: 2_000.0,
    },
    PerfBudget {
        benchmark: "rns_rpc_sdk_poll_events_v2",
        max_p50_ns: 35_000.0,
        max_p95_ns: 50_000.0,
        max_p99_ns: 60_000.0,
        min_throughput_ops_per_sec: 30_000.0,
    },
    PerfBudget {
        benchmark: "rns_rpc_sdk_snapshot_v2",
        max_p50_ns: 90_000.0,
        max_p95_ns: 120_000.0,
        max_p99_ns: 150_000.0,
        min_throughput_ops_per_sec: 10_000.0,
    },
    PerfBudget {
        benchmark: "rns_rpc_sdk_topic_create_v2",
        max_p50_ns: 250_000.0,
        max_p95_ns: 450_000.0,
        max_p99_ns: 500_000.0,
        min_throughput_ops_per_sec: 4_000.0,
    },
];

#[derive(Parser)]
#[command(name = "xtask")]
struct Xtask {
    #[command(subcommand)]
    command: XtaskCommand,
}
