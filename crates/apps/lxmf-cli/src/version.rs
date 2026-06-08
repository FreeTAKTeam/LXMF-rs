use clap::{Arg, ArgAction};
use std::ffi::OsStr;

pub const LXMF_RS_VERSION: &str = env!("LXMF_RS_VERSION");
pub const LXMF_CLI_CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn version_output() -> String {
    format!(
        "lxmf-rs {LXMF_RS_VERSION}\n\
         lxmf-cli crate {LXMF_CLI_CRATE_VERSION}\n\
         python-reticulum reference {} {}\n\
         python-lxmf reference {} {}",
        lxmf_sdk::PYTHON_RETICULUM_REFERENCE_VERSION,
        lxmf_sdk::PYTHON_RETICULUM_REFERENCE_REF,
        lxmf_sdk::PYTHON_LXMF_REFERENCE_VERSION,
        lxmf_sdk::PYTHON_LXMF_REFERENCE_REF
    )
}

pub fn parse_with_version<T>() -> T
where
    T: clap::Parser + clap::CommandFactory,
{
    if std::env::args_os()
        .skip(1)
        .any(|arg| arg == OsStr::new("--version") || arg == OsStr::new("-V"))
    {
        println!("{}", version_output());
        std::process::exit(0);
    }

    let matches = T::command()
        .arg(
            Arg::new("diagnostic-version")
                .short('V')
                .long("version")
                .help("Print project, crate, and compatibility reference versions")
                .action(ArgAction::SetTrue),
        )
        .get_matches();
    T::from_arg_matches(&matches).unwrap_or_else(|err| err.exit())
}
