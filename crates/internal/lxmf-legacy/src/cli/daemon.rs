use crate::cli::profile::{
    load_reticulum_config, normalize_optional_display_name, profile_paths, resolve_identity_path,
    ProfileSettings,
};
use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const INFERRED_TRANSPORT_BIND: &str = "127.0.0.1:0";
const DEFAULT_MANAGED_ANNOUNCE_INTERVAL_SECS: u64 = 900;
const STARTUP_PROCESS_GRACE: Duration = Duration::from_secs(3);
const STARTUP_POLL_INTERVAL: Duration = Duration::from_millis(80);
const STARTUP_PROCESS_GRACE_ENV_MS: &str = "LXMF_DAEMON_STARTUP_GRACE_MS";
const STARTUP_POLL_INTERVAL_ENV_MS: &str = "LXMF_DAEMON_STARTUP_POLL_MS";

#[derive(Debug, Clone, Serialize)]
pub struct DaemonStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub rpc: String,
    pub profile: String,
    pub managed: bool,
    pub transport: Option<String>,
    pub transport_inferred: bool,
    pub log_path: String,
}

#[derive(Debug, Clone)]
pub struct DaemonSupervisor {
    pub profile: String,
    pub settings: ProfileSettings,
}

impl DaemonSupervisor {
    pub fn new(profile: &str, settings: ProfileSettings) -> Self {
        Self { profile: profile.to_string(), settings }
    }

    pub fn start(
        &self,
        reticulumd_override: Option<String>,
        managed_override: Option<bool>,
        transport_override: Option<String>,
    ) -> Result<DaemonStatus> {
        let managed = managed_override.unwrap_or(self.settings.managed);
        if !managed {
            return Err(anyhow!(
                "profile '{}' is external mode; use --managed or update profile settings",
                self.profile
            ));
        }

        let paths = profile_paths(&self.profile)?;
        let (transport, transport_inferred) =
            resolve_transport_for_start(&self.profile, &self.settings, transport_override);
        if let Some(pid) = read_pid(&paths.daemon_pid)? {
            if is_pid_running(pid) {
                return Ok(DaemonStatus {
                    running: true,
                    pid: Some(pid),
                    rpc: self.settings.rpc.clone(),
                    profile: self.profile.clone(),
                    managed,
                    transport,
                    transport_inferred,
                    log_path: paths.daemon_log.display().to_string(),
                });
            }
            let _ = fs::remove_file(&paths.daemon_pid);
        }

        fs::create_dir_all(&paths.root)
            .with_context(|| format!("failed to create {}", paths.root.display()))?;

        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&paths.daemon_log)
            .with_context(|| format!("failed to open {}", paths.daemon_log.display()))?;
        let log_file_err = log_file.try_clone().context("failed to clone log file descriptor")?;

        let reticulumd_bin = resolve_reticulumd_binary(&reticulumd_override, &self.settings);

        let identity_path = resolve_identity_path(&self.settings, &paths);
        drop_empty_identity_stub(&identity_path)?;
        let db_path = self
            .settings
            .db_path
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| paths.daemon_db.clone());

        let mut cmd = Command::new(&reticulumd_bin);
        cmd.arg("--rpc")
            .arg(&self.settings.rpc)
            .arg("--db")
            .arg(&db_path)
            .arg("--announce-interval-secs")
            .arg(DEFAULT_MANAGED_ANNOUNCE_INTERVAL_SECS.to_string())
            .arg("--identity")
            .arg(identity_path)
            .arg("--config")
            .arg(&paths.reticulum_toml)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(log_file_err));

        if let Some(transport) = transport.as_deref() {
            cmd.arg("--transport").arg(transport);
        }

        let normalized_display_name =
            normalize_optional_display_name(self.settings.display_name.as_deref())?;
        if let Some(display_name) = normalized_display_name {
            cmd.env("LXMF_DISPLAY_NAME", display_name);
        } else {
            cmd.env_remove("LXMF_DISPLAY_NAME");
        }
        #[cfg(unix)]
        {
            // Start daemon in a dedicated process group so it is not tied to the caller's group.
            cmd.process_group(0);
        }

        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(anyhow!(
                    "failed to spawn '{}' (not found). set profile reticulumd_path, pass --reticulumd, set RETICULUMD_BIN, or build Reticulum-rs",
                    reticulumd_bin
                ))
            }
            Err(err) => {
                return Err(err).with_context(|| format!("failed to spawn {}", reticulumd_bin))
            }
        };
        let pid = child.id();

        let startup_deadline = Instant::now() + startup_process_grace();
        let startup_poll_interval = startup_poll_interval();
        loop {
            if let Some(status) =
                child.try_wait().context("failed to check reticulumd process status")?
            {
                let hint = startup_failure_hint(&paths.daemon_log, &self.settings.rpc);
                let hint_suffix =
                    hint.as_ref().map(|value| format!(". {value}")).unwrap_or_default();
                return Err(anyhow!(
                    "reticulumd exited during startup with status {}. check daemon log at {}{}",
                    status,
                    paths.daemon_log.display(),
                    hint_suffix
                ));
            }
            if Instant::now() >= startup_deadline {
                break;
            }
            std::thread::sleep(startup_poll_interval);
        }

        let mut pid_file = File::create(&paths.daemon_pid)
            .with_context(|| format!("failed to create {}", paths.daemon_pid.display()))?;
        writeln!(pid_file, "{}", pid).context("failed to write daemon pid")?;

        Ok(DaemonStatus {
            running: true,
            pid: Some(pid),
            rpc: self.settings.rpc.clone(),
            profile: self.profile.clone(),
            managed,
            transport,
            transport_inferred,
            log_path: paths.daemon_log.display().to_string(),
        })
    }

    pub fn stop(&self) -> Result<DaemonStatus> {
        let paths = profile_paths(&self.profile)?;
        let pid = read_pid(&paths.daemon_pid)?;

        if let Some(pid) = pid {
            if is_pid_running(pid) {
                let status = Command::new("kill")
                    .arg(pid.to_string())
                    .status()
                    .with_context(|| format!("failed to kill pid {}", pid))?;
                if !status.success() {
                    return Err(anyhow!("kill returned non-success status for pid {}", pid));
                }
            }
            let _ = fs::remove_file(&paths.daemon_pid);
        }

        Ok(DaemonStatus {
            running: false,
            pid: None,
            rpc: self.settings.rpc.clone(),
            profile: self.profile.clone(),
            managed: self.settings.managed,
            transport: self.settings.transport.clone(),
            transport_inferred: false,
            log_path: paths.daemon_log.display().to_string(),
        })
    }

    pub fn restart(
        &self,
        reticulumd_override: Option<String>,
        managed_override: Option<bool>,
        transport_override: Option<String>,
    ) -> Result<DaemonStatus> {
        let _ = self.stop();
        self.start(reticulumd_override, managed_override, transport_override)
    }

    pub fn status(&self) -> Result<DaemonStatus> {
        let paths = profile_paths(&self.profile)?;
        let pid = read_pid(&paths.daemon_pid)?;
        let running = pid.map(is_pid_running).unwrap_or(false);
        if !running && pid.is_some() {
            let _ = fs::remove_file(&paths.daemon_pid);
        }

        Ok(DaemonStatus {
            running,
            pid: if running { pid } else { None },
            rpc: self.settings.rpc.clone(),
            profile: self.profile.clone(),
            managed: self.settings.managed,
            transport: self.settings.transport.clone(),
            transport_inferred: false,
            log_path: paths.daemon_log.display().to_string(),
        })
    }

    pub fn logs(&self, tail: usize) -> Result<Vec<String>> {
        let paths = profile_paths(&self.profile)?;
        if !paths.daemon_log.exists() {
            return Ok(Vec::new());
        }
        let file = File::open(&paths.daemon_log)
            .with_context(|| format!("failed to open {}", paths.daemon_log.display()))?;
        let mut lines: Vec<String> = BufReader::new(file)
            .lines()
            .collect::<std::io::Result<Vec<_>>>()
            .context("failed to read daemon logs")?;
        if lines.len() > tail {
            lines = lines.split_off(lines.len() - tail);
        }
        Ok(lines)
    }
}

fn read_pid(path: &PathBuf) -> Result<Option<u32>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read pid file {}", path.display()))?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let pid =
        trimmed.parse::<u32>().with_context(|| format!("invalid pid in {}", path.display()))?;
    Ok(Some(pid))
}

fn is_pid_running(pid: u32) -> bool {
    match Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

fn drop_empty_identity_stub(path: &PathBuf) -> Result<()> {
    if let Ok(meta) = fs::metadata(path) {
        if meta.is_file() && meta.len() == 0 {
            fs::remove_file(path).with_context(|| {
                format!("failed to remove empty identity stub {}", path.display())
            })?;
        }
    }
    Ok(())
}

fn resolve_transport_for_start(
    profile: &str,
    settings: &ProfileSettings,
    transport_override: Option<String>,
) -> (Option<String>, bool) {
    if let Some(value) = clean_non_empty(transport_override) {
        return (Some(value), false);
    }

    if let Some(value) = clean_non_empty(settings.transport.clone()) {
        return (Some(value), false);
    }

    if should_infer_transport(profile) {
        return (Some(INFERRED_TRANSPORT_BIND.to_string()), true);
    }

    (None, false)
}

fn should_infer_transport(profile: &str) -> bool {
    load_reticulum_config(profile)
        .map(|config| config.interfaces.iter().any(|iface| iface.enabled))
        .unwrap_or(false)
}

fn clean_non_empty(value: Option<String>) -> Option<String> {
    value.map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
}

fn startup_process_grace() -> Duration {
    duration_ms_from_env(STARTUP_PROCESS_GRACE_ENV_MS).unwrap_or(STARTUP_PROCESS_GRACE)
}

fn startup_poll_interval() -> Duration {
    duration_ms_from_env(STARTUP_POLL_INTERVAL_ENV_MS)
        .map(|duration| duration.max(Duration::from_millis(1)))
        .unwrap_or(STARTUP_POLL_INTERVAL)
}

fn duration_ms_from_env(key: &str) -> Option<Duration> {
    let raw = std::env::var(key).ok()?;
    let millis = raw.parse::<u64>().ok()?;
    Some(Duration::from_millis(millis))
}

fn startup_failure_hint(log_path: &PathBuf, rpc: &str) -> Option<String> {
    let bytes = fs::read(log_path).ok()?;
    let start = bytes.len().saturating_sub(8192);
    let tail = String::from_utf8_lossy(&bytes[start..]).to_ascii_lowercase();

    if tail.contains("address already in use") {
        return Some(format!(
            "rpc endpoint {rpc} is already in use by another process; choose a different --rpc or stop the conflicting service"
        ));
    }
    if tail.contains("invalid identity") {
        return Some(
            "profile identity appears invalid; import a valid identity or remove it so reticulumd can regenerate".to_string(),
        );
    }
    None
}

fn resolve_reticulumd_binary(
    reticulumd_override: &Option<String>,
    settings: &ProfileSettings,
) -> String {
    if let Some(path) = reticulumd_override.clone() {
        return path;
    }
    if let Some(path) = settings.reticulumd_path.clone() {
        return path;
    }
    if let Ok(path) = std::env::var("RETICULUMD_BIN") {
        if !path.trim().is_empty() {
            return path;
        }
    }

    // Dev-friendly fallback for monorepo and legacy sibling checkout layouts.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().and_then(|dir| dir.parent());
    let sibling_root = workspace_root.and_then(std::path::Path::parent);
    let mut search_roots = Vec::new();
    if let Some(root) = workspace_root {
        search_roots.push(root.to_path_buf());
    }
    if let Some(root) = sibling_root {
        search_roots.push(root.to_path_buf());
    }

    for root in search_roots {
        let candidates = [
            root.join("target/debug/reticulumd"),
            root.join("target/release/reticulumd"),
            root.join("Reticulum-rs/target/debug/reticulumd"),
            root.join("Reticulum-rs/target/release/reticulumd"),
        ];
        for candidate in candidates {
            if candidate.exists() {
                return candidate.display().to_string();
            }
        }
    }

    "reticulumd".to_string()
}
