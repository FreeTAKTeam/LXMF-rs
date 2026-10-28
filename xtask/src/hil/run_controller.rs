use super::config::{case_applies, missing_profile_environment, reset_hook_configured, HilConfig};
use super::model::{ExecutionLevel, Profile, RunReport};
use super::runner::{blocked_case, run_case};
use super::support::{git_commit, tool_available, unix_secs, LeaseGuard};
use super::{evidence, reset};
use anyhow::{bail, Context, Result};
use clap::Args;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Args, Debug)]
pub struct RunArgs {
    #[arg(long, conflicts_with_all = ["suite", "all"])]
    pub profile: Option<String>,
    #[arg(long, conflicts_with_all = ["profile", "all"])]
    pub suite: Option<String>,
    #[arg(long, conflicts_with_all = ["profile", "suite"])]
    pub all: bool,
    /// Run only these case IDs; repeat the option or comma-separate IDs.
    #[arg(long = "case", value_delimiter = ',')]
    pub cases: Vec<String>,
    #[arg(long, value_enum)]
    pub level: ExecutionLevel,
    #[arg(long)]
    pub seed: Option<u64>,
    #[arg(long)]
    pub output: Option<PathBuf>,
}

pub fn run(config: &HilConfig, args: RunArgs) -> Result<()> {
    let profiles = config.profiles_for(args.profile.as_deref(), args.level)?;
    validate_case_ids(config, &profiles, &args)?;
    let started = Instant::now();
    let started_at_unix_secs = unix_secs();
    let run_id = format!("{}-{}", started_at_unix_secs, std::process::id());
    let output = args.output.unwrap_or_else(|| config.root.join("target/hil/runs").join(&run_id));
    fs::create_dir_all(&output)
        .with_context(|| format!("create HIL output directory {}", output.display()))?;
    fs::create_dir_all(output.join("device-logs"))?;
    fs::create_dir_all(output.join("serial-captures"))?;
    fs::write(
        output.join("runner.log"),
        format!("run_started_at_unix_secs={started_at_unix_secs}\nrun_id={run_id}\n"),
    )?;
    let lock_path = std::env::var_os(&config.lab.lock_env)
        .map(PathBuf::from)
        .unwrap_or_else(|| config.root.join("target/hil/hil.lock"));
    let _lease = LeaseGuard::acquire(&lock_path, config.lab.lock_ttl_secs)?;
    let seed = args.seed.unwrap_or_else(|| started_at_unix_secs ^ u64::from(std::process::id()));
    let mut report = RunReport::new(
        run_id,
        git_commit(&config.root),
        config.lab.rack_id.clone(),
        args.level,
        started_at_unix_secs,
    );
    report.environment.insert("host_os".to_string(), std::env::consts::OS.to_string());
    report.environment.insert("host_arch".to_string(), std::env::consts::ARCH.to_string());
    report.environment.insert(
        "crate_version".to_string(),
        fs::read_to_string(config.root.join("VERSION"))
            .map(|value| value.trim().to_string())
            .unwrap_or_else(|_| "unknown".to_string()),
    );
    report.environment.insert("seed".to_string(), seed.to_string());
    report.environment.insert("python_rns_version".to_string(), "1.5.2".to_string());
    report.environment.insert(
        "python_rns_revision".to_string(),
        "ea98db4f53dcf0defc0e71a16e60d28b1229c4e6".to_string(),
    );
    report.environment.insert(
        "python_lxmf_revision".to_string(),
        "727830cefda83d9c6e3982b48675425f3f988f9c".to_string(),
    );
    report.environment.insert(
        "reticulum_conformance_revision".to_string(),
        "0319444b20e0815f26c6b9ceeba8fa44de037c9b".to_string(),
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("create HIL command runtime")?;
    for profile in profiles {
        let cases = config
            .cases
            .iter()
            .filter(|case| {
                case_applies(case, profile, args.level, args.suite.as_deref())
                    && (args.cases.is_empty() || args.cases.contains(&case.id))
            })
            .collect::<Vec<_>>();
        if cases.is_empty() {
            continue;
        }
        let mut missing = if profile.physical {
            missing_profile_environment(profile, &config.lab)
        } else {
            Vec::new()
        };
        if profile.physical
            && profile.power_hub_env.is_some()
            && !reset_hook_configured(profile)
            && !tool_available("uhubctl")
        {
            missing.push("tool:uhubctl".to_string());
        }
        if !missing.is_empty() {
            for case in cases {
                report.cases.push(blocked_case(
                    case,
                    profile,
                    seed,
                    format!("profile unavailable: {}", missing.join(", ")),
                ));
            }
            continue;
        }
        if profile.physical {
            let reset_result = runtime.block_on(reset::reset_with_retry(
                profile,
                seed,
                &output,
                config.lab.reset_timeout_secs,
            ));
            let reset_failed = !reset_result.result.is_pass();
            report.cases.push(reset_result);
            if reset_failed {
                for case in cases {
                    report.cases.push(blocked_case(
                        case,
                        profile,
                        seed,
                        "profile reset failed; case was not attempted".to_string(),
                    ));
                }
                continue;
            }
        }
        let total = cases.len();
        for (index, case) in cases.into_iter().enumerate() {
            println!(
                "HIL {} [{}/{}] {}/{}: START",
                args.level,
                index + 1,
                total,
                profile.id,
                case.id
            );
            let result =
                runtime.block_on(run_case(config, profile, case, args.level, seed, &output));
            println!(
                "HIL {} [{}/{}] {}/{}: {} ({:.1}s, {} attempt(s)) {}",
                args.level,
                index + 1,
                total,
                profile.id,
                case.id,
                result.result,
                result.duration_ms as f64 / 1000.0,
                result.attempts,
                result.reason
            );
            report.cases.push(result);
        }
    }
    if report.cases.is_empty() {
        bail!("no HIL cases matched the requested level/profile/suite");
    }
    report.finalize(started.elapsed().as_millis());
    evidence::write_report(&output, &report, config)?;
    println!(
        "HIL {}: {} ({} cases) evidence={}",
        report.level,
        report.result,
        report.cases.len(),
        output.display()
    );
    if !report.result.is_pass() {
        bail!("HIL run finished with {}", report.result);
    }
    Ok(())
}
fn validate_case_ids(config: &HilConfig, profiles: &[&Profile], args: &RunArgs) -> Result<()> {
    for id in &args.cases {
        if !profiles.iter().any(|profile| {
            config.cases.iter().any(|case| {
                &case.id == id && case_applies(case, profile, args.level, args.suite.as_deref())
            })
        }) {
            bail!("unknown or inapplicable HIL case '{id}'");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::path::Path;

    #[derive(Parser)]
    struct Cli {
        #[command(flatten)]
        args: RunArgs,
    }

    fn config() -> HilConfig {
        HilConfig::load(Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("workspace"), None)
            .expect("HIL config")
    }

    fn args(extra: &[&str]) -> RunArgs {
        let mut cli = vec!["hil", "--level", "pr"];
        cli.extend(extra);
        Cli::parse_from(cli).args
    }

    #[test]
    fn case_ids_accept_repeated_and_comma_separated_values() {
        let args = args(&["--all", "--case", "one,two", "--case", "three"]);
        assert_eq!(args.cases, ["one", "two", "three"]);
    }

    #[test]
    fn unknown_or_inapplicable_case_is_rejected_even_with_a_valid_case() {
        let config = config();
        let profiles = config.profiles_for(Some("virtual"), ExecutionLevel::Pr).expect("virtual");
        for id in ["unknown", "python-channel-interop"] {
            let args =
                args(&["--profile", "virtual", "--case", "resource-retry-budget", "--case", id]);
            assert!(validate_case_ids(&config, &profiles, &args).is_err());
        }
    }

    #[test]
    fn selected_cases_run_and_default_selection_still_runs_all_cases() {
        let mut config = config();
        config.cases.retain(|case| {
            ["resource-retry-budget", "resource-split-assembly"].contains(&case.id.as_str())
        });
        for case in &mut config.cases {
            let command = case.command.as_mut().expect("case command");
            command.program = "rustc".to_string();
            command.args = vec!["--version".to_string()];
        }
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        config.root = std::env::temp_dir()
            .join(format!("hil-case-selection-{}-{unique}", std::process::id()));
        for (extra, expected) in
            [(vec!["--all", "--case", "resource-retry-budget"], 1), (vec!["--all"], 2)]
        {
            let mut args = args(&extra);
            let output = config.root.join("target/hil").join(expected.to_string());
            args.output = Some(output.clone());
            run(&config, args).expect("fake cases pass");
            let report: serde_json::Value =
                serde_json::from_slice(&fs::read(output.join("result.json")).expect("evidence"))
                    .expect("JSON");
            assert_eq!(report["cases"].as_array().expect("cases").len(), expected);
        }
        fs::remove_dir_all(&config.root).expect("remove test fixture");
    }
}
