use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::time::Duration;

use clap::Parser;
use rns_rpc::e2e_harness::{build_http_post, build_rpc_frame, parse_http_response_body};
use serde_json::{json, Value};

const DESTINATION_HASH_BYTES: usize = 16;
const DESTINATION_HASH_HEX_LEN: usize = DESTINATION_HASH_BYTES * 2;
const REQUEST_PATH_METHOD: &str = "request_path";

#[derive(Debug, Parser)]
#[command(name = "rnpath-rs", about = "Request Reticulum path discovery through daemon RPC.")]
struct Cli {
    #[arg(value_name = "DESTINATION_HASH", value_parser = parse_destination_hash)]
    destination_hash: String,

    #[arg(long, default_value = "127.0.0.1:4243")]
    rpc: String,

    #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..=3600))]
    timeout: u64,

    #[arg(long)]
    json: bool,
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match run(&cli, &mut io::stdout()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rnpath-rs: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli, output: &mut dyn Write) -> io::Result<()> {
    let params = json!({
        "destination_hash": cli.destination_hash,
        "timeout_secs": cli.timeout,
    });
    let response = rpc_call(&cli.rpc, 1, REQUEST_PATH_METHOD, Some(params), cli.rpc_timeout())?;
    let result = ensure_rpc_ok(response, REQUEST_PATH_METHOD)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "missing path discovery result")
    })?;
    if !result.get("path_found").and_then(Value::as_bool).unwrap_or(false) {
        let status = value_str(&result, "status").unwrap_or("unknown");
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("path discovery for {} did not complete: {status}", cli.destination_hash),
        ));
    }

    if cli.json {
        writeln!(output, "{}", serde_json::to_string_pretty(&result)?)?;
    } else {
        write_human_path_result(output, &cli.destination_hash, &result)?;
    }
    Ok(())
}

fn rpc_call(
    rpc: &str,
    id: u64,
    method: &str,
    params: Option<serde_json::Value>,
    timeout: Duration,
) -> io::Result<rns_rpc::RpcResponse> {
    let frame = build_rpc_frame(id, method, params)?;
    let request = build_http_post("/rpc", rpc, &frame);
    let addr = rpc.to_socket_addrs()?.next().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "RPC address did not resolve")
    })?;
    let mut stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.write_all(&request)?;
    stream.shutdown(Shutdown::Write)?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    let body = parse_http_response_body(&response)?;
    rns_rpc::rpc::codec::decode_frame(&body)
}

fn ensure_rpc_ok(
    response: rns_rpc::RpcResponse,
    method: &str,
) -> io::Result<Option<serde_json::Value>> {
    if let Some(error) = response.error {
        let detail = if error.code == "NOT_IMPLEMENTED" {
            format!("{method} is not implemented by this daemon; rnpath-rs is ready for the daemon path RPC")
        } else {
            format!("{method} failed: {} ({})", error.message, error.code)
        };
        return Err(io::Error::other(detail));
    }
    Ok(response.result)
}

fn write_human_path_result(
    output: &mut dyn Write,
    destination_hash: &str,
    result: &Value,
) -> io::Result<()> {
    let reported_destination =
        value_str(result, "destination_hash").unwrap_or(destination_hash).to_string();
    writeln!(output, "Path request: {reported_destination}")?;
    for key in [
        "status",
        "state",
        "requested",
        "path_found",
        "next_hop",
        "via",
        "interface",
        "hops",
        "cost",
    ] {
        write_optional_field(output, result, key)?;
    }
    Ok(())
}

fn write_optional_field(output: &mut dyn Write, result: &Value, key: &str) -> io::Result<()> {
    let Some(value) = result.get(key) else {
        return Ok(());
    };
    if value.is_null() {
        return Ok(());
    }
    match value.as_str() {
        Some(text) => writeln!(output, "{key}={text}"),
        None => writeln!(output, "{key}={value}"),
    }
}

fn value_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
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

impl Cli {
    fn rpc_timeout(&self) -> Duration {
        Duration::from_secs(self.timeout.max(1))
    }
}
