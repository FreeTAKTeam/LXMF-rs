use clap::{Arg, ArgAction};

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
    let command = T::command().subcommand_required(false).arg(
        Arg::new("diagnostic-version")
            .short('V')
            .long("version")
            .help("Print project, crate, and compatibility reference versions")
            .action(ArgAction::SetTrue),
    );
    if command.try_get_matches().ok().is_some_and(|matches| matches.get_flag("diagnostic-version"))
    {
        println!("{}", version_output());
        std::process::exit(0);
    }

    T::parse()
}
