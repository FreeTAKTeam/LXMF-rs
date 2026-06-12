fn run_embedded_link_check() -> Result<()> {
    run(
        "cargo",
        &["test", "-p", "reticulum-rs-transport", "--test", "embedded_link_contract", "--no-run"],
    )?;

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
    run("cargo", &["check", "-p", "lxmf-wire", "--no-default-features", "--features", "alloc"])?;
    run(
        "cargo",
        &["check", "-p", "reticulum-rs-core", "--no-default-features", "--features", "alloc"],
    )?;
    run("cargo", &["test", "-p", "rns-embedded-core"])?;
    run("cargo", &["test", "-p", "rns-embedded-ffi"])?;
    run("cargo", &["test", "-p", "rns-embedded-runtime"])?;

    let matrix = fs::read_to_string("docs/contracts/sdk-v2-feature-matrix.md")
        .context("missing docs/contracts/sdk-v2-feature-matrix.md")?;
    for marker in [
        "| `lxmf-wire` |",
        "| `reticulum-rs-core` |",
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

fn run_e2e_compatibility(timeout_secs: Option<u64>) -> Result<()> {
    let timeout_secs = timeout_secs.unwrap_or(20).to_string();
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
            "e2e",
            "--timeout-secs",
            timeout_secs.as_str(),
        ],
    )
}

fn run_e2e_bench(
    mode: E2eBenchMode,
    profile: E2eBenchProfile,
    scenarios: &[String],
    implementations: &[E2eBenchImplementation],
    keep: bool,
    output: Option<&Path>,
    dry_run: bool,
) -> Result<()> {
    const RUNNER_TOOLCHAIN: &str = "1.88.0";
    const RUNNER_MANIFEST: &str = "tools/e2e-runner/Cargo.toml";

    let toolchain_status = Command::new("rustup")
        .args(["run", RUNNER_TOOLCHAIN, "rustc", "--version"])
        .status()
        .context("rustup is required to launch the isolated E2E runner")?;
    if !toolchain_status.success() {
        bail!(
            "Rust {RUNNER_TOOLCHAIN} is required for the isolated E2E runner; install it with \
             `rustup toolchain install {RUNNER_TOOLCHAIN} --profile minimal`"
        );
    }

    let mut args = vec![
        "run".to_string(),
        RUNNER_TOOLCHAIN.to_string(),
        "cargo".to_string(),
        "run".to_string(),
        "--locked".to_string(),
        "--manifest-path".to_string(),
        RUNNER_MANIFEST.to_string(),
        "--".to_string(),
        "--mode".to_string(),
        match mode {
            E2eBenchMode::Correctness => "correctness",
            E2eBenchMode::Benchmark => "benchmark",
            E2eBenchMode::All => "all",
        }
        .to_string(),
        "--profile".to_string(),
        match profile {
            E2eBenchProfile::Smoke => "smoke",
            E2eBenchProfile::Report => "report",
        }
        .to_string(),
    ];
    for scenario in scenarios {
        args.push("--scenario".to_string());
        args.push(scenario.clone());
    }
    for implementation in implementations {
        args.push("--implementation".to_string());
        args.push(
            match implementation {
                E2eBenchImplementation::Rust => "rust",
                E2eBenchImplementation::Python => "python",
                E2eBenchImplementation::Tcp => "tcp",
            }
            .to_string(),
        );
    }
    if keep {
        args.push("--keep".to_string());
    }
    if let Some(output) = output {
        args.push("--output".to_string());
        args.push(output.to_string_lossy().into_owned());
    }
    if dry_run {
        args.push("--dry-run".to_string());
    }

    let status = Command::new("rustup")
        .args(&args)
        .status()
        .context("failed to launch the isolated E2E runner")?;
    if !status.success() {
        bail!("isolated E2E runner failed");
    }
    Ok(())
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
