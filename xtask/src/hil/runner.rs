use super::config::{
    case_applies, has_nonempty_env, missing_profile_environment, reset_hook_configured, HilConfig,
};
use super::model::{
    CaseDefinition, CaseResult, CommandSpec, ExecutionLevel, Profile, ResultClass, RunReport,
};
use super::support::{git_commit, tool_available, unix_secs, LeaseGuard};
use super::{evidence, reset};
use anyhow::{bail, Context, Result};
use clap::Args;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::Command as TokioCommand;
use tokio::time::timeout;

#[derive(Args, Debug)]
pub struct DoctorArgs {
    #[arg(long)]
    pub profile: Option<String>,
    #[arg(long)]
    pub host: Option<String>,
    #[arg(long)]
    pub all: bool,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct ListArgs {
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct RunArgs {
    #[arg(long, conflicts_with_all = ["suite", "all"])]
    pub profile: Option<String>,
    #[arg(long, conflicts_with_all = ["profile", "all"])]
    pub suite: Option<String>,
    #[arg(long, conflicts_with_all = ["profile", "suite"])]
    pub all: bool,
    #[arg(long, value_enum)]
    pub level: ExecutionLevel,
    #[arg(long)]
    pub seed: Option<u64>,
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct ReportArgs {
    #[arg(long)]
    pub evidence: Option<PathBuf>,
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Serialize)]
struct DoctorEntry {
    profile_id: String,
    physical: bool,
    ready: bool,
    missing: Vec<String>,
}

pub fn list(config: &HilConfig, args: ListArgs) -> Result<()> {
    if args.json {
        let payload = serde_json::json!({
            "rack_id": config.lab.rack_id,
            "profiles": config.lab.profiles,
            "cases": config.cases,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    println!("rack: {}", config.lab.rack_id);
    println!("profiles:");
    for profile in &config.lab.profiles {
        println!(
            "  {:<24} {:<12} host={} physical={} suites={}",
            profile.id,
            profile.adapter,
            profile.host,
            profile.physical,
            profile.suites.join(",")
        );
    }
    println!("cases:");
    for case in &config.cases {
        println!(
            "  {:<36} suite={} levels={} - {}",
            case.id,
            case.suite,
            case.levels.iter().map(ToString::to_string).collect::<Vec<_>>().join(","),
            case.description
        );
    }
    Ok(())
}

pub fn doctor(config: &HilConfig, args: DoctorArgs) -> Result<()> {
    let requested_profiles = if args.all || args.profile.is_none() {
        config.lab.profiles.iter().collect::<Vec<_>>()
    } else {
        vec![config.profile(args.profile.as_deref().unwrap_or_default()).with_context(|| {
            format!("unknown HIL profile '{}'", args.profile.as_deref().unwrap_or_default())
        })?]
    };
    let profiles = requested_profiles
        .into_iter()
        .filter(|profile| args.host.as_deref().is_none_or(|host| profile.host == host))
        .collect::<Vec<_>>();
    if profiles.is_empty() {
        bail!("no HIL profiles matched the doctor selection");
    }

    let entries = profiles
        .into_iter()
        .map(|profile| {
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
            DoctorEntry {
                profile_id: profile.id.clone(),
                physical: profile.physical,
                ready: missing.is_empty(),
                missing,
            }
        })
        .collect::<Vec<_>>();

    if args.json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else {
        for entry in &entries {
            if entry.ready {
                println!("{}: READY", entry.profile_id);
            } else {
                println!("{}: BLOCKED ({})", entry.profile_id, entry.missing.join(", "));
            }
        }
    }

    if entries.iter().any(|entry| !entry.ready) {
        bail!("HIL doctor found unavailable profiles; see the report above");
    }
    Ok(())
}

pub fn run(config: &HilConfig, args: RunArgs) -> Result<()> {
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
    let profiles = config.profiles_for(args.profile.as_deref(), args.level)?;
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
    report.environment.insert("python_rns_version".to_string(), "1.5.0".to_string());
    report.environment.insert(
        "python_rns_revision".to_string(),
        "e32d4df754a7b87b1bf1bb0d08675d12ff505ae6".to_string(),
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
            .filter(|case| case_applies(case, profile, args.level, args.suite.as_deref()))
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
        for case in cases {
            report
                .cases
                .push(runtime.block_on(run_case(config, profile, case, args.level, seed, &output)));
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
async fn run_case(
    config: &HilConfig,
    profile: &Profile,
    case: &CaseDefinition,
    level: ExecutionLevel,
    seed: u64,
    output: &Path,
) -> CaseResult {
    let mut attempts = 0;
    let started = Instant::now();
    loop {
        attempts += 1;
        let result = run_case_attempt(config, profile, case, level, seed, attempts, output).await;
        if result.result != ResultClass::FailLab || attempts >= 2 {
            return CaseResult { attempts, duration_ms: started.elapsed().as_millis(), ..result };
        }
    }
}

async fn run_case_attempt(
    config: &HilConfig,
    profile: &Profile,
    case: &CaseDefinition,
    level: ExecutionLevel,
    seed: u64,
    attempt: u8,
    output: &Path,
) -> CaseResult {
    let command_spec = case.command.clone().or_else(|| {
        profile.executor_env.as_deref().and_then(std::env::var_os).map(|command| {
            let command = command.to_string_lossy().to_string();
            if cfg!(windows) {
                CommandSpec {
                    program: "cmd".to_string(),
                    args: vec!["/C".to_string(), command],
                    env: Default::default(),
                    cwd: None,
                }
            } else {
                CommandSpec {
                    program: "sh".to_string(),
                    args: vec!["-c".to_string(), command],
                    env: Default::default(),
                    cwd: None,
                }
            }
        })
    });
    let Some(command_spec) = command_spec else {
        return blocked_case(
            case,
            profile,
            seed,
            "no adapter executor is configured for this profile".to_string(),
        );
    };
    let missing_env = case
        .requires_env
        .iter()
        .filter(|name| !has_nonempty_env(name))
        .cloned()
        .collect::<Vec<_>>();
    if !missing_env.is_empty() {
        return blocked_case(
            case,
            profile,
            seed,
            format!("missing required environment: {}", missing_env.join(", ")),
        );
    }

    let safe_case_id = case.id.replace(|character: char| !character.is_ascii_alphanumeric(), "_");
    let stdout_path =
        output.join("device-logs").join(format!("{safe_case_id}-{}-stdout.log", attempt));
    let stderr_path =
        output.join("serial-captures").join(format!("{safe_case_id}-{}-stderr.log", attempt));
    let mut command = TokioCommand::new(&command_spec.program);
    command.args(&command_spec.args);
    command.current_dir(
        command_spec
            .cwd
            .as_ref()
            .map(|path| config.root.join(path))
            .unwrap_or_else(|| config.root.clone()),
    );
    command.envs(&command_spec.env);
    command.env("HIL_PROFILE_ID", &profile.id);
    command.env("HIL_CASE_ID", &case.id);
    command.env("HIL_SUITE", &case.suite);
    command.env("HIL_EXECUTION_LEVEL", level.to_string());
    command.env("HIL_RANDOM_SEED", seed.to_string());
    command.env("HIL_ATTEMPT", attempt.to_string());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command.kill_on_drop(true);

    let command_line = case
        .command
        .as_ref()
        .map(|command| format!("{} {}", command.program, command.args.join(" ")))
        .or_else(|| profile.executor_env.as_deref().map(|name| format!("<executor:{name}>")))
        .unwrap_or_else(|| format!("{} {}", command_spec.program, command_spec.args.join(" ")));
    let timeout_secs = case.timeout_secs.unwrap_or(match level {
        ExecutionLevel::Pr => 300,
        ExecutionLevel::Nightly => 900,
        ExecutionLevel::Release => 3_600,
    });
    let child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return CaseResult {
                case_id: case.id.clone(),
                profile_id: profile.id.clone(),
                result: ResultClass::FailLab,
                reason: format!("spawn '{command_line}': {error}"),
                duration_ms: 0,
                attempts: 1,
                seed,
                command: Some(command_line),
                artifacts: Vec::new(),
            };
        }
    };
    let result = timeout(Duration::from_secs(timeout_secs), child.wait_with_output()).await;
    match result {
        Ok(Ok(output_result)) => {
            if let Err(error) = fs::write(&stdout_path, &output_result.stdout)
                .and_then(|_| fs::write(&stderr_path, &output_result.stderr))
            {
                return CaseResult {
                    case_id: case.id.clone(),
                    profile_id: profile.id.clone(),
                    result: ResultClass::FailLab,
                    reason: format!("write command evidence for '{command_line}': {error}"),
                    duration_ms: 0,
                    attempts: 1,
                    seed,
                    command: Some(command_line),
                    artifacts: Vec::new(),
                };
            }
            let class = if output_result.status.success() {
                ResultClass::Pass
            } else {
                case.failure_class.unwrap_or(ResultClass::FailAssertion)
            };
            CaseResult {
                case_id: case.id.clone(),
                profile_id: profile.id.clone(),
                result: class,
                reason: if output_result.status.success() {
                    "command completed successfully".to_string()
                } else {
                    format!("command exited with {}", output_result.status)
                },
                duration_ms: 0,
                attempts: 1,
                seed,
                command: Some(command_line),
                artifacts: vec![
                    stdout_path.display().to_string(),
                    stderr_path.display().to_string(),
                ],
            }
        }
        Ok(Err(error)) => CaseResult {
            case_id: case.id.clone(),
            profile_id: profile.id.clone(),
            result: ResultClass::FailLab,
            reason: format!("wait for '{command_line}': {error}"),
            duration_ms: 0,
            attempts: 1,
            seed,
            command: Some(command_line),
            artifacts: Vec::new(),
        },
        Err(_) => CaseResult {
            case_id: case.id.clone(),
            profile_id: profile.id.clone(),
            result: ResultClass::FailLab,
            reason: format!("hard timeout after {timeout_secs}s: {command_line}"),
            duration_ms: 0,
            attempts: 1,
            seed,
            command: Some(command_line),
            artifacts: Vec::new(),
        },
    }
}

fn blocked_case(case: &CaseDefinition, profile: &Profile, seed: u64, reason: String) -> CaseResult {
    CaseResult {
        case_id: case.id.clone(),
        profile_id: profile.id.clone(),
        result: ResultClass::Blocked,
        reason,
        duration_ms: 0,
        attempts: 1,
        seed,
        command: case
            .command
            .as_ref()
            .map(|command| format!("{} {}", command.program, command.args.join(" "))),
        artifacts: Vec::new(),
    }
}

pub fn report(config: &HilConfig, args: ReportArgs) -> Result<()> {
    evidence::report(config, args.evidence, args.output)
}
