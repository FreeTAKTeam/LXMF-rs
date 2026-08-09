use super::model::{CaseResult, Profile, ResultClass};
use std::fs;
use std::path::Path;
use std::process::Stdio;
use std::time::Instant;
use tokio::process::Command as TokioCommand;
use tokio::time::{timeout, Duration};

pub(super) async fn reset_profile(
    profile: &Profile,
    seed: u64,
    output: &Path,
    timeout_secs: u64,
) -> CaseResult {
    let started = Instant::now();
    let profile_id = profile.id.clone();
    let reset_command_label =
        profile.reset_command_env.as_deref().map(|name| format!("<reset:{name}>"));
    let command = profile
        .reset_command_env
        .as_deref()
        .and_then(std::env::var_os)
        .map(|value| value.to_string_lossy().to_string());
    let result = if let Some(ref command) = command {
        shell_command(command, timeout_secs).await
    } else if let (Some(hub_env), Some(port_env)) =
        (profile.power_hub_env.as_deref(), profile.power_port_env.as_deref())
    {
        match (std::env::var(hub_env), std::env::var(port_env)) {
            (Ok(hub), Ok(port)) => {
                let mut command = TokioCommand::new("uhubctl");
                command.args(["-l", &hub, "-p", &port, "cycle"]);
                run_command(command, timeout_secs).await
            }
            _ => Err("power-controller environment is incomplete".to_string()),
        }
    } else {
        Err("no reset command or uhubctl mapping configured".to_string())
    };
    let log_path = output.join("device-logs").join(format!("{profile_id}-reset.log"));
    let reason = match result {
        Ok(()) => "profile reset completed".to_string(),
        Err(error) => match fs::write(&log_path, error.as_bytes()) {
            Ok(()) => format!("reset failed: {error}"),
            Err(log_error) => {
                format!("reset failed: {error}; reset log write failed: {log_error}")
            }
        },
    };
    CaseResult {
        case_id: "__profile_reset__".to_string(),
        profile_id,
        result: if reason == "profile reset completed" {
            ResultClass::Pass
        } else {
            ResultClass::FailLab
        },
        reason,
        duration_ms: started.elapsed().as_millis(),
        attempts: 1,
        seed,
        command: reset_command_label.or_else(|| {
            profile
                .power_hub_env
                .as_ref()
                .zip(profile.power_port_env.as_ref())
                .map(|_| "<uhubctl mapping>".to_string())
        }),
        artifacts: if log_path.exists() {
            vec![log_path.display().to_string()]
        } else {
            Vec::new()
        },
    }
}

pub(super) async fn reset_with_retry(
    profile: &Profile,
    seed: u64,
    output: &Path,
    timeout_secs: u64,
) -> CaseResult {
    let first = reset_profile(profile, seed, output, timeout_secs).await;
    if first.result != ResultClass::FailLab {
        return first;
    }
    let mut retry = reset_profile(profile, seed, output, timeout_secs).await;
    retry.attempts = 2;
    if retry.result.is_pass() {
        retry.reason = "profile reset completed after one lab retry".to_string();
    }
    retry
}

async fn shell_command(command: &str, timeout_secs: u64) -> std::result::Result<(), String> {
    let process = if cfg!(windows) {
        let mut process = TokioCommand::new("cmd");
        process.args(["/C", command]);
        process
    } else {
        let mut process = TokioCommand::new("sh");
        process.args(["-c", command]);
        process
    };
    run_command(process, timeout_secs).await
}

async fn run_command(
    mut command: TokioCommand,
    timeout_secs: u64,
) -> std::result::Result<(), String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let child = command.spawn().map_err(|error| error.to_string())?;
    match timeout(Duration::from_secs(timeout_secs), child.wait_with_output()).await {
        Ok(Ok(output)) if output.status.success() => Ok(()),
        Ok(Ok(output)) => Err(format!("reset command exited with {}", output.status)),
        Ok(Err(error)) => Err(format!("wait for reset command failed: {error}")),
        Err(_) => Err(format!("reset command timed out after {timeout_secs}s")),
    }
}

#[cfg(test)]
mod tests {
    use super::run_command;
    use tokio::process::Command as TokioCommand;

    #[tokio::test]
    async fn reset_command_timeout_is_bounded() {
        let command = if cfg!(windows) {
            let mut command = TokioCommand::new("ping");
            command.args(["-n", "4", "127.0.0.1"]);
            command
        } else {
            let mut command = TokioCommand::new("sleep");
            command.arg("4");
            command
        };
        let error = run_command(command, 1).await.expect_err("command should time out");
        assert!(error.contains("timed out"), "unexpected reset error: {error}");
    }
}
