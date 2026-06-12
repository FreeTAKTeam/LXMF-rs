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
