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
    let mut parts = vec![match error {
        Some(error) if !error.is_empty() => format!("{status} ({error})"),
        _ => status.to_string(),
    }];
    if let Some(summary) = runtime
        .get("i2p")
        .and_then(|value| value.get("tunnel_status"))
        .and_then(i2p_runtime_summary)
    {
        parts.push(summary);
    }
    if let Some(summary) =
        runtime.get("pipe").and_then(|value| value.get("status")).and_then(pipe_runtime_summary)
    {
        parts.push(summary);
    }
    if let Some(summary) =
        runtime.get("weave").and_then(|value| value.get("status")).and_then(weave_runtime_summary)
    {
        parts.push(summary);
    }
    if let Some(summary) = runtime
        .get("lora")
        .and_then(|value| value.get("rnode_status"))
        .and_then(lora_rnode_runtime_summary)
    {
        parts.push(summary);
    }
    if let Some(summary) = runtime
        .get("rnode_multi")
        .and_then(|value| value.get("radio_status"))
        .and_then(rnode_multi_runtime_summary)
    {
        parts.push(summary);
    }
    parts.join("; ")
}

fn i2p_runtime_summary(status: &Value) -> Option<String> {
    if !status.is_object() {
        return None;
    }
    let peers = status.get("peers").and_then(Value::as_array);
    let peer_count = peers
        .map_or_else(|| value_u64(status, "configured_peer_count"), |rows| rows.len().to_string());
    let connected = count_rows_with_str(peers, "state", "connected");
    let stale = count_rows_with_str(peers, "state", "stale");
    let reconnecting = count_rows_with_str(peers, "state", "reconnecting");
    let mut summary = format!(
        "i2p sam={} accept={} peers={peer_count}",
        value_str(status, "sam_endpoint"),
        value_str(status, "accept_state")
    );
    append_count(&mut summary, "connected", connected);
    append_count(&mut summary, "stale", stale);
    append_count(&mut summary, "reconnecting", reconnecting);
    append_optional_str(&mut summary, "err", status.get("last_accept_error"));
    Some(summary)
}

fn pipe_runtime_summary(status: &Value) -> Option<String> {
    if !status.is_object() {
        return None;
    }
    let mut summary = format!(
        "pipe state={} open={} respawns={}",
        value_str(status, "process_state"),
        value_bool(status, "pipe_is_open"),
        value_u64(status, "respawn_attempts")
    );
    append_optional_str(&mut summary, "err", status.get("last_error"));
    Some(summary)
}

fn weave_runtime_summary(status: &Value) -> Option<String> {
    if !status.is_object() {
        return None;
    }
    let mut summary = format!(
        "weave link={} endpoints={} wdcl={}",
        value_str(status, "link_state"),
        value_u64(status, "endpoint_count"),
        value_bool(status, "wdcl_connected")
    );
    append_optional_u64(&mut summary, "rx", status.get("bytes_rx"));
    append_optional_u64(&mut summary, "tx", status.get("bytes_tx"));
    if let Some(display) = status.get("display").filter(|display| display.is_object()) {
        let width = value_u64(display, "width");
        let height = value_u64(display, "height");
        let complete = value_bool(display, "complete");
        summary.push_str(&format!(" display={width}x{height}/{complete}"));
    }
    if let Some(stats) = status.get("device_stats").filter(|stats| stats.is_object()) {
        append_optional_u64(&mut summary, "cpu", stats.get("cpu_load"));
        if let Some(percent) = stats
            .get("memory_used_percent_bp")
            .and_then(Value::as_u64)
            .map(format_basis_points_percent)
        {
            summary.push_str(&format!(" mem={percent}"));
        }
    }
    append_optional_str(&mut summary, "err", status.get("last_error"));
    Some(summary)
}

fn rnode_multi_runtime_summary(status: &Value) -> Option<String> {
    if !status.is_object() {
        return None;
    }
    let vports = status.get("vports").and_then(Value::as_array).map_or(0, Vec::len);
    let mut summary = format!(
        "rnode_multi stream={} selected={} vports={vports}",
        value_str(status, "stream_state"),
        value_u64(status, "selected_vport")
    );
    append_optional_str(&mut summary, "err", status.get("last_error"));
    Some(summary)
}

fn lora_rnode_runtime_summary(status: &Value) -> Option<String> {
    if !status.is_object() {
        return None;
    }
    let probe = status.get("probe_status").filter(|value| value.is_object());
    let radio = status.get("radio_status").filter(|value| value.is_object());
    let mut summary = format!(
        "rnode bearer={} online={} detected={}",
        value_str(status, "bearer"),
        value_bool(status, "online"),
        probe.map_or_else(|| "?".to_string(), |probe| value_bool(probe, "detected"))
    );
    if let Some(probe) = probe {
        if let Some(firmware) = probe
            .get("firmware_version")
            .and_then(|value| value.get("label"))
            .and_then(Value::as_str)
        {
            summary.push_str(&format!(" fw={firmware}"));
        }
    }
    if let Some(radio) = radio {
        append_optional_u64(&mut summary, "freq", radio.get("frequency_hz"));
        append_optional_u64(&mut summary, "bw", radio.get("bandwidth_hz"));
        append_optional_u64(&mut summary, "sf", radio.get("spreading_factor"));
        append_optional_u64(&mut summary, "cr", radio.get("coding_rate"));
        append_optional_u64(&mut summary, "txp", radio.get("tx_power_dbm"));
        append_optional_u64(&mut summary, "rx", radio.get("stat_rx"));
        append_optional_u64(&mut summary, "tx", radio.get("stat_tx"));
        append_optional_u64(&mut summary, "bat", radio.get("battery_percent"));
    }
    if let Some(errors) = status.get("hardware_errors").and_then(Value::as_array) {
        append_count(&mut summary, "hwerr", errors.len());
    }
    append_optional_str(&mut summary, "err", status.get("last_command_error"));
    Some(summary)
}

fn count_rows_with_str(rows: Option<&Vec<Value>>, key: &str, expected: &str) -> usize {
    rows.map_or(0, |rows| {
        rows.iter().filter(|row| row.get(key).and_then(Value::as_str) == Some(expected)).count()
    })
}

fn append_count(summary: &mut String, label: &str, value: usize) {
    if value > 0 {
        summary.push_str(&format!(" {label}={value}"));
    }
}

fn append_optional_u64(summary: &mut String, label: &str, value: Option<&Value>) {
    if let Some(value) = value.and_then(Value::as_u64) {
        summary.push_str(&format!(" {label}={value}"));
    }
}

fn append_optional_str(summary: &mut String, label: &str, value: Option<&Value>) {
    if let Some(value) = value.and_then(Value::as_str).filter(|value| !value.is_empty()) {
        summary.push_str(&format!(" {label}={value}"));
    }
}

fn format_basis_points_percent(value: u64) -> String {
    format!("{}.{:02}%", value / 100, value % 100)
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
    fn human_status_includes_interface_runtime_detail() {
        let status = json!({
            "identity_hash": "abc",
            "running": true,
            "interface_count": 4,
            "interfaces": [
                {
                    "name": "i2p-main",
                    "type": "i2p",
                    "enabled": true,
                    "settings": {
                        "_runtime": {
                            "startup_status": "spawned",
                            "i2p": {
                                "tunnel_status": {
                                    "sam_endpoint": "127.0.0.1:7656",
                                    "accept_state": "listening",
                                    "configured_peer_count": 2,
                                    "last_accept_error": null,
                                    "peers": [
                                        { "state": "connected" },
                                        { "state": "stale" }
                                    ]
                                }
                            }
                        }
                    }
                },
                {
                    "name": "weave-main",
                    "type": "weave",
                    "enabled": true,
                    "settings": {
                        "_runtime": {
                            "startup_status": "spawned",
                            "weave": {
                                "status": {
                                    "link_state": "connected",
                                    "endpoint_count": 2,
                                    "wdcl_connected": true,
                                    "bytes_rx": 120,
                                    "bytes_tx": 80,
                                    "display": {
                                        "width": 128,
                                        "height": 64,
                                        "complete": true
                                    },
                                    "device_stats": {
                                        "cpu_load": 42,
                                        "memory_used_percent_bp": 5125
                                    }
                                }
                            }
                        }
                    }
                },
                {
                    "name": "rnode-main",
                    "type": "lora",
                    "enabled": true,
                    "settings": {
                        "_runtime": {
                            "startup_status": "spawned",
                            "lora": {
                                "rnode_status": {
                                    "bearer": "serial",
                                    "online": true,
                                    "probe_status": {
                                        "detected": true,
                                        "firmware_version": { "label": "1.52" }
                                    },
                                    "radio_status": {
                                        "frequency_hz": 915000000,
                                        "bandwidth_hz": 125000,
                                        "spreading_factor": 9,
                                        "coding_rate": 5,
                                        "tx_power_dbm": 17,
                                        "stat_rx": 3,
                                        "stat_tx": 4,
                                        "battery_percent": 88
                                    },
                                    "hardware_errors": [],
                                    "last_command_error": null
                                }
                            }
                        }
                    }
                },
                {
                    "name": "rnode-multi",
                    "type": "rnode_multi",
                    "enabled": true,
                    "settings": {
                        "_runtime": {
                            "startup_status": "spawned",
                            "rnode_multi": {
                                "radio_status": {
                                    "stream_state": "running",
                                    "selected_vport": 2,
                                    "last_error": null,
                                    "vports": [2, 3]
                                }
                            }
                        }
                    }
                },
                {
                    "name": "pipe-main",
                    "type": "pipe",
                    "enabled": true,
                    "settings": {
                        "_runtime": {
                            "startup_status": "spawned",
                            "pipe": {
                                "status": {
                                    "command": "cat",
                                    "process_state": "respawning",
                                    "pipe_is_open": false,
                                    "respawn_attempts": 2,
                                    "last_error": "spawn cat failed"
                                }
                            }
                        }
                    }
                }
            ]
        });
        let mut output = Vec::new();

        write_human_status(&mut output, &status).expect("write status");

        let output = String::from_utf8(output).expect("utf8");
        assert!(output.contains("i2p sam=127.0.0.1:7656 accept=listening peers=2"));
        assert!(output.contains("connected=1"));
        assert!(output.contains("stale=1"));
        assert!(output.contains("weave link=connected endpoints=2 wdcl=true"));
        assert!(output.contains("display=128x64/true"));
        assert!(output.contains("cpu=42"));
        assert!(output.contains("mem=51.25%"));
        assert!(output.contains("rnode bearer=serial online=true detected=true"));
        assert!(output.contains("fw=1.52"));
        assert!(output.contains("freq=915000000"));
        assert!(output.contains("bat=88"));
        assert!(output.contains("rnode_multi stream=running selected=2 vports=2"));
        assert!(output.contains("pipe state=respawning open=false respawns=2"));
        assert!(output.contains("err=spawn cat failed"));
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
