fn run_public_benchmark(release: &str, profile: PythonImplBenchProfile) -> Result<()> {
    if release.is_empty() || release.chars().any(|character| matches!(character, '/' | '\\')) {
        bail!("public benchmark release must be a non-empty tag-like name");
    }

    run_python_impl_bench_report(profile, None, None, None, None)?;

    let python = if cfg!(windows) { "python" } else { "python3" };
    let e2e_profile = match profile {
        PythonImplBenchProfile::Fast => "smoke",
        PythonImplBenchProfile::Report => "report",
    };
    run(
        python,
        &[
            "tools/scripts/e2e_performance.py",
            "--profile",
            e2e_profile,
            "--output",
            "target/performance/e2e.json",
        ],
    )?;

    let dashboard_path = Path::new("target/performance/lxmf-rs-performance.html");
    run(
        python,
        &[
            "tools/scripts/performance_docs.py",
            "--release",
            release,
            "--report",
            PYTHON_IMPL_REPORT_JSON_PATH,
            "--e2e-report",
            "target/performance/e2e.json",
            "--dashboard-output",
            dashboard_path
                .to_str()
                .context("dashboard output path must be UTF-8")?,
        ],
    )?;

    let dataset_path = format!("docs/performance/{release}.json");
    let target_dataset = Path::new("target/performance/lxmf-rs-performance.json");
    fs::copy(&dataset_path, target_dataset)
        .with_context(|| format!("copy {dataset_path} to {}", target_dataset.display()))?;

    let checksum_paths = [target_dataset, dashboard_path];
    let mut checksums = String::new();
    for path in checksum_paths {
        let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
        let digest = Sha256::digest(bytes);
        let file_name = path
            .file_name()
            .context("benchmark artifact must have a file name")?
            .to_string_lossy();
        checksums.push_str(&format!("{}  {file_name}\n", hex::encode(digest)));
    }
    fs::write("target/performance/SHA256SUMS", checksums)
        .context("write target/performance/SHA256SUMS")?;
    log::info!(
        "public performance artifacts written under target/performance for {release}"
    );
    Ok(())
}
