mod rnsh_parts;

use clap::Parser;
use std::collections::BTreeSet;
use std::io;
use std::path::PathBuf;
use std::process::{Command, ExitStatus};
use std::time::Duration;

#[derive(Debug, Parser)]
#[command(
    name = "rnsh",
    about = "Run an authorised command locally or through a Reticulum shell workflow"
)]
pub(crate) struct Cli {
    #[arg(long, value_name = "PATH")]
    pub(crate) root: Option<PathBuf>,
    #[arg(long = "allow", value_name = "IDENTITY_OR_COMMAND", action = clap::ArgAction::Append)]
    pub(crate) allowed: Vec<String>,
    #[arg(long, value_name = "HOST:PORT", action = clap::ArgAction::Append)]
    pub(crate) listen: Vec<String>,
    #[arg(long, value_name = "HOST:PORT", action = clap::ArgAction::Append)]
    pub(crate) connect: Vec<String>,
    #[arg(long, value_name = "SEED")]
    pub(crate) identity_seed: Option<String>,
    #[arg(long, value_name = "PATH")]
    pub(crate) identity: Option<PathBuf>,
    #[arg(short = 'b', long, value_name = "PERIOD", default_missing_value = "0")]
    pub(crate) announce: Option<u32>,
    #[arg(short = 'n', long)]
    pub(crate) no_auth: bool,
    #[arg(short = 'N', long)]
    pub(crate) no_id: bool,
    #[arg(short = 'm', long)]
    pub(crate) mirror: bool,
    #[arg(short = 'A', long)]
    pub(crate) remote_command_as_args: bool,
    #[arg(short = 'C', long)]
    pub(crate) no_remote_command: bool,
    #[arg(short = 'w', long, default_value_t = 30.0)]
    pub(crate) timeout: f64,
    #[arg(short = 'p', long)]
    pub(crate) print_identity: bool,
    #[arg(value_name = "DESTINATION")]
    pub(crate) destination: Option<String>,
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub(crate) command: Vec<String>,
}

impl Cli {
    pub(crate) fn timeout_duration(&self) -> io::Result<Duration> {
        if !self.timeout.is_finite() || self.timeout <= 0.0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "--timeout must be positive"));
        }
        Ok(Duration::from_secs_f64(self.timeout))
    }

    pub(crate) fn default_command(&self) -> Vec<String> {
        let command = if !self.listen.is_empty() {
            self.destination.iter().cloned().chain(self.command.iter().cloned()).collect::<Vec<_>>()
        } else {
            self.command.clone()
        };
        if command.is_empty() {
            std::env::var("SHELL")
                .ok()
                .filter(|shell| !shell.is_empty())
                .map(|shell| vec![shell])
                .unwrap_or_else(|| vec!["sh".to_string()])
        } else {
            command
        }
    }

    fn network_mode(&self) -> bool {
        !self.listen.is_empty()
            || !self.connect.is_empty()
            || self.destination.is_some()
            || self.print_identity
    }
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    if cli.network_mode() {
        return match rnsh_parts::network::run(&cli).await {
            Ok(code) => {
                std::process::ExitCode::from(if cli.mirror { code.max(0) as u8 } else { 0 })
            }
            Err(error) => {
                eprintln!("rnsh: {error}");
                std::process::ExitCode::FAILURE
            }
        };
    }

    match execute_local(&cli) {
        Ok(status) => std::process::ExitCode::from(status.code().unwrap_or(1) as u8),
        Err(error) => {
            eprintln!("rnsh: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn execute_local(cli: &Cli) -> io::Result<ExitStatus> {
    let executable = cli
        .command
        .first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "command required"))?;
    let allowed = cli.allowed.iter().cloned().collect::<BTreeSet<_>>();
    if !allowed.contains(executable) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "command denied by default-deny policy",
        ));
    }
    let root = cli
        .root
        .as_deref()
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "--root is required in local mode")
        })?
        .canonicalize()?;
    Command::new(executable).args(&cli.command[1..]).current_dir(root).status()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_shell_policy_denies_unlisted_command() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cli = Cli {
            root: Some(temp.path().into()),
            allowed: vec!["true".into()],
            listen: Vec::new(),
            connect: Vec::new(),
            identity_seed: None,
            identity: None,
            announce: None,
            no_auth: false,
            no_id: false,
            mirror: false,
            remote_command_as_args: false,
            no_remote_command: false,
            timeout: 30.0,
            print_identity: false,
            destination: None,
            command: vec!["false".into()],
        };
        assert_eq!(
            execute_local(&cli).expect_err("denied").kind(),
            io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn network_initiator_parses_without_a_destination_for_later_validation() {
        let cli = Cli::try_parse_from(["rnsh", "--connect", "127.0.0.1:4243"])
            .expect("parse network options");
        assert!(cli.destination.is_none());
        assert!(cli.network_mode());
    }
}
