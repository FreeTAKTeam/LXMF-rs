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
    log::info!(
        "python implementation comparison written to {}",
        paths.compare_report_path.display()
    );
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
