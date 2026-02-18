#![cfg(feature = "cli")]

mod support;

use clap::Parser;
use lxmf::cli::app::{Cli, RuntimeContext};
use lxmf::cli::profile::{init_profile, select_profile};
use support::lock_config_root;

#[test]
fn runtime_context_rejects_unknown_explicit_profile() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _config_root_guard = lock_config_root(temp.path());

    init_profile("ops", false, None).expect("init profile");
    select_profile("ops").expect("select profile");

    let cli = Cli::parse_from(["lxmf", "--profile", "opss", "daemon", "status"]);
    let err = RuntimeContext::load(cli).expect_err("missing explicit profile should fail");
    let err_text = err.to_string();
    assert!(err_text.contains("profile 'opss' does not exist"));
}

#[test]
fn runtime_context_uses_selected_when_default_is_missing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _config_root_guard = lock_config_root(temp.path());

    init_profile("ops", false, None).expect("init profile");
    select_profile("ops").expect("select profile");

    let cli = Cli::parse_from(["lxmf", "daemon", "status"]);
    let ctx = RuntimeContext::load(cli).expect("fallback to selected profile");
    assert_eq!(ctx.profile_name, "ops");
}
