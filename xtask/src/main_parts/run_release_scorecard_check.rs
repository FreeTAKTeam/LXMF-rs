fn run_release_scorecard_check() -> Result<()> {
    run("bash", &["-lc", "SCORECARD_MAX_SOAK_FAILURES=1 tools/scripts/release-scorecard.sh"])?;

    let markdown_path = "target/release-scorecard/release-scorecard.md";
    let json_path = "target/release-scorecard/release-scorecard.json";
    let markdown = fs::read_to_string(markdown_path)
        .with_context(|| format!("missing generated scorecard markdown at {markdown_path}"))?;
    let json = fs::read_to_string(json_path)
        .with_context(|| format!("missing generated scorecard json at {json_path}"))?;

    for marker in
        ["# Release Scorecard", "| Overall status |", "| Performance budget status (advisory) |"]
    {
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
    run_no_blocking_sleep_check()?;
    run_no_unbounded_runtime_channel_check()?;
    Ok(())
}

fn is_test_only_source(path_text: &str) -> bool {
    path_text.split('/').any(|component| component == "tests")
        || path_text.ends_with("_tests.rs")
        || path_text.ends_with("/test.rs")
}

fn run_no_blocking_sleep_check() -> Result<()> {
    let mut violations = Vec::new();
    for root in BLOCKING_SLEEP_SCAN_ROOTS {
        let mut files = Vec::new();
        collect_files(Path::new(root), &mut files)?;
        for path in files {
            if path.extension().and_then(|value| value.to_str()) != Some("rs") {
                continue;
            }
            let path_text = path.to_string_lossy().replace('\\', "/");
            if is_test_only_source(&path_text) {
                continue;
            }
            let contents = fs::read_to_string(path.as_path())
                .with_context(|| format!("read {}", path.display()))?;
            let mut in_test_module = false;
            for (idx, line) in contents.lines().enumerate() {
                if line.trim_start().starts_with("#[cfg(test)]") {
                    in_test_module = true;
                }
                if in_test_module {
                    continue;
                }
                if !(line.contains("std::thread::sleep") || line.contains("thread::sleep")) {
                    continue;
                }
                if BLOCKING_SLEEP_ALLOWLIST.iter().any(|(allowed_path, marker)| {
                    path_text == *allowed_path && line.contains(marker)
                }) {
                    continue;
                }
                violations.push(format!("{}:{}: {}", path_text, idx + 1, line.trim()));
            }
        }
    }

    if !violations.is_empty() {
        bail!(
            "blocking thread sleep found in production runtime paths:\n{}",
            violations.join("\n")
        );
    }
    Ok(())
}

fn run_no_unbounded_runtime_channel_check() -> Result<()> {
    let mut violations = Vec::new();
    for root in BLOCKING_SLEEP_SCAN_ROOTS {
        let mut files = Vec::new();
        collect_files(Path::new(root), &mut files)?;
        for path in files {
            if path.extension().and_then(|value| value.to_str()) != Some("rs") {
                continue;
            }
            let path_text = path.to_string_lossy().replace('\\', "/");
            if is_test_only_source(&path_text) {
                continue;
            }
            let contents = fs::read_to_string(path.as_path())
                .with_context(|| format!("read {}", path.display()))?;
            let mut in_test_module = false;
            for (idx, line) in contents.lines().enumerate() {
                if line.trim_start().starts_with("#[cfg(test)]") {
                    in_test_module = true;
                }
                if in_test_module {
                    continue;
                }
                if line.contains("unbounded_channel")
                    || line.contains("UnboundedSender")
                    || line.contains("UnboundedReceiver")
                {
                    violations.push(format!("{}:{}: {}", path_text, idx + 1, line.trim()));
                }
            }
        }
    }

    if !violations.is_empty() {
        bail!("unbounded runtime channel found in production paths:\n{}", violations.join("\n"));
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
