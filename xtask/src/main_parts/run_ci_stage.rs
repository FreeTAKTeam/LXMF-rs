fn run_ci_stage(stage: CiStage, timeout_secs: Option<u64>) -> Result<()> {
    match stage {
        CiStage::LintFormat => run("cargo", &["fmt", "--all", "--", "--check"]),
        CiStage::BuildMatrix => run("cargo", &["build", "--workspace", "--all-targets"]),
        CiStage::TestNextestUnit => {
            run("cargo", &["nextest", "run", "--workspace", "--lib", "--bins"])
        }
        CiStage::TestIntegration => run("cargo", &["test", "--workspace", "--tests"]),
        CiStage::Doc => run("cargo", &["doc", "--workspace", "--no-deps", "--lib"]),
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
        CiStage::CompatKitCheck => run_compat_kit_check(),
        CiStage::CertificationReportCheck => run_certification_report_check(),
        CiStage::E2eCompatibility => run_e2e_compatibility(timeout_secs),
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
    run("cargo", &["doc", "--workspace", "--no-deps", "--lib"])?;
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
    run_e2e_compatibility(None)?;
    run_sdk_conformance()?;
    run_sdk_profile_build()?;
    run_sdk_examples_check()?;
    run_governance_check()?;
    run_interfaces_required()?;
    run_compliance_profile_check()?;
    run_support_policy_check()?;
    run_unsafe_audit_check()?;
    run_supply_chain_check()?;
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
    run_sdk_fuzz_check()?;
    run_sdk_property_check()?;
    run_sdk_model_check()?;
    run_sdk_race_check()?;
    run_sdk_replay_check()?;
    run_sdk_metrics_check()?;
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
    run("cargo", &["check", "-p", "reticulum-rs-rpc", "--all-targets"])?;
    run("cargo", &["check", "-p", "lxmf-sdk", "--all-targets"])?;
    run("cargo", &["check", "-p", "reticulum-rs-transport", "--all-targets"])?;
    run("cargo", &["test", "-p", "reticulumd", "--test", "config"])?;
    run("cargo", &["test", "-p", "reticulumd", "--bin", "reticulumd"])?;
    run("cargo", &["test", "-p", "reticulum-rs-transport", "serial::tests"])?;
    run("cargo", &["test", "-p", "reticulumd", "--bin", "reticulumd", "interfaces::ble::"])?;
    run("cargo", &["test", "-p", "reticulumd", "--bin", "reticulumd", "lora_state::tests"])?;
    run(
        "cargo",
        &["test", "-p", "reticulum-rs-rpc", "set_interfaces_rejects_startup_only_interface_kinds"],
    )?;
    run(
        "cargo",
        &["test", "-p", "reticulum-rs-rpc", "reload_config_hot_applies_legacy_tcp_only_diff"],
    )?;
    run(
        "cargo",
        &[
            "test",
            "-p",
            "reticulum-rs-rpc",
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
    run("cargo", &["run", "-p", "lxmf-cli", "--bin", "lxmf-cli", "--", "--help"])?;
    run(
        "bash",
        &["-lc", "cargo run -p lxmf-cli --bin lxmf-cli -- completions --shell bash > /dev/null"],
    )
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
