fn run_key_management_check() -> Result<()> {
    run("cargo", &["test", "-p", "reticulum-rs-core", "key_manager", "--", "--nocapture"])?;
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
    run("cargo", &["test", "-p", "reticulum-rs-rpc", "sdk_security", "--", "--nocapture"])
}

fn run_sdk_fuzz_check() -> Result<()> {
    run("cargo", &["check", "--manifest-path", "crates/libs/rns-rpc/fuzz/Cargo.toml"])?;
    run("cargo", &["check", "--manifest-path", "crates/libs/lxmf-sdk/fuzz/Cargo.toml"])?;
    run(
        "cargo",
        &[
            "test",
            "-p",
            "reticulum-rs-rpc",
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
    run("cargo", &["test", "-p", "reticulum-rs-rpc", "sdk_property", "--", "--nocapture"])
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
            "reticulum-rs-rpc",
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
        toolchain_cargo_command(&miri_toolchain, "miri test -p lxmf-wire --lib -- --nocapture");
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
    run("cargo", &["test", "-p", "reticulum-rs-rpc", "sdk_race", "--", "--nocapture"])
}

fn run_sdk_replay_check() -> Result<()> {
    run(
        "cargo",
        &[
            "test",
            "-p",
            "reticulum-rs-rpc",
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
    run("cargo", &["test", "-p", "reticulum-rs-rpc", "rpc::http::tests", "--", "--nocapture"])
}

fn run_sdk_bench_check() -> Result<()> {
    run(
        "cargo",
        &[
            "bench",
            "-p",
            "lxmf-wire",
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
            "reticulum-rs-rpc",
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
    references: PythonImplReferences,
    profiles: PythonImplBenchProfiles,
    comparisons: Vec<PythonImplComparison>,
}

#[derive(Debug, Deserialize)]
struct PythonImplReferences {
    reticulum: String,
    lxmf: String,
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
    python_rns_revision: String,
    python_lxmf_revision: String,
    uname: String,
    cpu: String,
    timestamp_utc: String,
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
        log::warn!(
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
    run_python_impl_bench_compare_with_paths(
        &config,
        profile_config,
        profile_config.python.iterations,
        &paths,
        true,
    )
}
