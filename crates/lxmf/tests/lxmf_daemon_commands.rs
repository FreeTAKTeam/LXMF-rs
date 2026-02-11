#![cfg(feature = "cli")]

use clap::Parser;
use lxmf::cli::app::{run_cli, Cli};
use lxmf::cli::daemon::DaemonSupervisor;
use lxmf::cli::profile::{init_profile, load_profile_settings, save_profile_settings};
use std::os::unix::fs::PermissionsExt;
use std::sync::{Mutex, OnceLock};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn daemon_start_uses_profile_managed_when_flag_omitted() {
    let _guard = env_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("LXMF_CONFIG_ROOT", temp.path());

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

    std::env::remove_var("LXMF_CONFIG_ROOT");
}

#[test]
fn daemon_start_external_without_managed_flag_fails() {
    let _guard = env_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("LXMF_CONFIG_ROOT", temp.path());

    init_profile("daemon-cmd-external", false, Some("127.0.0.1:4553".into())).unwrap();

    let start = Cli::parse_from(["lxmf", "--profile", "daemon-cmd-external", "daemon", "start"]);
    let err = run_cli(start).unwrap_err().to_string();
    assert!(err.contains("external mode"));

    std::env::remove_var("LXMF_CONFIG_ROOT");
}
