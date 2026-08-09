use anyhow::{bail, Context, Result};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn tool_available(tool: &str) -> bool {
    let command = if cfg!(windows) { "where" } else { "command" };
    if cfg!(windows) {
        std::process::Command::new(command)
            .arg(tool)
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    } else {
        std::process::Command::new("sh")
            .args(["-c", &format!("command -v {tool}")])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }
}

pub(super) fn git_commit(root: &Path) -> String {
    std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

pub(super) fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

pub(super) struct LeaseGuard {
    path: PathBuf,
}

impl LeaseGuard {
    pub(super) fn acquire(path: &Path, ttl_secs: u64) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let stale = fs::metadata(path)
                    .and_then(|metadata| metadata.modified())
                    .ok()
                    .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                    .is_some_and(|age| age.as_secs() > ttl_secs);
                if !stale {
                    bail!("HIL rack lock {} is already held", path.display());
                }
                fs::remove_file(path)
                    .with_context(|| format!("remove stale HIL rack lock {}", path.display()))?;
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .with_context(|| format!("reserve HIL rack lock {}", path.display()))?
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("reserve HIL rack lock {}", path.display()))
            }
        };
        writeln!(file, "pid={} acquired_at={}", std::process::id(), unix_secs())?;
        Ok(Self { path: path.to_path_buf() })
    }
}

impl Drop for LeaseGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
