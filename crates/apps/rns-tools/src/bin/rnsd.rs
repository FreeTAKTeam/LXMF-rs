use std::env;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let reticulumd = resolve_reticulumd_binary();

    #[cfg(unix)]
    {
        let err = Command::new(&reticulumd).args(&args).exec();
        eprintln!("rnsd: failed to exec {}: {}", reticulumd.display(), err);
        ExitCode::from(1)
    }

    #[cfg(not(unix))]
    {
        match Command::new(&reticulumd).args(&args).status() {
            Ok(status) => ExitCode::from(status.code().unwrap_or(1) as u8),
            Err(err) => {
                eprintln!("rnsd: failed to launch {}: {}", reticulumd.display(), err);
                ExitCode::from(1)
            }
        }
    }
}

fn resolve_reticulumd_binary() -> PathBuf {
    if let Some(path) = env::var_os("RETICULUMD_BIN").filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }

    let current_exe = env::current_exe().ok();
    let mut candidates = Vec::new();
    if let Some(exe) = current_exe.as_ref() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(reticulumd_binary_name()));
            if let Some(parent) = dir.parent() {
                candidates.push(parent.join(reticulumd_binary_name()));
            }
        }
    }

    candidates
        .into_iter()
        .find(|candidate| candidate.exists())
        .unwrap_or_else(|| PathBuf::from(reticulumd_binary_name()))
}

fn reticulumd_binary_name() -> &'static str {
    #[cfg(windows)]
    {
        "reticulumd.exe"
    }

    #[cfg(not(windows))]
    {
        "reticulumd"
    }
}
