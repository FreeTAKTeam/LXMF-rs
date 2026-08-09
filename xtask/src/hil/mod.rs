mod config;
mod evidence;
mod model;
mod reset;
mod runner;
mod support;

use anyhow::{Context, Result};
use clap::Subcommand;
use std::path::Path;

#[derive(Debug, Subcommand)]
pub enum HilCommand {
    /// Check local host and lab prerequisites.
    Doctor(runner::DoctorArgs),
    /// List configured profiles and cases.
    List(runner::ListArgs),
    /// Execute the requested HIL level and emit evidence.
    Run(runner::RunArgs),
    /// Aggregate run evidence into a support matrix.
    Report(runner::ReportArgs),
}

pub fn run(command: HilCommand) -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("locate repository root for HIL controller")?;
    let config = config::HilConfig::load(root, None)?;
    match command {
        HilCommand::Doctor(args) => runner::doctor(&config, args),
        HilCommand::List(args) => runner::list(&config, args),
        HilCommand::Run(args) => runner::run(&config, args),
        HilCommand::Report(args) => runner::report(&config, args),
    }
}
