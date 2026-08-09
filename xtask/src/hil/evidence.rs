use super::config::HilConfig;
use super::model::{ResultClass, RunReport};
use anyhow::{bail, Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub(super) fn write_report(output: &Path, report: &RunReport, config: &HilConfig) -> Result<()> {
    let json = serde_json::to_vec_pretty(report)?;
    fs::write(output.join("result.json"), json).context("write HIL result.json")?;
    let environment = serde_json::json!({
        "run_id": &report.run_id,
        "commit_sha": &report.commit_sha,
        "crate_version": report.environment.get("crate_version"),
        "rack_id": &report.rack_id,
        "level": report.level,
        "environment": &report.environment,
    });
    fs::write(output.join("environment.json"), serde_json::to_vec_pretty(&environment)?)?;
    write_random_seeds(output, report)?;
    write_firmware_manifest(output, config, report)?;
    append_runner_log(output, report)?;
    let mut junit = String::new();
    junit.push_str(&format!(
        "<testsuite name=\"lxmf-rs-hil\" tests=\"{}\" failures=\"{}\" skipped=\"{}\">\n",
        report.cases.len(),
        report
            .cases
            .iter()
            .filter(|case| !case.result.is_pass() && case.result != ResultClass::Blocked)
            .count(),
        report.cases.iter().filter(|case| case.result == ResultClass::Blocked).count()
    ));
    for case in &report.cases {
        junit.push_str(&format!(
            "  <testcase classname=\"{}\" name=\"{}\" time=\"{:.3}\">",
            xml_escape(&case.profile_id),
            xml_escape(&case.case_id),
            case.duration_ms as f64 / 1000.0
        ));
        if case.result == ResultClass::Blocked {
            junit.push_str(&format!("<skipped message=\"{}\"/>", xml_escape(&case.reason)));
        } else if !case.result.is_pass() {
            junit.push_str(&format!(
                "<failure message=\"{}\">{}</failure>",
                xml_escape(&case.result.to_string()),
                xml_escape(&case.reason)
            ));
        }
        junit.push_str("</testcase>\n");
    }
    junit.push_str("</testsuite>\n");
    fs::write(output.join("junit.xml"), junit).context("write HIL junit.xml")?;
    Ok(())
}

fn write_random_seeds(output: &Path, report: &RunReport) -> Result<()> {
    let payload = serde_json::json!({
        "run_id": &report.run_id,
        "run_seed": report.environment.get("seed"),
        "cases": report.cases.iter().map(|case| serde_json::json!({
            "case_id": &case.case_id,
            "profile_id": &case.profile_id,
            "seed": case.seed,
            "attempts": case.attempts,
        })).collect::<Vec<_>>(),
    });
    fs::write(output.join("random-seeds.json"), serde_json::to_vec_pretty(&payload)?)?;
    Ok(())
}

fn write_firmware_manifest(output: &Path, config: &HilConfig, report: &RunReport) -> Result<()> {
    let profiles = report
        .cases
        .iter()
        .map(|case| case.profile_id.as_str())
        .filter(|profile_id| *profile_id != "__profile_reset__")
        .collect::<BTreeSet<_>>();
    let entries = config
        .lab
        .profiles
        .iter()
        .filter(|profile| profiles.contains(profile.id.as_str()))
        .map(|profile| {
            serde_json::json!({
                "profile_id": &profile.id,
                "adapter": &profile.adapter,
                "host": &profile.host,
                "identity_env": &profile.identity_env,
                "endpoint_env": &profile.endpoint_env,
                "firmware_env": &profile.firmware_env,
                "firmware_hash_env": &profile.firmware_hash_env,
                "firmware": profile.firmware_env.as_deref().and_then(|name| std::env::var(name).ok()),
                "firmware_hash": profile.firmware_hash_env.as_deref().and_then(|name| std::env::var(name).ok()),
            })
        })
        .collect::<Vec<_>>();
    fs::write(
        output.join("firmware-manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({ "profiles": entries }))?,
    )?;
    Ok(())
}

fn append_runner_log(output: &Path, report: &RunReport) -> Result<()> {
    let mut log = OpenOptions::new().create(true).append(true).open(output.join("runner.log"))?;
    writeln!(log, "run_result={}", report.result)?;
    for case in &report.cases {
        writeln!(
            log,
            "case={} profile={} result={} duration_ms={} attempts={} reason={}",
            case.case_id,
            case.profile_id,
            case.result,
            case.duration_ms,
            case.attempts,
            case.reason.replace('\n', " ")
        )?;
    }
    Ok(())
}

pub(super) fn report(
    config: &HilConfig,
    evidence_arg: Option<PathBuf>,
    output_arg: Option<PathBuf>,
) -> Result<()> {
    let evidence = evidence_arg.unwrap_or_else(|| config.root.join("target/hil/runs"));
    let output = output_arg.unwrap_or_else(|| config.root.join("target/hil/report"));
    fs::create_dir_all(&output)?;
    let reports = find_result_files(&evidence)?;
    if reports.is_empty() {
        bail!("no HIL result.json files found under {}", evidence.display());
    }

    let mut rows = Vec::new();
    for path in reports {
        let value: serde_json::Value = serde_json::from_slice(
            &fs::read(&path).with_context(|| format!("read {}", path.display()))?,
        )
        .with_context(|| format!("parse {}", path.display()))?;
        rows.push(serde_json::json!({
            "run_id": value.get("run_id"),
            "level": value.get("level"),
            "profile_results": value.get("cases"),
            "result": value.get("result"),
            "source": path,
        }));
    }
    fs::write(output.join("support-matrix.json"), serde_json::to_vec_pretty(&rows)?)?;
    let mut observations = BTreeMap::<String, BTreeMap<String, String>>::new();
    for row in &rows {
        if let Some(cases) = row["profile_results"].as_array() {
            for case in cases {
                let Some(profile_id) = case["profile_id"].as_str() else {
                    continue;
                };
                let suite = config
                    .cases
                    .iter()
                    .find(|definition| {
                        definition.id == case["case_id"].as_str().unwrap_or_default()
                    })
                    .map(|definition| definition.suite.as_str())
                    .unwrap_or("unknown");
                let result = case["result"].as_str().unwrap_or("unknown").to_string();
                let slot = observations
                    .entry(profile_id.to_string())
                    .or_default()
                    .entry(suite.to_string())
                    .or_insert_with(|| "PASS".to_string());
                if result_rank(&result) > result_rank(slot) {
                    *slot = result;
                }
            }
        }
    }
    let mut markdown = String::from(
        "# Hardware Support Matrix\n\n| Adapter | Virtual | Host | Physical smoke | Soak | RNS 1.4.2 interop |\n|---|---|---|---|---|---|\n",
    );
    for profile in &config.lab.profiles {
        let profile_observations = observations.get(&profile.id);
        let status = |suite: &str| {
            profile_observations
                .and_then(|values| values.get(suite))
                .map(String::as_str)
                .unwrap_or("PENDING")
        };
        let virtual_status = if profile.id == "virtual" {
            profile_observations
                .and_then(|values| values.values().max_by_key(|value| result_rank(value)))
                .map(String::as_str)
                .unwrap_or("PENDING")
        } else {
            "N/A"
        };
        let interop_status =
            if profile.id == "python-reference" { status("interop") } else { "PENDING" };
        let soak_status = profile_observations
            .and_then(|values| values.get("soak").or_else(|| values.get("recovery")))
            .map(String::as_str)
            .unwrap_or("PENDING");
        markdown.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            profile.label,
            virtual_status,
            profile.host,
            status("smoke"),
            soak_status,
            interop_status
        ));
    }
    markdown.push_str("\n## Run cases\n\n| Run | Level | Profile | Result |\n|---|---|---|---|\n");
    for row in &rows {
        let run_id = row["run_id"].as_str().unwrap_or("unknown");
        let level = row["level"].as_str().unwrap_or("unknown");
        if let Some(cases) = row["profile_results"].as_array() {
            for case in cases {
                markdown.push_str(&format!(
                    "| {run_id} | {level} | {} | {} |\n",
                    case["profile_id"].as_str().unwrap_or("unknown"),
                    case["result"].as_str().unwrap_or("unknown")
                ));
            }
        } else {
            markdown.push_str(&format!(
                "| {run_id} | {level} | unknown | {} |\n",
                row["result"].as_str().unwrap_or("unknown")
            ));
        }
    }
    fs::write(output.join("hardware-support.md"), markdown)?;
    println!("HIL support report: {}", output.display());
    Ok(())
}

fn find_result_files(root: &Path) -> Result<Vec<PathBuf>> {
    if root.is_file() {
        return Ok(vec![root.to_path_buf()]);
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(root).with_context(|| format!("read {}", root.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            files.extend(find_result_files(&path)?);
        } else if path.file_name().and_then(|value| value.to_str()) == Some("result.json") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn result_rank(result: &str) -> u8 {
    match result {
        "FAIL_PROTOCOL" => 6,
        "FAIL_ASSERTION" => 5,
        "FAIL_DEVICE" => 4,
        "FAIL_LAB" => 3,
        "BLOCKED" => 2,
        "PASS" => 1,
        _ => 0,
    }
}
