mod rncp_parts;

use clap::Parser;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Parser)]
#[command(name = "rncp", about = "Copy files through a deterministic Reticulum transfer workflow")]
pub(crate) struct Cli {
    source: Option<PathBuf>,
    destination: Option<String>,
    #[arg(long, help = "Restrict both paths to this simulation root")]
    simulate_root: Option<PathBuf>,
    #[arg(long)]
    force: bool,
    #[arg(long, value_name = "HOST:PORT", action = clap::ArgAction::Append)]
    listen: Vec<String>,
    #[arg(long, value_name = "HOST:PORT", action = clap::ArgAction::Append)]
    connect: Vec<String>,
    #[arg(long, value_name = "SEED", help = "Deterministically derive the local identity")]
    identity_seed: Option<String>,
    #[arg(long, value_name = "PATH", help = "Read or create the local identity key file")]
    identity: Option<PathBuf>,
    #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..=3600))]
    timeout: u64,
    #[arg(long, help = "Print the receive destination and exit")]
    print_identity: bool,
    #[arg(short = 'F', long, help = "Allow incoming authenticated fetch requests")]
    allow_fetch: bool,
    #[arg(short = 'n', long, help = "Accept incoming resources without an identity allow-list")]
    no_auth: bool,
    #[arg(short = 'a', long = "allowed-identity", value_name = "IDENTITY_HASH", action = clap::ArgAction::Append)]
    allowed_identity: Vec<String>,
    #[arg(short = 's', long, value_name = "PATH", help = "Directory for received files")]
    save: Option<PathBuf>,
    #[arg(short = 'O', long, help = "Allow received files to replace an existing file")]
    overwrite: bool,
    #[arg(short = 'f', long, help = "Fetch the named remote file instead of sending a local file")]
    fetch: bool,
    #[arg(short = 'S', long, help = "Suppress progress output")]
    silent: bool,
    #[arg(short = 'C', long, help = "Disable automatic Resource compression")]
    no_compress: bool,
    #[arg(
        short = 'j',
        long,
        value_name = "PATH",
        help = "Restrict fetch requests to this directory"
    )]
    jail: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    if cli.network_mode() {
        return match rncp_parts::network::run(&cli).await {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("rncp: {error}");
                if error.kind() == io::ErrorKind::NotADirectory {
                    std::process::ExitCode::from(3)
                } else {
                    std::process::ExitCode::FAILURE
                }
            }
        };
    }

    match copy(&cli) {
        Ok(bytes) => {
            println!("copied {bytes} bytes");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("rncp: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

impl Cli {
    pub(crate) fn network_mode(&self) -> bool {
        !self.listen.is_empty()
            || !self.connect.is_empty()
            || self.print_identity
            || self.fetch
            || self.allow_fetch
            || self.no_auth
            || !self.allowed_identity.is_empty()
            || self.identity_seed.is_some()
            || self.identity.is_some()
            || self.save.is_some()
            || self.silent
            || self.no_compress
            || self.jail.is_some()
    }

    pub(crate) fn source(&self) -> io::Result<&Path> {
        self.source
            .as_deref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing source file"))
    }

    pub(crate) fn destination(&self) -> io::Result<&str> {
        self.destination
            .as_deref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing destination hash"))
    }
}

fn copy(cli: &Cli) -> io::Result<u64> {
    let source = scoped_path(cli.source()?, cli.simulate_root.as_deref())?;
    let destination = scoped_path(Path::new(cli.destination()?), cli.simulate_root.as_deref())?;
    if destination.exists() && !cli.force {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "destination exists; use --force",
        ));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source, destination)
}

fn scoped_path(path: &Path, root: Option<&Path>) -> io::Result<PathBuf> {
    if path.components().any(|component| component == Component::ParentDir) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "parent traversal is denied"));
    }
    let Some(root) = root else { return Ok(path.to_path_buf()) };
    let root = root.canonicalize()?;
    let candidate = if path.is_absolute() { path.to_path_buf() } else { root.join(path) };
    let parent = candidate.parent().unwrap_or(&candidate);
    fs::create_dir_all(parent)?;
    let parent = parent.canonicalize()?;
    if !parent.starts_with(&root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "path escapes simulation root",
        ));
    }
    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn simulation_copy_is_root_scoped_and_binary_safe() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::write(temp.path().join("source"), [0, 1, 2, 255]).expect("source");
        let cli = Cli {
            source: Some("source".into()),
            destination: Some("nested/dest".into()),
            simulate_root: Some(temp.path().into()),
            force: false,
            listen: Vec::new(),
            connect: Vec::new(),
            identity_seed: None,
            identity: None,
            timeout: 30,
            print_identity: false,
            allow_fetch: false,
            no_auth: false,
            allowed_identity: Vec::new(),
            save: None,
            overwrite: false,
            fetch: false,
            silent: false,
            no_compress: false,
            jail: None,
        };
        assert_eq!(copy(&cli).expect("copy"), 4);
        assert_eq!(fs::read(temp.path().join("nested/dest")).expect("dest"), [0, 1, 2, 255]);
    }
}
