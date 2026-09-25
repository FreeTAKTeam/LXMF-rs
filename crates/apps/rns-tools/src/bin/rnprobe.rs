use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(unix)]
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{ArgAction, CommandFactory, Parser};
use rns_rpc::e2e_harness::{build_http_post, build_rpc_frame, parse_http_response_body};
use serde_json::{json, Value};
#[cfg(unix)]
use socket2::{Domain, SockAddr, Socket, Type};

const DEFAULT_RPC_ADDR: &str = "127.0.0.1:4243";
const DEFAULT_SIZE: usize = 16;
const DEFAULT_PROBES: usize = 1;
const DEFAULT_TIMEOUT_SECS: f64 = 12.0;
const DEFAULT_WAIT_SECS: f64 = 0.0;
const MAX_PROBES: usize = 1_024;
const MAX_TIMEOUT_SECS: f64 = 3_600.0;
const RPC_READ_HEADROOM: Duration = Duration::from_secs(2);

#[derive(Debug, Parser)]
#[command(
    name = "rnprobe",
    about = "Send packet probes to a Reticulum destination through daemon RPC."
)]
struct Cli {
    #[arg(value_name = "FULL_NAME")]
    full_name: Option<String>,

    #[arg(value_name = "DESTINATION_HASH", value_parser = parse_destination_hash)]
    destination_hash: Option<String>,

    #[arg(long, value_name = "ADDR", help = "Daemon TCP RPC address (default: 127.0.0.1:4243)")]
    rpc: Option<String>,

    #[cfg(unix)]
    #[arg(long, value_name = "PATH", conflicts_with = "rpc")]
    rpc_unix: Option<PathBuf>,

    #[arg(short = 's', long, default_value_t = DEFAULT_SIZE)]
    size: usize,

    #[arg(short = 'n', long, default_value_t = DEFAULT_PROBES)]
    probes: usize,

    #[arg(short = 't', long, default_value_t = DEFAULT_TIMEOUT_SECS)]
    timeout: f64,

    #[arg(short = 'w', long, default_value_t = DEFAULT_WAIT_SECS)]
    wait: f64,

    #[arg(short = 'v', long, action = ArgAction::Count)]
    verbose: u8,

    #[arg(long)]
    json: bool,
}

#[derive(Debug)]
enum ProbeOutcome {
    Delivered,
    PacketLoss,
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    if cli.destination_hash.is_none() {
        let mut command = Cli::command();
        let _ = command.print_help();
        println!();
        return std::process::ExitCode::SUCCESS;
    }

    match run(&cli, &mut io::stdout()) {
        Ok(ProbeOutcome::Delivered) => std::process::ExitCode::SUCCESS,
        Ok(ProbeOutcome::PacketLoss) => std::process::ExitCode::from(2),
        Err(error) => {
            eprintln!("rnprobe: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli, output: &mut dyn Write) -> io::Result<ProbeOutcome> {
    let full_name = cli
        .full_name
        .as_deref()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "full_name is required"))?;
    let destination_hash = cli.destination_hash.as_deref().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "destination hash is required")
    })?;
    validate_options(cli)?;

    let params = json!({
        "full_name": full_name,
        "destination": destination_hash,
        "size": cli.size,
        "probes": cli.probes,
        "timeout_secs": cli.timeout,
        "wait_secs": cli.wait,
    });
    let response = rpc_call(cli, 1, "probe", Some(params))?;
    let result = ensure_rpc_ok(response, "probe")?
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing probe result"))?;
    let replies = result.get("replies").and_then(Value::as_u64).unwrap_or(0);
    let probes = result.get("probes").and_then(Value::as_u64).unwrap_or(cli.probes as u64);
    if cli.json {
        writeln!(output, "{}", serde_json::to_string_pretty(&result)?)?;
    } else {
        write_human_result(output, &result, cli.verbose)?;
    }
    Ok(if replies == probes { ProbeOutcome::Delivered } else { ProbeOutcome::PacketLoss })
}

fn validate_options(cli: &Cli) -> io::Result<()> {
    if cli.probes == 0 || cli.probes > MAX_PROBES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("probes must be between 1 and {MAX_PROBES}"),
        ));
    }
    validate_seconds(cli.timeout, "timeout", false)?;
    validate_seconds(cli.wait, "wait", true)?;
    Ok(())
}

fn validate_seconds(seconds: f64, field: &str, allow_zero: bool) -> io::Result<()> {
    let valid = seconds.is_finite()
        && if allow_zero { seconds >= 0.0 } else { seconds > 0.0 }
        && seconds <= MAX_TIMEOUT_SECS;
    if valid {
        return Ok(());
    }
    let lower_bound = if allow_zero { "non-negative" } else { "positive" };
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("{field} must be a finite {lower_bound} number no greater than {MAX_TIMEOUT_SECS}"),
    ))
}

fn expected_rpc_duration(cli: &Cli) -> io::Result<Duration> {
    let timeout = Duration::from_secs_f64(cli.timeout);
    let wait = Duration::from_secs_f64(cli.wait);
    timeout
        .checked_mul((cli.probes + 1) as u32)
        .and_then(|duration| duration.checked_add(wait.checked_mul((cli.probes - 1) as u32)?))
        .and_then(|duration| duration.checked_add(RPC_READ_HEADROOM))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "probe duration is too large"))
}

fn rpc_call(
    cli: &Cli,
    id: u64,
    method: &str,
    params: Option<Value>,
) -> io::Result<rns_rpc::RpcResponse> {
    let frame = build_rpc_frame(id, method, params)?;
    let read_timeout = expected_rpc_duration(cli)?;
    let write_timeout = Duration::from_secs(5);
    #[cfg(unix)]
    if let Some(path) = cli.rpc_unix.as_ref() {
        let request = build_http_post("/rpc", "localhost", &frame);
        let stream = connect_unix_with_timeout(path, write_timeout)?;
        return rpc_call_with_stream(stream, &request, write_timeout, read_timeout);
    }

    let rpc = cli.rpc.as_deref().unwrap_or(DEFAULT_RPC_ADDR);
    let request = build_http_post("/rpc", rpc, &frame);
    let address = rpc.to_socket_addrs()?.next().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "RPC address did not resolve")
    })?;
    let stream = TcpStream::connect_timeout(&address, write_timeout)?;
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

fn ensure_rpc_ok(response: rns_rpc::RpcResponse, method: &str) -> io::Result<Option<Value>> {
    if let Some(error) = response.error {
        return Err(io::Error::other(format!(
            "{method} failed: {} ({})",
            error.message, error.code
        )));
    }
    Ok(response.result)
}

fn write_human_result(output: &mut dyn Write, result: &Value, verbose: u8) -> io::Result<()> {
    let full_name = result.get("full_name").and_then(Value::as_str).unwrap_or("unknown");
    let destination = result.get("destination").and_then(Value::as_str).unwrap_or("unknown");
    let size = result.get("size").and_then(Value::as_u64).unwrap_or(0);
    let probes = result.get("probes").and_then(Value::as_u64).unwrap_or(0);
    let replies = result.get("replies").and_then(Value::as_u64).unwrap_or(0);
    let loss = result.get("packet_loss_percent").and_then(Value::as_f64).unwrap_or(0.0);

    writeln!(output, "Probe: {full_name}")?;
    writeln!(output, "Destination: {destination}")?;
    writeln!(output, "Size: {size} bytes, probes: {probes}")?;
    if let Some(results) = result.get("results").and_then(Value::as_array) {
        for probe in results {
            let number = probe.get("probe").and_then(Value::as_u64).unwrap_or(0);
            let status = probe.get("status").and_then(Value::as_str).unwrap_or("unknown");
            let rtt = probe.get("rtt_ms").and_then(Value::as_f64).map(format_rtt);
            let hops = probe.get("hops").and_then(Value::as_u64);
            match (rtt, hops) {
                (Some(rtt), Some(hops)) => {
                    writeln!(output, "{number}: {status}, {rtt}, hops={hops}")?
                }
                (Some(rtt), None) => writeln!(output, "{number}: {status}, {rtt}")?,
                (None, Some(hops)) => writeln!(output, "{number}: {status}, hops={hops}")?,
                (None, None) => writeln!(output, "{number}: {status}")?,
            }
            if verbose > 0 {
                if let Some(packet_hash) = probe.get("packet_hash").and_then(Value::as_str) {
                    writeln!(output, "  packet_hash={packet_hash}")?;
                }
            }
        }
    }
    writeln!(output, "Replies: {replies}/{probes}, packet loss: {loss:.1}%")?;
    Ok(())
}

fn format_rtt(milliseconds: f64) -> String {
    if milliseconds < 1_000.0 {
        format!("{milliseconds:.3} milliseconds")
    } else {
        format!("{:.3} seconds", milliseconds / 1_000.0)
    }
}

fn parse_destination_hash(value: &str) -> Result<String, String> {
    if value.len() != 32 {
        return Err("destination hash must be 32 hexadecimal characters".to_string());
    }
    hex::decode(value).map_err(|_| "destination hash must be hexadecimal".to_string())?;
    Ok(value.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_defaults_match_python_probe_defaults() {
        let cli =
            Cli::parse_from(["rnprobe", "rnstransport.probe", "00112233445566778899aabbccddeeff"]);

        assert_eq!(cli.size, DEFAULT_SIZE);
        assert_eq!(cli.probes, DEFAULT_PROBES);
        assert_eq!(cli.timeout, DEFAULT_TIMEOUT_SECS);
        assert_eq!(cli.wait, DEFAULT_WAIT_SECS);
        assert_eq!(cli.rpc, None);
    }

    #[test]
    fn cli_accepts_unix_rpc_and_probe_options() {
        let cli = Cli::parse_from([
            "rnprobe",
            "rnstransport.probe",
            "00112233445566778899aabbccddeeff",
            "--rpc-unix",
            "/tmp/reticulum.sock",
            "--size",
            "32",
            "--probes",
            "3",
            "--timeout",
            "1.5",
            "--wait",
            "0.25",
            "-vv",
        ]);

        assert_eq!(cli.rpc_unix.as_deref(), Some(Path::new("/tmp/reticulum.sock")));
        assert_eq!(cli.size, 32);
        assert_eq!(cli.probes, 3);
        assert_eq!(cli.timeout, 1.5);
        assert_eq!(cli.wait, 0.25);
        assert_eq!(cli.verbose, 2);
    }

    #[test]
    fn invalid_probe_options_are_rejected_before_rpc() {
        let cli = Cli::parse_from([
            "rnprobe",
            "rnstransport.probe",
            "00112233445566778899aabbccddeeff",
            "--probes",
            "0",
        ]);

        assert!(validate_options(&cli).is_err());
    }

    #[test]
    fn rtt_uses_milliseconds_below_one_second() {
        assert_eq!(format_rtt(12.3456), "12.346 milliseconds");
        assert_eq!(format_rtt(1_000.0), "1.000 seconds");
    }
}
