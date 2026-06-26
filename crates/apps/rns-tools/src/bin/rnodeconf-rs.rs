use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};

use clap::{Parser, Subcommand};
use rns_rpc::e2e_harness::{build_http_post, build_rpc_frame, parse_http_response_body};
use serde_json::json;

#[derive(Debug, Parser)]
#[command(name = "rnodeconf-rs")]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:4243")]
    rpc: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    QueryRadioState {
        #[arg(long = "interface")]
        iface: String,
    },
    ReadConfig {
        #[arg(long = "interface")]
        iface: String,
    },
    ReadRom {
        #[arg(long = "interface")]
        iface: String,
    },
    Blink {
        #[arg(long = "interface")]
        iface: String,
        #[arg(long)]
        pattern: u8,
    },
    SetDisplayIntensity {
        #[arg(long = "interface")]
        iface: String,
        #[arg(long)]
        intensity: u8,
    },
    SetDisplayBlanking {
        #[arg(long = "interface")]
        iface: String,
        #[arg(long)]
        timeout: u8,
    },
    SetDisplayRotation {
        #[arg(long = "interface")]
        iface: String,
        #[arg(long)]
        rotation: u8,
    },
    ReconditionDisplay {
        #[arg(long = "interface")]
        iface: String,
    },
    SetDisplayAddress {
        #[arg(long = "interface")]
        iface: String,
        #[arg(long)]
        address: u8,
    },
    SetNeopixelIntensity {
        #[arg(long = "interface")]
        iface: String,
        #[arg(long)]
        intensity: u8,
    },
    DisableInterferenceAvoidance {
        #[arg(long = "interface")]
        iface: String,
    },
    EnableInterferenceAvoidance {
        #[arg(long = "interface")]
        iface: String,
    },
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match run(&cli, &mut io::stdout()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rnodeconf-rs: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli, output: &mut dyn Write) -> io::Result<()> {
    let params = match &cli.command {
        Command::QueryRadioState { iface } => json!({
            "iface": iface,
            "command": "radio_state_query",
        }),
        Command::ReadConfig { iface } => json!({
            "iface": iface,
            "command": "config_read",
        }),
        Command::ReadRom { iface } => json!({
            "iface": iface,
            "command": "rom_read",
        }),
        Command::Blink { iface, pattern } => json!({
            "iface": iface,
            "command": "blink",
            "pattern": pattern,
        }),
        Command::SetDisplayIntensity { iface, intensity } => json!({
            "iface": iface,
            "command": "display_intensity",
            "intensity": intensity,
        }),
        Command::SetDisplayBlanking { iface, timeout } => json!({
            "iface": iface,
            "command": "display_blanking",
            "timeout": timeout,
        }),
        Command::SetDisplayRotation { iface, rotation } => json!({
            "iface": iface,
            "command": "display_rotation",
            "rotation": rotation,
        }),
        Command::ReconditionDisplay { iface } => json!({
            "iface": iface,
            "command": "display_recondition",
        }),
        Command::SetDisplayAddress { iface, address } => json!({
            "iface": iface,
            "command": "display_address",
            "address": address,
        }),
        Command::SetNeopixelIntensity { iface, intensity } => json!({
            "iface": iface,
            "command": "neopixel_intensity",
            "intensity": intensity,
        }),
        Command::DisableInterferenceAvoidance { iface } => json!({
            "iface": iface,
            "command": "disable_interference_avoidance",
            "disabled": true,
        }),
        Command::EnableInterferenceAvoidance { iface } => json!({
            "iface": iface,
            "command": "enable_interference_avoidance",
        }),
    };
    let response = rpc_call(&cli.rpc, 1, "rnode_management", Some(params))?;
    let result = ensure_rpc_ok(response, "rnode_management")?
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing RNode result"))?;
    writeln!(output, "{}", serde_json::to_string_pretty(&result)?)?;
    Ok(())
}

fn rpc_call(
    rpc: &str,
    id: u64,
    method: &str,
    params: Option<serde_json::Value>,
) -> io::Result<rns_rpc::RpcResponse> {
    let frame = build_rpc_frame(id, method, params)?;
    let request = build_http_post("/rpc", rpc, &frame);
    let mut stream = TcpStream::connect(rpc)?;
    stream.write_all(&request)?;
    stream.shutdown(Shutdown::Write)?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    let body = parse_http_response_body(&response)?;
    rns_rpc::rpc::codec::decode_frame(&body)
}

fn ensure_rpc_ok(
    response: rns_rpc::RpcResponse,
    context: &str,
) -> io::Result<Option<serde_json::Value>> {
    if let Some(error) = response.error {
        return Err(io::Error::other(format!(
            "{} failed: {} ({})",
            context, error.message, error.code
        )));
    }
    Ok(response.result)
}
