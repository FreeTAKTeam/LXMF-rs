use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};

use clap::{Args, Parser, Subcommand};
use rns_rpc::e2e_harness::{build_http_post, build_rpc_frame, parse_http_response_body};
use serde_json::json;

#[derive(Debug, Parser)]
#[command(name = "weaveconf-rs")]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:4243")]
    rpc: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    EnableRemoteDisplay {
        #[command(flatten)]
        target: Target,
    },
    DisableRemoteDisplay {
        #[command(flatten)]
        target: Target,
    },
}

#[derive(Debug, Args)]
struct Target {
    #[arg(long = "interface")]
    iface: String,
    #[arg(long = "remote-switch-id-hex")]
    remote_switch_id_hex: Option<String>,
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match run(&cli, &mut io::stdout()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("weaveconf-rs: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli, output: &mut dyn Write) -> io::Result<()> {
    let params = match &cli.command {
        Command::EnableRemoteDisplay { target } => rpc_params(target, true),
        Command::DisableRemoteDisplay { target } => rpc_params(target, false),
    };
    let response = rpc_call(&cli.rpc, 1, "weave_remote_display_control", Some(params))?;
    let result = ensure_rpc_ok(response, "weave_remote_display_control")?
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing Weave result"))?;
    writeln!(output, "{}", serde_json::to_string_pretty(&result)?)?;
    Ok(())
}

fn rpc_params(target: &Target, enable: bool) -> serde_json::Value {
    let mut params = json!({
        "iface": target.iface,
        "enable": enable,
    });
    if let (Some(map), Some(remote_switch_id_hex)) =
        (params.as_object_mut(), target.remote_switch_id_hex.as_ref())
    {
        map.insert("remote_switch_id_hex".to_string(), json!(remote_switch_id_hex));
    }
    params
}

fn rpc_call(
    addr: &str,
    id: u64,
    method: &str,
    params: Option<serde_json::Value>,
) -> io::Result<rns_rpc::RpcResponse> {
    let frame = build_rpc_frame(id, method, params)?;
    let http = build_http_post("/rpc", addr, &frame);
    let mut stream = TcpStream::connect(addr)?;
    stream.write_all(&http)?;
    stream.shutdown(Shutdown::Write)?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    let body = parse_http_response_body(&raw)?;
    rns_rpc::rpc::codec::decode_frame(&body)
}

fn ensure_rpc_ok(
    response: rns_rpc::RpcResponse,
    method: &str,
) -> io::Result<Option<serde_json::Value>> {
    if let Some(error) = response.error {
        return Err(io::Error::other(format!(
            "{method} failed: {}: {}",
            error.code, error.message
        )));
    }
    Ok(response.result)
}
