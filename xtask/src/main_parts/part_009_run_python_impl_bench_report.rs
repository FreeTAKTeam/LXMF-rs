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
    log::info!(
        "python implementation benchmark report written to {}",
        PYTHON_IMPL_REPORT_TEXT_PATH
    );
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
            "lxmf-wire",
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
            "reticulum-rs-core",
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
            "reticulum-rs-transport",
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
    log::info!("performance budget report written to {PERF_BUDGET_REPORT_PATH}");

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
    log::info!("benchmark summary written to {BENCH_SUMMARY_PATH}");
    Ok(())
}
