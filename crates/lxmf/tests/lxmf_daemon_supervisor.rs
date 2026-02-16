#![cfg(feature = "cli")]

use lxmf::cli::daemon::DaemonSupervisor;
use lxmf::cli::profile::{init_profile, profile_paths, save_profile_settings, ProfileSettings};
use std::os::unix::fs::PermissionsExt;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

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
fn daemon_supervisor_start_stop_cycle() {
    let _guard = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let _startup = StartupTimingGuard::fast();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("LXMF_CONFIG_ROOT", temp.path());

    init_profile("daemon-test", true, Some("127.0.0.1:4550".into())).unwrap();
    let paths = profile_paths("daemon-test").unwrap();

    let fake = temp.path().join("fake-reticulumd.sh");
    std::fs::write(&fake, "#!/bin/sh\ntrap 'exit 0' TERM INT\nwhile true; do sleep 1; done\n")
        .unwrap();
    let mut perms = std::fs::metadata(&fake).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&fake, perms).unwrap();

    let settings = ProfileSettings {
        name: "daemon-test".into(),
        managed: true,
        rpc: "127.0.0.1:4550".into(),
        display_name: None,
        reticulumd_path: Some(fake.display().to_string()),
        db_path: None,
        identity_path: None,
        transport: None,
    };
    save_profile_settings(&settings).unwrap();

    let supervisor = DaemonSupervisor::new("daemon-test", settings);
    let started = supervisor.start(None, None, None).unwrap();
    assert!(started.running);
    assert!(paths.daemon_pid.exists());

    let status = supervisor.status().unwrap();
    assert!(status.running);

    let stopped = supervisor.stop().unwrap();
    assert!(!stopped.running);

    std::env::remove_var("LXMF_CONFIG_ROOT");
}

#[test]
fn daemon_supervisor_errors_when_reticulumd_missing() {
    let _guard = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let _startup = StartupTimingGuard::fast();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("LXMF_CONFIG_ROOT", temp.path());

    init_profile("daemon-missing", true, Some("127.0.0.1:4554".into())).unwrap();
    let settings = ProfileSettings {
        name: "daemon-missing".into(),
        managed: true,
        rpc: "127.0.0.1:4554".into(),
        display_name: None,
        reticulumd_path: Some(temp.path().join("nope-reticulumd").display().to_string()),
        db_path: None,
        identity_path: None,
        transport: None,
    };
    save_profile_settings(&settings).unwrap();

    let supervisor = DaemonSupervisor::new("daemon-missing", settings);
    let err = supervisor.start(None, None, None).unwrap_err().to_string();
    assert!(err.contains("not found"));

    std::env::remove_var("LXMF_CONFIG_ROOT");
}

#[test]
fn daemon_supervisor_errors_when_process_exits_immediately() {
    let _guard = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("LXMF_CONFIG_ROOT", temp.path());

    init_profile("daemon-exit-fast", true, Some("127.0.0.1:4555".into())).unwrap();

    let fake = temp.path().join("exit-fast-reticulumd.sh");
    std::fs::write(&fake, "#!/bin/sh\nexit 1\n").unwrap();
    let mut perms = std::fs::metadata(&fake).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&fake, perms).unwrap();

    let settings = ProfileSettings {
        name: "daemon-exit-fast".into(),
        managed: true,
        rpc: "127.0.0.1:4555".into(),
        display_name: None,
        reticulumd_path: Some(fake.display().to_string()),
        db_path: None,
        identity_path: None,
        transport: None,
    };
    save_profile_settings(&settings).unwrap();

    let supervisor = DaemonSupervisor::new("daemon-exit-fast", settings);
    let err = supervisor.start(None, None, None).unwrap_err().to_string();
    assert!(err.contains("exited during startup"));

    std::env::remove_var("LXMF_CONFIG_ROOT");
}

#[test]
fn daemon_supervisor_drops_empty_identity_stub_before_start() {
    let _guard = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let _startup = StartupTimingGuard::fast();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("LXMF_CONFIG_ROOT", temp.path());

    init_profile("daemon-empty-id", true, Some("127.0.0.1:4556".into())).unwrap();
    let paths = profile_paths("daemon-empty-id").unwrap();
    std::fs::write(&paths.identity_file, []).unwrap();

    let fake = temp.path().join("fake-reticulumd.sh");
    std::fs::write(&fake, "#!/bin/sh\ntrap 'exit 0' TERM INT\nwhile true; do sleep 1; done\n")
        .unwrap();
    let mut perms = std::fs::metadata(&fake).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&fake, perms).unwrap();

    let settings = ProfileSettings {
        name: "daemon-empty-id".into(),
        managed: true,
        rpc: "127.0.0.1:4556".into(),
        display_name: None,
        reticulumd_path: Some(fake.display().to_string()),
        db_path: None,
        identity_path: None,
        transport: None,
    };
    save_profile_settings(&settings).unwrap();

    let supervisor = DaemonSupervisor::new("daemon-empty-id", settings);
    supervisor.start(None, None, None).unwrap();
    assert!(!paths.identity_file.exists());
    supervisor.stop().unwrap();

    std::env::remove_var("LXMF_CONFIG_ROOT");
}

#[test]
fn daemon_supervisor_infers_transport_when_interfaces_are_enabled() {
    let _guard = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let _startup = StartupTimingGuard::fast();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("LXMF_CONFIG_ROOT", temp.path());

    init_profile("daemon-infer-transport", true, Some("127.0.0.1:4557".into())).unwrap();
    let paths = profile_paths("daemon-infer-transport").unwrap();
    std::fs::write(
        &paths.reticulum_toml,
        "[[interfaces]]\nname = \"uplink\"\ntype = \"tcp_client\"\nenabled = true\nhost = \"rmap.world\"\nport = 4242\n",
    )
    .unwrap();

    let args_log = temp.path().join("reticulumd-args.txt");
    let fake = temp.path().join("fake-reticulumd.sh");
    std::fs::write(
        &fake,
        format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\ntrap 'exit 0' TERM INT\nwhile true; do sleep 1; done\n",
            args_log.display()
        ),
    )
    .unwrap();
    let mut perms = std::fs::metadata(&fake).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&fake, perms).unwrap();

    let settings = ProfileSettings {
        name: "daemon-infer-transport".into(),
        managed: true,
        rpc: "127.0.0.1:4557".into(),
        display_name: None,
        reticulumd_path: Some(fake.display().to_string()),
        db_path: None,
        identity_path: None,
        transport: None,
    };
    save_profile_settings(&settings).unwrap();

    let supervisor = DaemonSupervisor::new("daemon-infer-transport", settings);
    let started = supervisor.start(None, None, None).unwrap();
    assert!(started.transport_inferred);
    assert_eq!(started.transport.as_deref(), Some("127.0.0.1:0"));

    let deadline = Instant::now() + Duration::from_secs(1);
    let mut args = String::new();
    while Instant::now() < deadline {
        if let Ok(content) = std::fs::read_to_string(&args_log) {
            if !content.trim().is_empty() {
                args = content;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    assert!(args.contains("--transport 127.0.0.1:0"), "args: {args}");
    supervisor.stop().unwrap();

    std::env::remove_var("LXMF_CONFIG_ROOT");
}

#[test]
fn daemon_status_clears_stale_pid_file() {
    let _guard = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let _startup = StartupTimingGuard::fast();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("LXMF_CONFIG_ROOT", temp.path());

    init_profile("daemon-stale-pid", true, Some("127.0.0.1:4558".into())).unwrap();
    let paths = profile_paths("daemon-stale-pid").unwrap();
    std::fs::write(&paths.daemon_pid, "999999\n").unwrap();

    let settings = ProfileSettings {
        name: "daemon-stale-pid".into(),
        managed: true,
        rpc: "127.0.0.1:4558".into(),
        display_name: None,
        reticulumd_path: None,
        db_path: None,
        identity_path: None,
        transport: None,
    };
    save_profile_settings(&settings).unwrap();

    let supervisor = DaemonSupervisor::new("daemon-stale-pid", settings);
    let status = supervisor.status().unwrap();
    assert!(!status.running);
    assert_eq!(status.pid, None);
    assert!(!paths.daemon_pid.exists());

    std::env::remove_var("LXMF_CONFIG_ROOT");
}
