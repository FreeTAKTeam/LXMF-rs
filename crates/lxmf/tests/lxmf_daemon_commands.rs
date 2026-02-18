#![cfg(feature = "cli")]

mod support;

use clap::Parser;
use lxmf::cli::app::{run_cli, Cli};
use lxmf::cli::daemon::DaemonSupervisor;
use lxmf::cli::profile::{init_profile, load_profile_settings, save_profile_settings};
use std::os::unix::fs::PermissionsExt;
use std::sync::{Mutex, OnceLock};
use support::lock_config_root;

const STARTUP_GRACE_ENV_MS: &str = "LXMF_DAEMON_STARTUP_GRACE_MS";
const STARTUP_POLL_ENV_MS: &str = "LXMF_DAEMON_STARTUP_POLL_MS";

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct StartupTimingGuard;

impl StartupTimingGuard {
    fn fast() -> Self {
        // Keep startup checks fast for deterministic unit test runtime.
        std::env::set_var(STARTUP_GRACE_ENV_MS, "150");
        std::env::set_var(STARTUP_POLL_ENV_MS, "10");
        Self
    }
}

impl Drop for StartupTimingGuard {
    fn drop(&mut self) {
        std::env::remove_var(STARTUP_GRACE_ENV_MS);
        std::env::remove_var(STARTUP_POLL_ENV_MS);
    }
}

#[test]
fn daemon_start_uses_profile_managed_when_flag_omitted() {
    let _guard = env_lock().lock().unwrap();
    let _startup = StartupTimingGuard::fast();
    let temp = tempfile::tempdir().unwrap();
    let _config_root_guard = lock_config_root(temp.path());

    init_profile("daemon-cmd-managed", true, Some("127.0.0.1:4552".into())).unwrap();

    let fake = temp.path().join("fake-reticulumd.sh");
    std::fs::write(&fake, "#!/bin/sh\ntrap 'exit 0' TERM INT\nwhile true; do sleep 1; done\n")
        .unwrap();
    let mut perms = std::fs::metadata(&fake).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&fake, perms).unwrap();

    let mut settings = load_profile_settings("daemon-cmd-managed").unwrap();
    settings.reticulumd_path = Some(fake.display().to_string());
    save_profile_settings(&settings).unwrap();

    let start = Cli::parse_from(["lxmf", "--profile", "daemon-cmd-managed", "daemon", "start"]);
    run_cli(start).unwrap();

    let supervisor = DaemonSupervisor::new(
        "daemon-cmd-managed",
        load_profile_settings("daemon-cmd-managed").unwrap(),
    );
    assert!(supervisor.status().unwrap().running);

    let stop = Cli::parse_from(["lxmf", "--profile", "daemon-cmd-managed", "daemon", "stop"]);
    run_cli(stop).unwrap();
}

#[test]
fn daemon_start_external_without_managed_flag_fails() {
    let _guard = env_lock().lock().unwrap();
    let _startup = StartupTimingGuard::fast();
    let temp = tempfile::tempdir().unwrap();
    let _config_root_guard = lock_config_root(temp.path());

    init_profile("daemon-cmd-external", false, Some("127.0.0.1:4553".into())).unwrap();

    let start = Cli::parse_from(["lxmf", "--profile", "daemon-cmd-external", "daemon", "start"]);
    let err = run_cli(start).unwrap_err().to_string();
    assert!(err.contains("external mode"));
}
