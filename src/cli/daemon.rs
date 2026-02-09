use crate::cli::profile::{profile_paths, resolve_identity_path, ProfileSettings};
use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Serialize)]
pub struct DaemonStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub rpc: String,
    pub profile: String,
    pub managed: bool,
    pub log_path: String,
}

#[derive(Debug, Clone)]
pub struct DaemonSupervisor {
    pub profile: String,
    pub settings: ProfileSettings,
}

impl DaemonSupervisor {
    pub fn new(profile: &str, settings: ProfileSettings) -> Self {
        Self {
            profile: profile.to_string(),
            settings,
        }
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
        if let Some(pid) = read_pid(&paths.daemon_pid)? {
            if is_pid_running(pid) {
                return Ok(DaemonStatus {
                    running: true,
                    pid: Some(pid),
                    rpc: self.settings.rpc.clone(),
                    profile: self.profile.clone(),
                    managed,
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
        let log_file_err = log_file
            .try_clone()
            .context("failed to clone log file descriptor")?;

        let reticulumd_bin = reticulumd_override
            .or_else(|| self.settings.reticulumd_path.clone())
            .or_else(|| std::env::var("RETICULUMD_BIN").ok())
            .unwrap_or_else(|| "reticulumd".into());

        let identity_path = resolve_identity_path(&self.settings, &paths);
        let db_path = self
            .settings
            .db_path
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| paths.daemon_db.clone());
        let transport = transport_override.or_else(|| self.settings.transport.clone());

        let mut cmd = Command::new(&reticulumd_bin);
        cmd.arg("--rpc")
            .arg(&self.settings.rpc)
            .arg("--db")
            .arg(&db_path)
            .arg("--identity")
            .arg(identity_path)
            .arg("--config")
            .arg(&paths.reticulum_toml)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(log_file_err));

        if let Some(transport) = transport {
            cmd.arg("--transport").arg(transport);
        }

        let child = cmd
            .spawn()
            .with_context(|| format!("failed to spawn {}", reticulumd_bin))?;
        let pid = child.id();

        let mut pid_file = File::create(&paths.daemon_pid)
            .with_context(|| format!("failed to create {}", paths.daemon_pid.display()))?;
        writeln!(pid_file, "{}", pid).context("failed to write daemon pid")?;

        Ok(DaemonStatus {
            running: true,
            pid: Some(pid),
            rpc: self.settings.rpc.clone(),
            profile: self.profile.clone(),
            managed,
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

        Ok(DaemonStatus {
            running,
            pid: if running { pid } else { None },
            rpc: self.settings.rpc.clone(),
            profile: self.profile.clone(),
            managed: self.settings.managed,
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
    let pid = trimmed
        .parse::<u32>()
        .with_context(|| format!("invalid pid in {}", path.display()))?;
    Ok(Some(pid))
}

fn is_pid_running(pid: u32) -> bool {
    match Command::new("kill").arg("-0").arg(pid.to_string()).status() {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}
