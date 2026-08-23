use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(unix)]
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Parser;
use rns_rpc::e2e_harness::{build_http_post, build_rpc_frame, parse_http_response_body};
use serde_json::{json, Value};
#[cfg(unix)]
use socket2::{Domain, SockAddr, Socket, Type};

const DESTINATION_HASH_BYTES: usize = 16;
const DESTINATION_HASH_HEX_LEN: usize = DESTINATION_HASH_BYTES * 2;
const DEFAULT_RPC_ADDR: &str = "127.0.0.1:4243";
const REQUEST_PATH_METHOD: &str = "request_path";
const RPC_READ_HEADROOM: Duration = Duration::from_secs(2);

#[derive(Debug, Parser)]
#[command(name = "rnpath-rs", about = "Request Reticulum path discovery through daemon RPC.")]
struct Cli {
    #[arg(value_name = "DESTINATION_HASH", value_parser = parse_destination_hash)]
    destination_hash: String,

    #[arg(long, value_name = "ADDR", help = "Daemon TCP RPC address (default: 127.0.0.1:4243)")]
    rpc: Option<String>,

    #[cfg(unix)]
    #[arg(long, value_name = "PATH", conflicts_with = "rpc")]
    rpc_unix: Option<PathBuf>,

    #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..=3600))]
    timeout: u64,

    #[arg(long)]
    json: bool,

    #[arg(long, value_name = "IFACE_HASH", value_parser = parse_destination_hash)]
    on_iface: Option<String>,

    #[arg(long, value_name = "TAG_HEX", value_parser = parse_request_tag_hex)]
    tag_hex: Option<String>,
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
    let medium_timeout = rpc_call(cli, 0, "medium_path_timeout", None)
        .ok()
        .and_then(|response| response.result)
        .and_then(|value| value.as_f64())
        .unwrap_or(0.0);
    let timeout = adaptive_timeout(cli.timeout, medium_timeout);
    let params = json!({
        "destination_hash": cli.destination_hash,
        "timeout_secs": timeout,
        "on_iface": cli.on_iface,
        "tag_hex": cli.tag_hex,
    });
    let response = rpc_call_with_adaptive_timeout(
        cli,
        1,
        REQUEST_PATH_METHOD,
        Some(params),
        Duration::from_secs(timeout),
    )?;
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

fn adaptive_timeout(configured: u64, medium_path_timeout: f64) -> u64 {
    configured.max(medium_path_timeout.max(0.0).ceil() as u64).max(1)
}

fn rpc_call(
    cli: &Cli,
    id: u64,
    method: &str,
    params: Option<serde_json::Value>,
) -> io::Result<rns_rpc::RpcResponse> {
    rpc_call_with_adaptive_timeout(cli, id, method, params, cli.rpc_timeout())
}

fn rpc_call_with_adaptive_timeout(
    cli: &Cli,
    id: u64,
    method: &str,
    params: Option<serde_json::Value>,
    adaptive_timeout: Duration,
) -> io::Result<rns_rpc::RpcResponse> {
    let frame = build_rpc_frame(id, method, params)?;
    let write_timeout = cli.rpc_timeout().max(adaptive_timeout);
    let read_timeout = write_timeout + RPC_READ_HEADROOM;
    #[cfg(unix)]
    if let Some(path) = cli.rpc_unix.as_ref() {
        let request = build_http_post("/rpc", "localhost", &frame);
        let stream = connect_unix_with_timeout(path, write_timeout)?;
        return rpc_call_with_stream(stream, &request, write_timeout, read_timeout);
    }

    let rpc = cli.rpc_addr();
    let request = build_http_post("/rpc", rpc, &frame);
    let addr = rpc.to_socket_addrs()?.next().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "RPC address did not resolve")
    })?;
    let stream = TcpStream::connect_timeout(&addr, write_timeout)?;
    rpc_call_with_stream(stream, &request, write_timeout, read_timeout)
}

#[cfg(unix)]
fn connect_unix_with_timeout(path: &Path, timeout: Duration) -> io::Result<UnixStream> {
    let socket = Socket::new(Domain::UNIX, Type::STREAM, None)?;
    socket.connect_timeout(&SockAddr::unix(path)?, timeout)?;
    Ok(socket.into())
}

trait RpcStream: Read + Write {
    fn shutdown_write(&self) -> io::Result<()>;
    fn set_rpc_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()>;
    fn set_rpc_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()>;
}

impl RpcStream for TcpStream {
    fn shutdown_write(&self) -> io::Result<()> {
        self.shutdown(Shutdown::Write)
    }

    fn set_rpc_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.set_read_timeout(timeout)
    }

    fn set_rpc_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.set_write_timeout(timeout)
    }
}

#[cfg(unix)]
impl RpcStream for UnixStream {
    fn shutdown_write(&self) -> io::Result<()> {
        self.shutdown(Shutdown::Write)
    }

    fn set_rpc_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.set_read_timeout(timeout)
    }

    fn set_rpc_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.set_write_timeout(timeout)
    }
}

fn rpc_call_with_stream<S: RpcStream>(
    mut stream: S,
    request: &[u8],
    write_timeout: Duration,
    read_timeout: Duration,
) -> io::Result<rns_rpc::RpcResponse> {
    stream.set_rpc_read_timeout(Some(read_timeout))?;
    stream.set_rpc_write_timeout(Some(write_timeout))?;
    stream.write_all(request)?;
    stream.shutdown_write()?;
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
        "on_iface",
        "tag_hex",
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

fn parse_request_tag_hex(value: &str) -> Result<String, String> {
    let bytes = hex::decode(value).map_err(|_| "tag must be hexadecimal")?;
    if bytes.is_empty() || bytes.len() > DESTINATION_HASH_BYTES {
        return Err(format!("tag must decode to 1..={DESTINATION_HASH_BYTES} bytes"));
    }
    Ok(value.to_ascii_lowercase())
}

impl Cli {
    fn rpc_addr(&self) -> &str {
        self.rpc.as_deref().unwrap_or(DEFAULT_RPC_ADDR)
    }

    fn rpc_timeout(&self) -> Duration {
        Duration::from_secs(self.timeout.max(1))
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    const DESTINATION_HASH: &str = "00112233445566778899aabbccddeeff";

    #[test]
    fn cli_defaults_to_standard_tcp_rpc_addr_at_runtime() {
        let cli = Cli::parse_from(["rnpath-rs", DESTINATION_HASH]);

        assert_eq!(cli.rpc_addr(), DEFAULT_RPC_ADDR);
    }

    #[test]
    fn cli_uses_explicit_tcp_rpc_addr_when_supplied() {
        let cli = Cli::parse_from(["rnpath-rs", DESTINATION_HASH, "--rpc", "127.0.0.1:4444"]);

        assert_eq!(cli.rpc_addr(), "127.0.0.1:4444");
    }

    #[test]
    fn cli_adds_read_headroom_beyond_path_timeout() {
        let cli = Cli::parse_from(["rnpath-rs", DESTINATION_HASH, "--timeout", "1"]);

        assert_eq!(cli.rpc_timeout(), Duration::from_secs(1));
        assert_eq!(cli.rpc_timeout() + RPC_READ_HEADROOM, Duration::from_secs(3));
    }

    #[test]
    fn rns_1_5_adaptive_timeout_uses_medium_timeout_as_a_lower_bound() {
        assert_eq!(adaptive_timeout(30, 120.1), 121);
        assert_eq!(adaptive_timeout(300, 120.1), 300);
        assert_eq!(adaptive_timeout(0, -1.0), 1);
    }
}
