#[derive(Subcommand)]
enum XtaskCommand {
    Ci {
        #[arg(long)]
        stage: Option<CiStage>,
        #[arg(long)]
        timeout_secs: Option<u64>,
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
    PublishCrates {
        #[arg(long, value_enum, default_value_t = PublishWave::Wave1)]
        wave: PublishWave,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        allow_dirty: bool,
    },
    YankCrate {
        package: String,
        version: String,
        #[arg(long)]
        undo: bool,
    },
    CompatKitCheck,
    E2eCompatibility {
        #[arg(long)]
        timeout_secs: Option<u64>,
    },
    E2eBench {
        #[arg(long, value_enum, default_value_t = E2eBenchMode::All)]
        mode: E2eBenchMode,
        #[arg(long, value_enum, default_value_t = E2eBenchProfile::Smoke)]
        profile: E2eBenchProfile,
        #[arg(long)]
        scenario: Vec<String>,
        #[arg(long, value_enum)]
        implementation: Vec<E2eBenchImplementation>,
        #[arg(long)]
        keep: bool,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
    },
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

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum E2eBenchMode {
    Correctness,
    Benchmark,
    All,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum E2eBenchProfile {
    Smoke,
    Report,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum E2eBenchImplementation {
    Rust,
    Python,
    Tcp,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum PublishWave {
    Wave1,
    Facades,
    All,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let xtask = Xtask::parse();
    match xtask.command {
        XtaskCommand::Ci { stage, timeout_secs } => run_ci(stage, timeout_secs),
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
        XtaskCommand::PublishCrates { wave, dry_run, allow_dirty } => {
            run_publish_crates(wave, dry_run, allow_dirty)
        }
        XtaskCommand::YankCrate { package, version, undo } => {
            run_yank_crate(&package, &version, undo)
        }
        XtaskCommand::CompatKitCheck => run_compat_kit_check(),
        XtaskCommand::E2eCompatibility { timeout_secs } => run_e2e_compatibility(timeout_secs),
        XtaskCommand::E2eBench {
            mode,
            profile,
            scenario,
            implementation,
            keep,
            output,
            dry_run,
        } => run_e2e_bench(
            mode,
            profile,
            &scenario,
            &implementation,
            keep,
            output.as_deref(),
            dry_run,
        ),
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

fn run_ci(stage: Option<CiStage>, timeout_secs: Option<u64>) -> Result<()> {
    if let Some(stage) = stage {
        return run_ci_stage(stage, timeout_secs);
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
    run_sdk_schema_check()?;
    run_publish_crates(PublishWave::All, true, true)?;
    run("cargo", &["check", "-p", "reticulumd", "-p", "rns-tools"])?;
    run("bash", &["tools/scripts/check-boundaries.sh"])?;
    run_cargo_deny_policy_check()?;
    run_cargo_audit()?;
    Ok(())
}
