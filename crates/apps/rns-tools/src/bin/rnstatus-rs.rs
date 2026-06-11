use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};

use clap::Parser;
use rns_rpc::e2e_harness::{build_http_post, build_rpc_frame, parse_http_response_body};
use serde_json::Value;

#[derive(Debug, Parser)]
#[command(name = "rnstatus-rs")]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:4243")]
    rpc: String,

    #[arg(long)]
    json: bool,
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match run(&cli, &mut io::stdout()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rnstatus-rs: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli, output: &mut dyn Write) -> io::Result<()> {
    let response = rpc_call(&cli.rpc, 1, "daemon_status_ex")?;
    let status = ensure_rpc_ok(response, "daemon_status_ex")?
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing daemon status"))?;
    if cli.json {
        writeln!(output, "{}", serde_json::to_string_pretty(&status)?)?;
    } else {
        write_human_status(output, &status)?;
    }
    Ok(())
}

fn rpc_call(rpc: &str, id: u64, method: &str) -> io::Result<rns_rpc::RpcResponse> {
    let frame = build_rpc_frame(id, method, None)?;
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

fn write_human_status(output: &mut dyn Write, status: &Value) -> io::Result<()> {
    writeln!(
        output,
        "Identity: {}",
        status.get("identity_hash").and_then(Value::as_str).unwrap_or("unknown")
    )?;
    writeln!(
        output,
        "Running: {}",
        status.get("running").and_then(Value::as_bool).unwrap_or(false)
    )?;
    writeln!(
        output,
        "Interfaces: {}",
        status.get("interface_count").and_then(Value::as_u64).unwrap_or_else(|| {
            status.get("interfaces").and_then(Value::as_array).map_or(0, |rows| rows.len() as u64)
        })
    )?;
    write_propagation_status(output, status)?;

    let Some(interfaces) = status.get("interfaces").and_then(Value::as_array) else {
        return Ok(());
    };
    if interfaces.is_empty() {
        return Ok(());
    }

    writeln!(output)?;
    writeln!(output, "{:<24} {:<16} {:<8} {:<22} Runtime", "Name", "Type", "Enabled", "Endpoint")?;
    for interface in interfaces {
        let name = interface.get("name").and_then(Value::as_str).unwrap_or("-");
        let kind = interface.get("type").and_then(Value::as_str).unwrap_or("-");
        let enabled = interface
            .get("enabled")
            .and_then(Value::as_bool)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_string());
        let endpoint = interface_endpoint(interface);
        let runtime = interface_runtime(interface);
        writeln!(output, "{name:<24} {kind:<16} {enabled:<8} {endpoint:<22} {runtime}")?;
    }
    Ok(())
}

fn write_propagation_status(output: &mut dyn Write, status: &Value) -> io::Result<()> {
    let Some(propagation) = status.get("propagation") else {
        return Ok(());
    };
    let enabled = value_bool(propagation, "enabled");
    let peers = status
        .get("peer_count")
        .and_then(Value::as_u64)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string());
    let selected = value_str(propagation, "selected_node");
    let sync = value_u64(propagation, "sync_state");
    let progress = propagation
        .get("sync_progress")
        .and_then(Value::as_f64)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string());
    let target_cost = value_u64(propagation, "target_cost");
    let static_only = value_bool(propagation, "from_static_only");
    writeln!(
        output,
        "Propagation: enabled={enabled} peers={peers} selected={selected} sync={sync} progress={progress} target_cost={target_cost} static_only={static_only}"
    )
}

fn value_bool(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_bool)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn value_u64(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_u64)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn value_str(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("-")
        .to_string()
}

fn interface_endpoint(interface: &Value) -> String {
    match (
        interface.get("host").and_then(Value::as_str),
        interface.get("port").and_then(Value::as_u64),
    ) {
        (Some(host), Some(port)) => format!("{host}:{port}"),
        (Some(host), None) => host.to_string(),
        (None, Some(port)) => port.to_string(),
        (None, None) => "-".to_string(),
    }
}

fn interface_runtime(interface: &Value) -> String {
    let runtime = interface
        .get("settings")
        .and_then(|settings| settings.get("_runtime"))
        .unwrap_or(&Value::Null);
    let status = runtime.get("startup_status").and_then(Value::as_str).unwrap_or("-");
    let error = runtime.get("startup_error").and_then(Value::as_str);
    match error {
        Some(error) if !error.is_empty() => format!("{status} ({error})"),
        _ => status.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn human_status_includes_runtime_state() {
        let status = json!({
            "identity_hash": "abc",
            "running": true,
            "interface_count": 1,
            "interfaces": [{
                "name": "uplink",
                "type": "tcp_server",
                "enabled": true,
                "host": "127.0.0.1",
                "port": 4242,
                "settings": {
                    "_runtime": {
                        "startup_status": "failed",
                        "startup_error": "bind denied"
                    }
                }
            }]
        });
        let mut output = Vec::new();

        write_human_status(&mut output, &status).expect("write status");

        let output = String::from_utf8(output).expect("utf8");
        assert!(output.contains("uplink"));
        assert!(output.contains("tcp_server"));
        assert!(output.contains("failed (bind denied)"));
    }

    #[test]
    fn human_status_includes_propagation_peer_summary() {
        let status = json!({
            "identity_hash": "abc",
            "running": true,
            "interface_count": 0,
            "peer_count": 3,
            "propagation": {
                "enabled": true,
                "selected_node": "feedface",
                "sync_state": 255,
                "sync_progress": 0.5,
                "target_cost": 12,
                "from_static_only": true
            },
            "interfaces": []
        });
        let mut output = Vec::new();

        write_human_status(&mut output, &status).expect("write status");

        let output = String::from_utf8(output).expect("utf8");
        assert!(output.contains("Propagation: enabled=true"));
        assert!(output.contains("peers=3"));
        assert!(output.contains("selected=feedface"));
        assert!(output.contains("sync=255"));
        assert!(output.contains("progress=0.5"));
        assert!(output.contains("target_cost=12"));
        assert!(output.contains("static_only=true"));
    }
}
