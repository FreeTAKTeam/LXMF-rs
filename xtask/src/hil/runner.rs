use super::config::{
    has_nonempty_env, missing_profile_environment, reset_hook_configured, HilConfig,
};
use super::evidence;
use super::model::{CaseDefinition, CaseResult, CommandSpec, ExecutionLevel, Profile, ResultClass};
pub use super::run_controller::{run, RunArgs};
use super::support::tool_available;
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

pub(super) async fn run_case(
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

pub(super) fn blocked_case(
    case: &CaseDefinition,
    profile: &Profile,
    seed: u64,
    reason: String,
) -> CaseResult {
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
