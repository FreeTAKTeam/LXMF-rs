use clap::Parser;
use lxmf::cli::app::{Cli, RuntimeContext};
use lxmf::cli::profile::{init_profile, select_profile};
use std::sync::{Mutex, OnceLock};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn runtime_context_rejects_unknown_explicit_profile() {
    let _guard = env_lock().lock().expect("env lock");
    let temp = tempfile::tempdir().expect("tempdir");
    std::env::set_var("LXMF_CONFIG_ROOT", temp.path());

    init_profile("ops", false, None).expect("init profile");
    select_profile("ops").expect("select profile");

    let cli = Cli::parse_from(["lxmf", "--profile", "opss", "tui"]);
    let err = RuntimeContext::load(cli).expect_err("missing explicit profile should fail");
    let err_text = err.to_string();
    assert!(err_text.contains("profile 'opss' does not exist"));

    std::env::remove_var("LXMF_CONFIG_ROOT");
}

#[test]
fn runtime_context_uses_selected_when_default_is_missing() {
    let _guard = env_lock().lock().expect("env lock");
    let temp = tempfile::tempdir().expect("tempdir");
    std::env::set_var("LXMF_CONFIG_ROOT", temp.path());

    init_profile("ops", false, None).expect("init profile");
    select_profile("ops").expect("select profile");

    let cli = Cli::parse_from(["lxmf", "tui"]);
    let ctx = RuntimeContext::load(cli).expect("fallback to selected profile");
    assert_eq!(ctx.profile_name, "ops");

    std::env::remove_var("LXMF_CONFIG_ROOT");
}
