fn run_interop_drift_check(update: bool) -> Result<()> {
    let current = build_interop_drift_baseline()?;
    if update {
        let serialized =
            serde_json::to_string_pretty(&current).context("serialize interop drift baseline")?;
        fs::write(INTEROP_DRIFT_BASELINE_PATH, format!("{serialized}\n"))
            .with_context(|| format!("write {INTEROP_DRIFT_BASELINE_PATH}"))?;
        return Ok(());
    }

    let baseline_raw = fs::read_to_string(INTEROP_DRIFT_BASELINE_PATH).with_context(|| {
        format!(
            "missing interop drift baseline at {INTEROP_DRIFT_BASELINE_PATH}; run `cargo run -p xtask -- interop-drift-check --update`"
        )
    })?;
    let baseline: InteropDriftBaseline =
        serde_json::from_str(&baseline_raw).context("parse interop drift baseline")?;
    let classification = classify_interop_drift(&baseline, &current);

    for note in &classification.additive {
        log::info!("interop drift additive: {note}");
    }
    if !classification.breaking.is_empty() {
        let details = classification.breaking.join("; ");
        bail!("interop semantic drift detected (breaking): {details}");
    }
    Ok(())
}

fn run_schema_client_check() -> Result<()> {
    let report = client_codegen::run_schema_client_generate(
        Path::new("."),
        Path::new(SCHEMA_CLIENT_MANIFEST_PATH),
        client_codegen::SchemaClientMode::Check,
    )?;
    let failed = report
        .target_compile_checks
        .iter()
        .filter(|(_, status)| status.starts_with("FAIL:"))
        .collect::<Vec<_>>();
    let status = if report.missing_smoke_count == 0 && failed.is_empty() { "PASS" } else { "FAIL" };
    write_schema_client_check_report(&report, status)?;

    if report.missing_smoke_count > 0 {
        bail!("schema client smoke coverage missing {} method vectors", report.missing_smoke_count);
    }

    if !failed.is_empty() {
        let details = failed
            .into_iter()
            .map(|(language, status)| format!("{language}:{status}"))
            .collect::<Vec<_>>();
        bail!("schema client compile checks failed: {}", details.join(", "));
    }

    Ok(())
}

fn run_schema_client_generate(check: bool) -> Result<client_codegen::SchemaClientReport> {
    let mode = if check {
        client_codegen::SchemaClientMode::Check
    } else {
        client_codegen::SchemaClientMode::Write
    };

    let report = client_codegen::run_schema_client_generate(
        Path::new("."),
        Path::new(SCHEMA_CLIENT_MANIFEST_PATH),
        mode,
    )?;
    let failed = report
        .target_compile_checks
        .iter()
        .filter(|(_, status)| status.starts_with("FAIL:"))
        .collect::<Vec<_>>();
    if !failed.is_empty() {
        let details = failed
            .into_iter()
            .map(|(language, status)| format!("{language}:{status}"))
            .collect::<Vec<_>>();
        bail!("schema client compile checks failed: {}", details.join(", "));
    }

    let status = if report.missing_smoke_count == 0 { "PASS" } else { "PASS_WITH_WARNINGS" };
    write_schema_client_check_report(&report, status)?;
    Ok(report)
}

fn write_schema_client_check_report(
    report: &client_codegen::SchemaClientReport,
    status: &str,
) -> Result<()> {
    let output_parent =
        Path::new(SCHEMA_CLIENT_SMOKE_REPORT_PATH).parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(output_parent)
        .with_context(|| format!("create report directory {}", output_parent.display()))?;

    let mut lines = vec![
        format!("manifest_path={}", report.manifest_path.display()),
        format!("spec_path={}", report.spec_path.display()),
        format!("method_count={}", report.method_count),
        format!("spec_hash={}", report.spec_hash),
        format!("missing_smoke_count={}", report.missing_smoke_count),
        format!("methods={}", report.methods.join(",")),
        format!("status={status}"),
    ];
    for (language, hash) in &report.target_hashes {
        lines.push(format!("target.{language}.hash={hash}"));
    }
    for (language, status) in &report.target_compile_checks {
        lines.push(format!("target.{language}.compile={status}"));
    }

    fs::write(SCHEMA_CLIENT_SMOKE_REPORT_PATH, format!("{}\n", lines.join("\n")))
        .with_context(|| format!("write {SCHEMA_CLIENT_SMOKE_REPORT_PATH}"))?;

    Ok(())
}

fn build_interop_drift_baseline() -> Result<InteropDriftBaseline> {
    let corpus_raw = fs::read_to_string(INTEROP_CORPUS_PATH)
        .with_context(|| format!("read {INTEROP_CORPUS_PATH}"))?;
    let corpus: InteropCorpus =
        serde_json::from_str(&corpus_raw).context("parse interop golden corpus")?;

    #[derive(Default)]
    struct ClientAccumulator {
        release_track: String,
        entry_ids: BTreeSet<String>,
        slices: BTreeSet<String>,
        rpc_methods: BTreeSet<String>,
        event_types: BTreeSet<String>,
    }

    let mut by_client: BTreeMap<String, ClientAccumulator> = BTreeMap::new();
    for entry in corpus.entries {
        let slot = by_client.entry(entry.client.clone()).or_default();
        if slot.release_track.is_empty() {
            slot.release_track = entry.release_track.clone();
        }
        slot.entry_ids.insert(entry.id);
        slot.rpc_methods.insert(entry.rpc_send_request.method);
        slot.event_types.insert(entry.event_payload.event_type);
        for slice in entry.slices {
            slot.slices.insert(slice);
        }
    }

    let clients = by_client
        .into_iter()
        .map(|(client, acc)| {
            (
                client,
                InteropClientSummary {
                    release_track: acc.release_track,
                    entry_ids: acc.entry_ids.into_iter().collect(),
                    slices: acc.slices.into_iter().collect(),
                    rpc_methods: acc.rpc_methods.into_iter().collect(),
                    event_types: acc.event_types.into_iter().collect(),
                },
            )
        })
        .collect();

    Ok(InteropDriftBaseline { version: 1, corpus_version: corpus.version, clients })
}

fn classify_interop_drift(
    baseline: &InteropDriftBaseline,
    current: &InteropDriftBaseline,
) -> InteropDriftClassification {
    let mut drift = InteropDriftClassification::default();

    for (client, baseline_summary) in &baseline.clients {
        let Some(current_summary) = current.clients.get(client) else {
            drift.breaking.push(format!("client '{client}' removed from corpus"));
            continue;
        };

        if baseline_summary.release_track != current_summary.release_track {
            drift.breaking.push(format!(
                "client '{client}' release_track changed '{}' -> '{}'",
                baseline_summary.release_track, current_summary.release_track
            ));
        }

        classify_vector_drift(
            &mut drift,
            client,
            "entry_ids",
            &baseline_summary.entry_ids,
            &current_summary.entry_ids,
        );
        classify_vector_drift(
            &mut drift,
            client,
            "slices",
            &baseline_summary.slices,
            &current_summary.slices,
        );
        classify_vector_drift(
            &mut drift,
            client,
            "rpc_methods",
            &baseline_summary.rpc_methods,
            &current_summary.rpc_methods,
        );
        classify_vector_drift(
            &mut drift,
            client,
            "event_types",
            &baseline_summary.event_types,
            &current_summary.event_types,
        );
    }

    for client in current.clients.keys() {
        if !baseline.clients.contains_key(client) {
            drift.additive.push(format!("client '{client}' added to corpus"));
        }
    }

    drift
}

fn classify_vector_drift(
    drift: &mut InteropDriftClassification,
    client: &str,
    field: &str,
    baseline: &[String],
    current: &[String],
) {
    let baseline_set = baseline.iter().cloned().collect::<BTreeSet<_>>();
    let current_set = current.iter().cloned().collect::<BTreeSet<_>>();

    for removed in baseline_set.difference(&current_set) {
        drift.breaking.push(format!(
            "client '{client}' removed {field} value '{removed}' from interop baseline"
        ));
    }
    for added in current_set.difference(&baseline_set) {
        drift
            .additive
            .push(format!("client '{client}' added {field} value '{added}' to interop corpus"));
    }
}

fn build_interop_artifacts_manifest() -> Result<InteropArtifactsManifest> {
    let mut files = Vec::new();
    for root in ["docs/contracts", "docs/schemas", "docs/fixtures"] {
        let root_path = Path::new(root);
        if !root_path.exists() {
            continue;
        }
        collect_files(root_path, &mut files)?;
    }

    files.sort();
    files.dedup();
    let mut entries = Vec::with_capacity(files.len());
    for path in files {
        if path == Path::new(INTEROP_BASELINE_PATH) {
            continue;
        }
        let raw_bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        let bytes = normalize_interop_artifact_bytes(raw_bytes);
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let sha256 = hex::encode(hasher.finalize());
        let relative = path
            .strip_prefix(Path::new("."))
            .unwrap_or(path.as_path())
            .to_string_lossy()
            .replace('\\', "/");
        entries.push(InteropArtifactEntry {
            path: relative,
            bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            sha256,
        });
    }
    entries.sort_by(|left, right| left.path.cmp(&right.path));

    Ok(InteropArtifactsManifest { version: 1, files: entries })
}

fn normalize_interop_artifact_bytes(bytes: Vec<u8>) -> Vec<u8> {
    bytes
        .split(|byte| *byte == b'\n')
        .enumerate()
        .flat_map(|(index, line)| {
            let mut normalized = if line.last() == Some(&b'\r') {
                line[..line.len() - 1].to_vec()
            } else {
                line.to_vec()
            };
            if index > 0 {
                normalized.insert(0, b'\n');
            }
            normalized
        })
        .collect()
}

fn collect_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if root.is_file() {
        files.push(root.to_path_buf());
        return Ok(());
    }
    let mut children = fs::read_dir(root)
        .with_context(|| format!("read dir {}", root.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect::<Vec<_>>();
    children.sort();
    for path in children {
        if path.is_dir() {
            collect_files(path.as_path(), files)?;
        } else if path.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn run_sdk_profile_build() -> Result<()> {
    run(
        "cargo",
        &[
            "check",
            "-p",
            "lxmf-sdk",
            "--no-default-features",
            "--features",
            "std,rpc-backend,sdk-async",
        ],
    )?;
    run(
        "cargo",
        &["check", "-p", "lxmf-sdk", "--no-default-features", "--features", "std,rpc-backend"],
    )?;
    run(
        "cargo",
        &[
            "check",
            "-p",
            "lxmf-sdk",
            "--no-default-features",
            "--features",
            "std,rpc-backend,embedded-alloc",
        ],
    )?;
    Ok(())
}

fn run_sdk_examples_check() -> Result<()> {
    run("cargo", &["test", "-p", "lxmf-sdk", "--examples", "--no-run"])
}

fn run_sdk_api_break() -> Result<()> {
    const BASELINE_PATH: &str = "docs/contracts/baselines/lxmf-sdk-public-api.txt";
    const MANIFEST_PATH: &str = "crates/libs/lxmf-sdk/Cargo.toml";

    let baseline = fs::read_to_string(BASELINE_PATH).with_context(|| {
        format!(
            "missing SDK API baseline at {BASELINE_PATH}; add baseline before enabling sdk-api-break"
        )
    })?;
    let current = capture_public_api(MANIFEST_PATH)?;

    let baseline_normalized = normalize_public_api(&baseline);
    let current_normalized = normalize_public_api(&current);

    if baseline_normalized != current_normalized {
        let diff = public_api_line_diff(&baseline_normalized, &current_normalized);
        bail!(
            "sdk public API drift detected for {MANIFEST_PATH}; review and refresh {BASELINE_PATH}\n{diff}"
        );
    }

    run_sdk_api_stability_check(&current_normalized)?;

    Ok(())
}

fn public_api_line_diff(baseline: &str, current: &str) -> String {
    let baseline_lines = baseline.lines().collect::<BTreeSet<_>>();
    let current_lines = current.lines().collect::<BTreeSet<_>>();
    let removed = baseline_lines.difference(&current_lines).map(|line| format!("- {line}"));
    let added = current_lines.difference(&baseline_lines).map(|line| format!("+ {line}"));
    let line_diff = removed.chain(added).collect::<Vec<_>>();
    if !line_diff.is_empty() {
        return line_diff.join("\n");
    }

    let baseline_lines = baseline.lines().collect::<Vec<_>>();
    let current_lines = current.lines().collect::<Vec<_>>();
    let mismatch = baseline_lines
        .iter()
        .zip(&current_lines)
        .position(|(baseline_line, current_line)| baseline_line != current_line);
    match mismatch {
        Some(index) => format!(
            "first ordering difference at line {}:\nbaseline: {}\ncurrent: {}",
            index + 1,
            baseline_lines[index],
            current_lines[index]
        ),
        None => format!(
            "API line counts differ: baseline={}, current={}",
            baseline_lines.len(),
            current_lines.len()
        ),
    }
}
