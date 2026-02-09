use lxmf::cli::daemon::DaemonSupervisor;
use lxmf::cli::profile::{
    init_profile, profile_paths, save_profile_settings, ProfileSettings,
};
use std::os::unix::fs::PermissionsExt;
use std::sync::{Mutex, OnceLock};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn daemon_supervisor_start_stop_cycle() {
    let _guard = env_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("LXMF_CONFIG_ROOT", temp.path());

    init_profile("daemon-test", true, Some("127.0.0.1:4550".into())).unwrap();
    let paths = profile_paths("daemon-test").unwrap();

    let fake = temp.path().join("fake-reticulumd.sh");
    std::fs::write(
        &fake,
        "#!/bin/sh\ntrap 'exit 0' TERM INT\nwhile true; do sleep 1; done\n",
    )
    .unwrap();
    let mut perms = std::fs::metadata(&fake).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&fake, perms).unwrap();

    let settings = ProfileSettings {
        name: "daemon-test".into(),
        managed: true,
        rpc: "127.0.0.1:4550".into(),
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
