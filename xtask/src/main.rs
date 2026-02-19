use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::process::Command;

#[derive(Parser)]
#[command(name = "xtask")]
struct Xtask {
    #[command(subcommand)]
    command: XtaskCommand,
}

#[derive(Subcommand)]
enum XtaskCommand {
    Ci,
    ReleaseCheck,
    ApiDiff,
    Licenses,
    MigrationChecks,
    ArchitectureChecks,
    ForbiddenDeps,
}

fn main() -> Result<()> {
    let xtask = Xtask::parse();
    match xtask.command {
        XtaskCommand::Ci => run_ci(),
        XtaskCommand::ReleaseCheck => run_release_check(),
        XtaskCommand::ApiDiff => run_api_diff(),
        XtaskCommand::Licenses => run_licenses(),
        XtaskCommand::MigrationChecks => run_migration_checks(),
        XtaskCommand::ArchitectureChecks => run_architecture_checks(),
        XtaskCommand::ForbiddenDeps => run_forbidden_deps(),
    }
}

fn run_ci() -> Result<()> {
    run("cargo", &["fmt", "--all", "--", "--check"])?;
    run(
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--no-deps",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    run("cargo", &["test", "--workspace"])?;
    run("cargo", &["doc", "--workspace", "--no-deps"])?;
    run_migration_checks()?;
    run_architecture_checks()?;
    Ok(())
}

fn run_release_check() -> Result<()> {
    run_ci()?;
    run("cargo", &["deny", "check"])?;
    run("cargo", &["audit"])?;
    Ok(())
}

fn run_api_diff() -> Result<()> {
    for manifest in [
        "crates/libs/lxmf-core/Cargo.toml",
        "crates/libs/lxmf-runtime/Cargo.toml",
        "crates/libs/rns-core/Cargo.toml",
        "crates/libs/rns-transport/Cargo.toml",
        "crates/libs/rns-rpc/Cargo.toml",
    ] {
        let command = format!(
            "RUSTUP_TOOLCHAIN=nightly RUSTC=\"$(rustup which --toolchain nightly rustc)\" RUSTDOC=\"$(rustup which --toolchain nightly rustdoc)\" cargo public-api --manifest-path {manifest}"
        );
        run("bash", &["-lc", &command])?;
    }
    Ok(())
}

fn run_licenses() -> Result<()> {
    run("cargo", &["deny", "check", "licenses"])
}

fn run_migration_checks() -> Result<()> {
    let enforce_legacy_imports =
        std::env::var("ENFORCE_LEGACY_APP_IMPORTS").unwrap_or("1".to_string());
    let enforce_legacy_shims =
        std::env::var("ENFORCE_RETM_LEGACY_SHIMS").unwrap_or("1".to_string());
    run_boundary_checks(&enforce_legacy_imports, &enforce_legacy_shims)?;
    run(
        "bash",
        &["-lc", "! rg -n 'crates/(lxmf|reticulum|reticulum-daemon)/' README.md .github/workflows || exit 1"],
    )?;
    Ok(())
}

fn run_architecture_checks() -> Result<()> {
    run_forbidden_deps()
}

fn run_forbidden_deps() -> Result<()> {
    let enforce_legacy_imports =
        std::env::var("ENFORCE_LEGACY_APP_IMPORTS").unwrap_or("1".to_string());
    let enforce_legacy_shims =
        std::env::var("ENFORCE_RETM_LEGACY_SHIMS").unwrap_or("1".to_string());
    run_boundary_checks(&enforce_legacy_imports, &enforce_legacy_shims)
}

fn run_boundary_checks(enforce_legacy_imports: &str, enforce_legacy_shims: &str) -> Result<()> {
    let command = format!(
        "ENFORCE_LEGACY_APP_IMPORTS={enforce_legacy_imports} ENFORCE_RETM_LEGACY_SHIMS={enforce_legacy_shims} ./tools/scripts/check-boundaries.sh"
    );
    run("bash", &["-lc", &command])
}

fn run(cmd: &str, args: &[&str]) -> Result<()> {
    let status =
        Command::new(cmd).args(args).status().with_context(|| format!("failed to spawn {cmd}"))?;
    if !status.success() {
        bail!("command failed: {cmd} {}", args.join(" "));
    }
    Ok(())
}
