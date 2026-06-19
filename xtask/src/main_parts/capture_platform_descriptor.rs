fn capture_platform_descriptor() -> Result<String> {
    #[cfg(target_family = "windows")]
    {
        let release = capture_command_stdout("cmd", &["/C", "ver"]).unwrap_or_else(|_| {
            format!(
                "Windows ({})",
                std::env::var("OS").unwrap_or_else(|_| std::env::consts::OS.to_string())
            )
        });
        let arch = std::env::var("PROCESSOR_ARCHITECTURE")
            .unwrap_or_else(|_| std::env::consts::ARCH.to_string());
        Ok(format!("{release}; arch={arch}"))
    }

    #[cfg(not(target_family = "windows"))]
    {
        capture_command_stdout("uname", &["-a"])
    }
}

fn collect_estimate_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_estimate_files(&path, out)?;
            continue;
        }
        if path.file_name().and_then(|name| name.to_str()) == Some("estimates.json") {
            out.push(path);
        }
    }
    Ok(())
}

fn run_sdk_queue_pressure_check() -> Result<()> {
    run(
        "cargo",
        &[
            "test",
            "-p",
            "reticulum-rs-rpc",
            "sdk_event_queues_remain_bounded_under_sustained_load",
            "--",
            "--nocapture",
        ],
    )
}

#[derive(Debug, Serialize)]
struct SupplyChainProvenance {
    schema_version: u32,
    generated_at_unix_secs: u64,
    git_commit: String,
    rustc_version: String,
    cargo_version: String,
    lockfile_sha256: String,
    artifacts: Vec<SupplyChainArtifact>,
}

#[derive(Debug, Serialize)]
struct SupplyChainArtifact {
    name: String,
    path: String,
    bytes: u64,
    sha256: String,
}

fn run_supply_chain_check() -> Result<()> {
    let metadata_output = Command::new("cargo")
        .args(["metadata", "--locked", "--format-version", "1"])
        .output()
        .context("run cargo metadata for sbom export")?;
    if !metadata_output.status.success() {
        let stderr = String::from_utf8_lossy(&metadata_output.stderr);
        bail!("cargo metadata failed for sbom export: {stderr}");
    }
    write_bytes(SUPPLY_CHAIN_SBOM_PATH, &metadata_output.stdout)?;

    run("cargo", &["build", "--release", "--workspace", "--bins"])?;

    let lockfile = fs::read("Cargo.lock").context("read Cargo.lock for provenance digest")?;
    let lockfile_sha256 = sha256_hex(&lockfile);
    let git_commit = capture_command_stdout("git", &["rev-parse", "HEAD"])?;
    let rustc_version = capture_command_stdout("rustc", &["--version"])?;
    let cargo_version = capture_command_stdout("cargo", &["--version"])?;
    let generated_at_unix_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);

    let mut artifacts = Vec::with_capacity(RELEASE_BINARIES.len());
    for name in RELEASE_BINARIES {
        let binary_name = executable_name(name);
        let path = Path::new("target/release").join(&binary_name);
        if !path.exists() {
            bail!("release artifact missing: {}", path.display());
        }
        let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        artifacts.push(SupplyChainArtifact {
            name: (*name).to_string(),
            path: path.to_string_lossy().replace('\\', "/"),
            bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            sha256: sha256_hex(&bytes),
        });
    }

    let provenance = SupplyChainProvenance {
        schema_version: 1,
        generated_at_unix_secs,
        git_commit,
        rustc_version,
        cargo_version,
        lockfile_sha256,
        artifacts,
    };
    let bytes = serde_json::to_vec_pretty(&provenance).context("serialize supply-chain report")?;
    write_bytes(SUPPLY_CHAIN_PROVENANCE_PATH, &bytes)?;
    let digest = sha256_hex(&bytes);
    let provenance_name = Path::new(SUPPLY_CHAIN_PROVENANCE_PATH)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            anyhow::anyhow!("invalid provenance path: {SUPPLY_CHAIN_PROVENANCE_PATH}")
        })?;
    let signature_payload = format!("{digest}  {provenance_name}\n");
    write_bytes(SUPPLY_CHAIN_SIGNATURE_PATH, signature_payload.as_bytes())?;
    Ok(())
}

fn run_reproducible_build_check() -> Result<()> {
    run("bash", &["tools/scripts/reproducible-build-check.sh"])?;
    if !Path::new(REPRODUCIBLE_BUILD_REPORT_PATH).exists() {
        bail!("reproducible build report is missing at {REPRODUCIBLE_BUILD_REPORT_PATH}");
    }
    Ok(())
}

fn run_package_daemon_bundle(version: Option<String>) -> Result<()> {
    let version = release_version_label(version)?;
    let bundle_stem = format!("lxmf-rs-tools-{version}-{}", release_platform_label());
    let output_dir = Path::new(RELEASE_BUNDLE_OUTPUT_DIR);
    fs::create_dir_all(output_dir).with_context(|| format!("create {}", output_dir.display()))?;

    for (package, binary) in DAEMON_RELEASE_BINARIES {
        run("cargo", &["build", "--release", "-p", package, "--bin", binary])?;
    }

    let staging_dir = output_dir.join(&bundle_stem);
    if staging_dir.exists() {
        fs::remove_dir_all(&staging_dir)
            .with_context(|| format!("remove {}", staging_dir.display()))?;
    }
    fs::create_dir_all(&staging_dir)
        .with_context(|| format!("create {}", staging_dir.display()))?;

    for (_, binary) in DAEMON_RELEASE_BINARIES {
        let binary_name = executable_name(binary);
        let source = Path::new("target").join("release").join(&binary_name);
        let destination = staging_dir.join(&binary_name);
        fs::copy(&source, &destination).with_context(|| {
            format!("copy bundled binary {} -> {}", source.display(), destination.display())
        })?;
    }

    let lxmd_path = Path::new("target").join("release").join(executable_name("lxmd"));
    let example_config = capture_command_stdout(
        lxmd_path.to_str().ok_or_else(|| anyhow!("invalid lxmd path: {}", lxmd_path.display()))?,
        &["--exampleconfig"],
    )?;
    let example_config_path = staging_dir.join("lxmd.example.config");
    fs::write(&example_config_path, example_config.as_bytes())
        .with_context(|| format!("write {}", example_config_path.display()))?;

    let readme_path = staging_dir.join("README.md");
    fs::copy("README.md", &readme_path)
        .with_context(|| format!("copy README.md -> {}", readme_path.display()))?;

    let archive_path = create_release_archive(output_dir, &staging_dir, &bundle_stem)?;
    let archive_bytes = fs::read(&archive_path)
        .with_context(|| format!("read archive {}", archive_path.display()))?;
    let archive_name = archive_path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow!("invalid archive filename: {}", archive_path.display()))?;
    let sha_path = output_dir.join(format!("{archive_name}.sha256"));
    let checksum_line = format!("{}  {archive_name}\n", sha256_hex(&archive_bytes));
    fs::write(&sha_path, checksum_line.as_bytes())
        .with_context(|| format!("write {}", sha_path.display()))?;

    fs::remove_dir_all(&staging_dir)
        .with_context(|| format!("remove {}", staging_dir.display()))?;

    log::info!("created {}", archive_path.display());
    log::info!("created {}", sha_path.display());
    Ok(())
}

fn release_version_label(version: Option<String>) -> Result<String> {
    if let Some(version) = version.map(|value| value.trim().to_string()) {
        if !version.is_empty() {
            return Ok(version.replace('/', "-"));
        }
    }

    let project_version =
        fs::read_to_string("VERSION").context("read project release version from VERSION")?;
    let exact_tag = capture_command_stdout("git", &["describe", "--tags", "--exact-match"]).ok();
    resolve_release_version(None, exact_tag.as_deref(), &project_version)
}

fn resolve_release_version(
    explicit_version: Option<&str>,
    exact_tag: Option<&str>,
    project_version: &str,
) -> Result<String> {
    if let Some(version) = explicit_version.map(str::trim).filter(|value| !value.is_empty()) {
        return Ok(version.replace('/', "-"));
    }

    let project_version = project_version.trim();
    if project_version.is_empty() || project_version.chars().any(char::is_whitespace) {
        bail!("VERSION must contain exactly one non-empty version token");
    }

    if let Some(tag) = exact_tag.map(str::trim).filter(|value| !value.is_empty()) {
        let normalized_tag = tag.strip_prefix('v').unwrap_or(tag);
        let normalized_project = project_version.strip_prefix('v').unwrap_or(project_version);
        if normalized_tag != normalized_project {
            bail!("exact git tag '{tag}' does not match project VERSION '{project_version}'");
        }
        return Ok(tag.replace('/', "-"));
    }

    Ok(project_version.replace('/', "-"))
}

fn release_platform_label() -> String {
    let os = std::env::consts::OS;
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "x86",
        "arm" => "arm",
        other => other,
    };
    format!("{os}-{arch}")
}

fn create_release_archive(
    output_dir: &Path,
    staging_dir: &Path,
    bundle_stem: &str,
) -> Result<PathBuf> {
    if cfg!(windows) {
        let archive = output_dir.join(format!("{bundle_stem}.zip"));
        if archive.exists() {
            fs::remove_file(&archive).with_context(|| format!("remove {}", archive.display()))?;
        }
        let staging_arg = staging_dir
            .to_str()
            .ok_or_else(|| anyhow!("invalid staging path: {}", staging_dir.display()))?;
        let archive_arg = archive
            .to_str()
            .ok_or_else(|| anyhow!("invalid archive path: {}", archive.display()))?;
        run("tar", &["-a", "-c", "-f", archive_arg, staging_arg])?;
        return Ok(archive);
    }

    let archive = output_dir.join(format!("{bundle_stem}.tar.gz"));
    if archive.exists() {
        fs::remove_file(&archive).with_context(|| format!("remove {}", archive.display()))?;
    }
    let archive_arg =
        archive.to_str().ok_or_else(|| anyhow!("invalid archive path: {}", archive.display()))?;
    let staging_name = staging_dir
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow!("invalid staging path: {}", staging_dir.display()))?;
    run("tar", &["-C", RELEASE_BUNDLE_OUTPUT_DIR, "-czf", archive_arg, staging_name])?;
    Ok(archive)
}

fn write_bytes(path: &str, bytes: &[u8]) -> Result<()> {
    let path = Path::new(path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(path, bytes).with_context(|| format!("write {}", path.display()))
}

fn capture_command_stdout(command: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(command)
        .args(args)
        .output()
        .with_context(|| format!("run {command} {}", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{command} {} failed: {stderr}", args.join(" "));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn run_sdk_matrix_check() -> Result<()> {
    run("cargo", &["test", "-p", "test-support", "sdk_matrix", "--", "--nocapture"])
}

fn run_embedded_native_lock_check() -> Result<()> {
    let lockfile = fs::read_to_string(EMBEDDED_NATIVE_LOCKFILE_PATH)
        .with_context(|| format!("missing {EMBEDDED_NATIVE_LOCKFILE_PATH}"))?;
    let required_markers = [
        "contract_ble_camera_wire_ref =",
        "contract_ble_transport_runtime_ref =",
        "contract_native_embedded_interop_ref =",
        "firmware_repo =",
        "firmware_ref =",
        "owners = [",
        "ci_workflow =",
        "xtask_gate = \"embedded-native-lock-check\"",
    ];
    for marker in required_markers {
        if !lockfile.contains(marker) {
            bail!("embedded native lockfile missing marker '{marker}' in {EMBEDDED_NATIVE_LOCKFILE_PATH}");
        }
    }
    for forbidden in ["<set-me>", "TODO", "TBD"] {
        if lockfile.contains(forbidden) {
            bail!("embedded native lockfile contains unresolved placeholder '{forbidden}'");
        }
    }

    let interop_profile = fs::read_to_string(EMBEDDED_NATIVE_INTEROP_PROFILE_PATH)
        .with_context(|| format!("missing {EMBEDDED_NATIVE_INTEROP_PROFILE_PATH}"))?;
    for marker in [
        "# Native Embedded Interop Profile v1",
        "## Lab Profile Reference",
        "## Normative Encoding Rules",
        "## Transport Invariants",
        "## Canonical Transport Parameters",
        "## Lifecycle Ownership",
        "## Success Response Schemas",
        "## Error Code Mapping",
        "## Fixture Set",
    ] {
        if !interop_profile.contains(marker) {
            bail!(
                "embedded native interop profile missing marker '{marker}' in {EMBEDDED_NATIVE_INTEROP_PROFILE_PATH}"
            );
        }
    }

    for path in [
        BLE_CAMERA_WIRE_CONTRACT_PATH,
        BLE_TRANSPORT_RUNTIME_CONTRACT_PATH,
        EMBEDDED_NATIVE_LAB_PROFILE_PATH,
        EMBEDDED_NATIVE_NODE_CONFIG_PATH,
        EMBEDDED_NATIVE_WORKFLOW_PATH,
    ] {
        if !Path::new(path).exists() {
            bail!("required path missing for embedded native lock check: {path}");
        }
    }

    let lab_profile = fs::read_to_string(EMBEDDED_NATIVE_LAB_PROFILE_PATH)
        .with_context(|| format!("missing {EMBEDDED_NATIVE_LAB_PROFILE_PATH}"))?;
    for marker in [
        "# Native Embedded Lab Profile v1",
        "## Hardware",
        "## Network Profiles",
        "### LAN profile",
        "### Internet-shaped profile",
        "## Measurement Rules",
    ] {
        if !lab_profile.contains(marker) {
            bail!(
                "embedded native lab profile missing marker '{marker}' in {EMBEDDED_NATIVE_LAB_PROFILE_PATH}"
            );
        }
    }

    let node_config = fs::read_to_string(EMBEDDED_NATIVE_NODE_CONFIG_PATH)
        .with_context(|| format!("missing {EMBEDDED_NATIVE_NODE_CONFIG_PATH}"))?;
    for marker in [
        "# Native Embedded Node Config v1",
        "## Schema Version",
        "## Stored Fields",
        "### Node mode",
        "### Wi-Fi",
        "### TCP client",
        "### TCP server",
        "## Lifecycle coupling",
    ] {
        if !node_config.contains(marker) {
            bail!(
                "embedded native node config missing marker '{marker}' in {EMBEDDED_NATIVE_NODE_CONFIG_PATH}"
            );
        }
    }

    for marker in [
        "contract_native_embedded_lab_profile_ref =",
        "contract_native_embedded_node_config_ref =",
        "release_revision_mode = \"pinned\"",
        "tcp_read_timeout_secs = 8",
        "tcp_heartbeat_interval_ms = 30000",
        "capture_hard_max_bytes = 2097152",
    ] {
        if !lockfile.contains(marker) {
            bail!("embedded native lockfile missing marker '{marker}' in {EMBEDDED_NATIVE_LOCKFILE_PATH}");
        }
    }

    Ok(())
}
