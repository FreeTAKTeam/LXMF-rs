use std::io;

use clap::Parser;

const DESTINATION_HASH_BYTES: usize = 16;
const DESTINATION_HASH_HEX_LEN: usize = DESTINATION_HASH_BYTES * 2;

#[derive(Debug, Parser)]
#[command(
    name = "rnpath-rs",
    about = "Validate Reticulum path discovery arguments; daemon path requests are not implemented yet."
)]
struct Cli {
    #[arg(value_name = "DESTINATION_HASH", value_parser = parse_destination_hash)]
    destination_hash: String,

    #[arg(long, default_value = "127.0.0.1:4243")]
    rpc: String,

    #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..=3600))]
    timeout: u64,
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rnpath-rs: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        format!(
            "path discovery for {} via {} is not wired to daemon RPC yet; timeout={}s",
            cli.destination_hash, cli.rpc, cli.timeout
        ),
    ))
}

fn parse_destination_hash(value: &str) -> Result<String, String> {
    if value.len() != DESTINATION_HASH_HEX_LEN {
        return Err(format!(
            "destination hash must be {DESTINATION_HASH_HEX_LEN} hexadecimal characters"
        ));
    }
    let bytes = hex::decode(value).map_err(|_| "destination hash must be hexadecimal")?;
    if bytes.len() != DESTINATION_HASH_BYTES {
        return Err(format!("destination hash must decode to {DESTINATION_HASH_BYTES} bytes"));
    }
    Ok(value.to_ascii_lowercase())
}
