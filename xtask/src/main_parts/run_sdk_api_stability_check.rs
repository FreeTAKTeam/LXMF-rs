fn run_sdk_api_stability_check(current_public_api: &str) -> Result<()> {
    let stability_doc = fs::read_to_string(SDK_API_STABILITY_PATH)
        .with_context(|| format!("missing {SDK_API_STABILITY_PATH}"))?;
    for marker in [
        "# SDK API Stability Classes",
        "## Stability Classes",
        "| Class | Match Prefix | Lifecycle Rule |",
        "## Deprecation Workflow",
    ] {
        if !stability_doc.contains(marker) {
            bail!("stability contract missing marker '{marker}' in {SDK_API_STABILITY_PATH}");
        }
    }

    let rows =
        parse_markdown_table_rows(&stability_doc, &["Class", "Match Prefix", "Lifecycle Rule"])?;
    if rows.is_empty() {
        bail!("stability contract must contain at least one classification row");
    }

    let mut rules = Vec::<(String, String)>::new();
    for row in rows {
        if row.len() < 3 {
            continue;
        }
        let class = row[0].trim().trim_matches('`').to_ascii_lowercase();
        let prefix = row[1].trim().trim_matches('`').to_string();
        let lifecycle = row[2].trim().trim_matches('`');
        if class.is_empty() || prefix.is_empty() || lifecycle.is_empty() {
            continue;
        }
        if !matches!(class.as_str(), "stable" | "experimental" | "internal") {
            bail!("invalid stability class '{class}' in {SDK_API_STABILITY_PATH}");
        }
        rules.push((class, prefix));
    }
    if rules.is_empty() {
        bail!("stability contract has no usable classification rules");
    }

    let mut unmatched = Vec::new();
    let mut matched_rule_indexes = BTreeSet::new();

    for line in current_public_api.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("pub ") || !trimmed.contains("lxmf_sdk::") {
            continue;
        }

        let matched = rules.iter().enumerate().find(|(_, (_, prefix))| trimmed.contains(prefix));

        if let Some((idx, _)) = matched {
            matched_rule_indexes.insert(idx);
        } else {
            unmatched.push(trimmed.to_string());
        }
    }

    if !unmatched.is_empty() {
        let first = unmatched[0].clone();
        bail!(
            "unclassified sdk public api entry '{first}' (update {SDK_API_STABILITY_PATH} rules)"
        );
    }

    for (idx, (_, prefix)) in rules.iter().enumerate() {
        if !matched_rule_indexes.contains(&idx) {
            bail!(
                "stability rule prefix '{prefix}' in {SDK_API_STABILITY_PATH} is stale (matches no public API entries)"
            );
        }
    }

    Ok(())
}

fn run_sdk_migration_check() -> Result<()> {
    const CUTOVER_MAP_PATH: &str = "docs/migrations/sdk-v2.5-cutover-map.md";
    let markdown = fs::read_to_string(CUTOVER_MAP_PATH)
        .with_context(|| format!("missing {CUTOVER_MAP_PATH}"))?;
    let rows = parse_cutover_rows(&markdown)?;
    if rows.is_empty() {
        bail!("cutover map must contain at least one consumer row");
    }

    for (idx, row) in rows.iter().enumerate() {
        let owner = row[2].trim();
        let classification = row[3].trim().to_ascii_lowercase();
        let replacement = row[4].trim();
        let removal_version = row[5].trim();

        if owner.is_empty() {
            bail!("cutover row {idx} missing owner");
        }
        if classification.is_empty() {
            bail!("cutover row {idx} missing classification");
        }
        if replacement.is_empty() {
            bail!("cutover row {idx} missing replacement");
        }
        if removal_version.is_empty() {
            bail!("cutover row {idx} missing removal version");
        }
        if !matches!(classification.as_str(), "keep" | "wrap" | "deprecate") {
            bail!("cutover row {idx} has invalid classification '{classification}'");
        }
        if classification == "wrap" && removal_version.eq_ignore_ascii_case("n/a") {
            bail!("cutover row {idx} classification=wrap requires explicit removal version");
        }
    }

    Ok(())
}

fn run_changelog_migration_check() -> Result<()> {
    let migration_contract_path = "docs/contracts/sdk-v2-migration.md";
    let migration_contract = fs::read_to_string(migration_contract_path)
        .with_context(|| format!("missing {migration_contract_path}"))?;
    for marker in [
        "## Machine-Checkable Migration Gates",
        "cargo xtask sdk-migration-check",
        "cargo xtask sdk-api-break",
        "cargo xtask changelog-migration-check",
        GENERATED_MIGRATION_NOTES_PATH,
    ] {
        if !migration_contract.contains(marker) {
            bail!(
                "migration contract missing required marker '{marker}' in {migration_contract_path}"
            );
        }
    }

    let sdk_contract_path = "docs/contracts/sdk-v2.md";
    let sdk_contract = fs::read_to_string(sdk_contract_path)
        .with_context(|| format!("missing {sdk_contract_path}"))?;
    let contract_release = extract_backtick_value(&sdk_contract, "Contract release:")
        .with_context(|| format!("unable to parse contract release from {sdk_contract_path}"))?;
    let schema_namespace = extract_backtick_value(&sdk_contract, "Schema namespace:")
        .with_context(|| format!("unable to parse schema namespace from {sdk_contract_path}"))?;

    let output = format!(
        "# Generated Migration Notes\n\n\
         This file is generated by `cargo xtask changelog-migration-check`.\n\n\
         ## Contract Snapshot\n\n\
         - Contract release: `{contract_release}`\n\
         - Schema namespace: `{schema_namespace}`\n\n\
         ## Required Migration Gates\n\n\
         - `cargo xtask sdk-migration-check`\n\
         - `cargo xtask sdk-api-break`\n\
         - `cargo xtask sdk-schema-check`\n\
         - `cargo xtask sdk-conformance`\n\n\
         ## Release Operator Checklist\n\n\
         1. Validate cutover map ownership and replacement classification.\n\
         2. Confirm alias/deprecation timelines in `docs/contracts/sdk-v2-migration.md`.\n\
         3. Attach this generated note artifact to release readiness evidence.\n"
    );

    if let Some(parent) = Path::new(GENERATED_MIGRATION_NOTES_PATH).parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(GENERATED_MIGRATION_NOTES_PATH, output)
        .with_context(|| format!("write {GENERATED_MIGRATION_NOTES_PATH}"))?;

    let generated = fs::read_to_string(GENERATED_MIGRATION_NOTES_PATH)
        .with_context(|| format!("missing {GENERATED_MIGRATION_NOTES_PATH}"))?;
    for marker in [
        "# Generated Migration Notes",
        "## Contract Snapshot",
        "## Required Migration Gates",
        "## Release Operator Checklist",
        "cargo xtask sdk-migration-check",
        "cargo xtask sdk-api-break",
    ] {
        if !generated.contains(marker) {
            bail!(
                "generated migration notes missing marker '{marker}' in {GENERATED_MIGRATION_NOTES_PATH}"
            );
        }
    }

    Ok(())
}

fn run_governance_check() -> Result<()> {
    let codeowners = fs::read_to_string(CODEOWNERS_PATH)
        .with_context(|| format!("missing {CODEOWNERS_PATH}"))?;

    let parsed_lines: Vec<(&str, Vec<&str>)> = codeowners
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let mut fields = line.split_whitespace();
            let path = fields.next().unwrap_or_default();
            let owners = fields.collect::<Vec<_>>();
            (path, owners)
        })
        .collect();

    for forbidden in GOVERNANCE_FORBIDDEN_CODEOWNER_PATHS {
        if parsed_lines.iter().any(|(path, _)| path == forbidden) {
            bail!("CODEOWNERS contains deprecated path '{forbidden}'");
        }
    }

    for required_path in GOVERNANCE_REQUIRED_CODEOWNER_PATHS {
        let owners = parsed_lines
            .iter()
            .find_map(|(path, owners)| (*path == *required_path).then_some(owners))
            .with_context(|| {
                format!("CODEOWNERS missing required ownership entry '{required_path}'")
            })?;

        if owners.is_empty() {
            bail!("CODEOWNERS entry '{required_path}' must declare at least one owner");
        }
        if !owners.iter().any(|owner| owner.starts_with('@')) {
            bail!("CODEOWNERS entry '{required_path}' must use explicit GitHub owner handles");
        }
        if !owners.contains(&"@FreeTAKTeam") {
            bail!("CODEOWNERS entry '{required_path}' must include @FreeTAKTeam");
        }
    }

    let security_policy = fs::read_to_string(SECURITY_POLICY_DOC_PATH)
        .with_context(|| format!("missing {SECURITY_POLICY_DOC_PATH}"))?;
    for marker in [
        "# Security Policy",
        "## Reporting a Vulnerability",
        "## Coordinated Disclosure Workflow",
        "docs/runbooks/cve-response-workflow.md",
    ] {
        if !security_policy.contains(marker) {
            bail!("security policy missing marker '{marker}' in {SECURITY_POLICY_DOC_PATH}");
        }
    }

    let cve_runbook = fs::read_to_string(CVE_RESPONSE_RUNBOOK_PATH)
        .with_context(|| format!("missing {CVE_RESPONSE_RUNBOOK_PATH}"))?;
    for marker in [
        "# CVE Disclosure and Response Workflow",
        "## Intake and Triage",
        "## Severity Classification",
        "## Patch and Backport Process",
        "## Advisory Publication",
        "## Evidence Checklist",
    ] {
        if !cve_runbook.contains(marker) {
            bail!("cve runbook missing marker '{marker}' in {CVE_RESPONSE_RUNBOOK_PATH}");
        }
    }

    let release_readiness = fs::read_to_string("docs/runbooks/release-readiness.md")
        .context("missing docs/runbooks/release-readiness.md")?;
    if !release_readiness.contains("docs/runbooks/cve-response-workflow.md") {
        bail!("release readiness runbook must reference docs/runbooks/cve-response-workflow.md");
    }

    Ok(())
}

fn run_compliance_profile_check() -> Result<()> {
    let runbook = fs::read_to_string(COMPLIANCE_PROFILES_RUNBOOK_PATH)
        .with_context(|| format!("missing {COMPLIANCE_PROFILES_RUNBOOK_PATH}"))?;
    for marker in [
        "# Compliance Deployment Profiles",
        "## Objectives",
        "## Profile: Regulated Baseline",
        "## Profile: Regulated Strict",
        "## Audit Logging and Evidence",
        "## Release Gate Mapping",
        "## Operational Checklist",
        "regulated-baseline",
        "regulated-strict",
        "key-management-check",
    ] {
        if !runbook.contains(marker) {
            bail!(
                "compliance profiles runbook missing marker '{marker}' in {COMPLIANCE_PROFILES_RUNBOOK_PATH}"
            );
        }
    }

    let matrix = fs::read_to_string(SDK_FEATURE_MATRIX_PATH)
        .with_context(|| format!("missing {SDK_FEATURE_MATRIX_PATH}"))?;
    for marker in [
        "## Compliance Deployment Profiles",
        "docs/runbooks/compliance-profiles.md",
        "regulated-baseline",
        "regulated-strict",
    ] {
        if !matrix.contains(marker) {
            bail!(
                "feature matrix missing compliance marker '{marker}' in {SDK_FEATURE_MATRIX_PATH}"
            );
        }
    }

    let release_readiness = fs::read_to_string("docs/runbooks/release-readiness.md")
        .context("missing docs/runbooks/release-readiness.md")?;
    for marker in [
        "compliance-profile-check",
        "cargo run -p xtask -- compliance-profile-check",
        "docs/runbooks/compliance-profiles.md",
    ] {
        if !release_readiness.contains(marker) {
            bail!("release readiness runbook missing compliance marker '{marker}'");
        }
    }

    Ok(())
}

fn run_support_policy_check() -> Result<()> {
    let support_policy = fs::read_to_string(SUPPORT_POLICY_PATH)
        .with_context(|| format!("missing {SUPPORT_POLICY_PATH}"))?;
    for marker in [
        "# Version Support and LTS Policy",
        "## Release Channels",
        "## LTS Selection Rules",
        "## Deprecation and Removal Policy",
        "## Compliance Gates",
        "| `Current (N)` |",
        "| `Maintenance (N-1)` |",
        "| `LTS` |",
        "| `EOL` |",
    ] {
        if !support_policy.contains(marker) {
            bail!("support policy missing required marker '{marker}' in {SUPPORT_POLICY_PATH}");
        }
    }

    let readme = fs::read_to_string("README.md").context("missing README.md")?;
    if !readme.contains("docs/contracts/support-policy.md") {
        bail!("README.md must reference docs/contracts/support-policy.md");
    }

    let migration = fs::read_to_string("docs/contracts/sdk-v2-migration.md")
        .context("missing docs/contracts/sdk-v2-migration.md")?;
    if !migration.contains("docs/contracts/support-policy.md") {
        bail!("sdk-v2 migration contract must reference docs/contracts/support-policy.md");
    }

    let release_readiness = fs::read_to_string("docs/runbooks/release-readiness.md")
        .context("missing docs/runbooks/release-readiness.md")?;
    if !release_readiness.contains("support-policy-check") {
        bail!("release readiness checklist must include support-policy-check gate");
    }

    Ok(())
}

fn run_unsafe_audit_check() -> Result<()> {
    let policy = fs::read_to_string(UNSAFE_POLICY_PATH)
        .with_context(|| format!("missing {UNSAFE_POLICY_PATH}"))?;
    for marker in [
        "# Unsafe Code Policy",
        "## Guardrails",
        "## Inventory Process",
        "## Reviewer Requirements",
        "## CI Gate",
        "tools/scripts/check-unsafe.sh",
        "cargo xtask ci --stage unsafe-audit-check",
    ] {
        if !policy.contains(marker) {
            bail!("unsafe policy missing required marker '{marker}' in {UNSAFE_POLICY_PATH}");
        }
    }

    let inventory = fs::read_to_string(UNSAFE_INVENTORY_PATH)
        .with_context(|| format!("missing {UNSAFE_INVENTORY_PATH}"))?;
    for marker in [
        "# Unsafe Inventory",
        "## Active Unsafe Entries",
        "| Id | File | Line | Safety Invariant | Owner | Last Reviewed |",
    ] {
        if !inventory.contains(marker) {
            bail!("unsafe inventory missing required marker '{marker}' in {UNSAFE_INVENTORY_PATH}");
        }
    }

    let adr = fs::read_to_string(UNSAFE_GOVERNANCE_ADR_PATH)
        .with_context(|| format!("missing {UNSAFE_GOVERNANCE_ADR_PATH}"))?;
    for marker in [
        "# ADR 0006: Unsafe Code Audit Governance",
        "- Status: Accepted",
        "tools/scripts/check-unsafe.sh",
    ] {
        if !adr.contains(marker) {
            bail!("unsafe governance adr missing required marker '{marker}' in {UNSAFE_GOVERNANCE_ADR_PATH}");
        }
    }

    run("bash", &[UNSAFE_AUDIT_SCRIPT_PATH])?;

    let codeowners = fs::read_to_string(CODEOWNERS_PATH)
        .with_context(|| format!("missing {CODEOWNERS_PATH}"))?;
    for entry in [
        "/docs/architecture/unsafe-code-policy.md @FreeTAKTeam",
        "/docs/architecture/unsafe-inventory.md @FreeTAKTeam",
        "/docs/adr/0006-unsafe-code-audit-governance.md @FreeTAKTeam",
        "/tools/scripts/check-unsafe.sh @FreeTAKTeam",
    ] {
        if !codeowners.contains(entry) {
            bail!("CODEOWNERS missing unsafe governance entry '{entry}'");
        }
    }

    Ok(())
}
