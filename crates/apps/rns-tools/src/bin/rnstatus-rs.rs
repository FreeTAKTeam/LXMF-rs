#![recursion_limit = "256"]

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};

use clap::Parser;
use rns_rpc::e2e_harness::{build_http_post, build_rpc_frame, parse_http_response_body};
use serde_json::{json, Value};

#[derive(Debug, Parser)]
#[command(name = "rnstatus-rs")]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:4243")]
    rpc: String,

    #[arg(long)]
    json: bool,

    #[arg(long, value_name = "INTERFACE")]
    weave_display: Option<String>,
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
    if let Some(interface_name) = cli.weave_display.as_deref() {
        let weave_status = find_weave_status(&status, interface_name)?;
        if cli.json {
            writeln!(
                output,
                "{}",
                serde_json::to_string_pretty(&weave_display_report(interface_name, weave_status))?
            )?;
        } else {
            write_weave_display_view(output, interface_name, weave_status)?;
        }
    } else if cli.json {
        writeln!(output, "{}", serde_json::to_string_pretty(&status)?)?;
    } else {
        write_human_status(output, &status)?;
    }
    Ok(())
}

fn find_weave_status<'a>(status: &'a Value, interface_name: &str) -> io::Result<&'a Value> {
    let Some(interface) =
        status.get("interfaces").and_then(Value::as_array).and_then(|interfaces| {
            interfaces.iter().find(|interface| {
                interface.get("name").and_then(Value::as_str) == Some(interface_name)
            })
        })
    else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("interface {interface_name:?} not found"),
        ));
    };

    interface
        .get("settings")
        .and_then(|settings| settings.get("_runtime"))
        .and_then(|runtime| runtime.get("weave"))
        .and_then(|weave| weave.get("status"))
        .filter(|status| status.is_object())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("interface {interface_name:?} has no Weave runtime display status"),
            )
        })
}

fn weave_display_report(interface_name: &str, status: &Value) -> Value {
    json!({
        "interface": interface_name,
        "link_state": status.get("link_state").cloned().unwrap_or(Value::Null),
        "wdcl_connected": status.get("wdcl_connected").cloned().unwrap_or(Value::Null),
        "remote_switch_id": status.get("remote_switch_id").cloned().unwrap_or(Value::Null),
        "display": status.get("display").cloned().unwrap_or(Value::Null),
        "device_stats": status.get("device_stats").cloned().unwrap_or(Value::Null),
        "last_error": status.get("last_error").cloned().unwrap_or(Value::Null),
    })
}

fn write_weave_display_view(
    output: &mut dyn Write,
    interface_name: &str,
    status: &Value,
) -> io::Result<()> {
    writeln!(output, "Weave Display: {interface_name}")?;
    writeln!(
        output,
        "link={} wdcl={} remote={}",
        value_str(status, "link_state"),
        value_bool(status, "wdcl_connected"),
        value_str(status, "remote_switch_id")
    )?;
    if let Some(display) = status.get("display").filter(|display| display.is_object()) {
        writeln!(
            output,
            "size={}x{} complete={} color={} bytes={}/{}",
            value_u64(display, "width"),
            value_u64(display, "height"),
            value_bool(display, "complete"),
            value_u64(display, "color_format"),
            value_u64(display, "received_size"),
            value_u64(display, "total_size")
        )?;
        if let Some(buffer_hex) = display.get("buffer_hex").and_then(Value::as_str) {
            writeln!(output, "buffer_hex={buffer_hex}")?;
        }
    } else {
        writeln!(output, "display=unavailable")?;
    }
    if let Some(stats) = status.get("device_stats").filter(|stats| stats.is_object()) {
        let mut summary = String::from("stats");
        append_optional_u64(&mut summary, "cpu", stats.get("cpu_load"));
        if let Some(percent) = stats
            .get("memory_used_percent_bp")
            .and_then(Value::as_u64)
            .map(format_basis_points_percent)
        {
            summary.push_str(&format!(" mem={percent}"));
        }
        if let Some(task_count) =
            stats.get("task_cpu").and_then(Value::as_object).map(serde_json::Map::len)
        {
            append_count(&mut summary, "tasks", task_count);
        }
        writeln!(output, "{summary}")?;
    }
    append_optional_str_line(output, "err", status.get("last_error"))
}

fn append_optional_str_line(
    output: &mut dyn Write,
    label: &str,
    value: Option<&Value>,
) -> io::Result<()> {
    if let Some(value) = value.and_then(Value::as_str).filter(|value| !value.is_empty()) {
        writeln!(output, "{label}={value}")?;
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
    let settings = interface.get("settings").unwrap_or(&Value::Null);
    if let Some(endpoint) = host_port_endpoint(
        interface
            .get("host")
            .and_then(Value::as_str)
            .or_else(|| settings.get("host").and_then(Value::as_str)),
        interface
            .get("port")
            .and_then(Value::as_u64)
            .or_else(|| settings.get("port").and_then(Value::as_u64)),
    ) {
        if let Some(target) = host_port_endpoint(
            settings.get("target_host").and_then(Value::as_str),
            settings.get("target_port").and_then(Value::as_u64),
        ) {
            return format!("{endpoint}->{target}");
        }
        return endpoint;
    }

    if let Some(socket_path) = settings.get("socket_path").and_then(Value::as_str) {
        return socket_path.to_string();
    }
    if let Some(device) = settings.get("device").and_then(Value::as_str) {
        return device.to_string();
    }
    if let Some(peripheral_id) = settings.get("peripheral_id").and_then(Value::as_str) {
        return format!("ble:{peripheral_id}");
    }
    if let Some(command) = settings.get("command").and_then(Value::as_str) {
        return command.to_string();
    }
    if let Some(sam) = host_port_endpoint(
        settings.get("sam_host").and_then(Value::as_str),
        settings.get("sam_port").and_then(Value::as_u64),
    ) {
        return format!("sam:{sam}");
    }
    if let Some(peers) = settings.get("peers").and_then(Value::as_array) {
        return format!("peers:{}", peers.len());
    }
    if let Some(group) = settings.get("group_id").and_then(Value::as_str) {
        return format!("group:{group}");
    }
    "-".to_string()
}

fn host_port_endpoint(host: Option<&str>, port: Option<u64>) -> Option<String> {
    let host = host.map(str::trim).filter(|value| !value.is_empty());
    match (host, port) {
        (Some(host), Some(port)) => Some(format!("{host}:{port}")),
        (Some(host), None) => Some(host.to_string()),
        (None, Some(port)) => Some(port.to_string()),
        (None, None) => None,
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
        .get("auto")
        .and_then(|value| value.get("carrier_runtime"))
        .and_then(auto_runtime_summary)
    {
        parts.push(summary);
    }
    if let Some(summary) = runtime
        .get("i2p")
        .and_then(|value| value.get("tunnel_status"))
        .and_then(i2p_runtime_summary)
    {
        parts.push(summary);
    }
    if let Some(summary) = runtime
        .get("tcp")
        .and_then(|value| value.get("stream_status"))
        .and_then(tcp_runtime_summary)
    {
        parts.push(summary);
    }
    if let Some(summary) = runtime
        .get("tcp")
        .and_then(|value| value.get("listener_status"))
        .and_then(tcp_listener_runtime_summary)
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
    if let Some(summary) =
        runtime.get("vrn76").and_then(|value| value.get("status")).and_then(vrn76_runtime_summary)
    {
        parts.push(summary);
    }
    if let Some(summary) =
        runtime.get("udp").and_then(|value| value.get("status")).and_then(udp_runtime_summary)
    {
        parts.push(summary);
    }
    if let Some(summary) =
        runtime.get("serial").and_then(|value| value.get("status")).and_then(serial_runtime_summary)
    {
        parts.push(summary);
    }
    if let Some(summary) =
        runtime.get("kiss").and_then(|value| value.get("status")).and_then(kiss_runtime_summary)
    {
        parts.push(summary);
    }
    if let Some(summary) =
        runtime.get("kiss_tcp").and_then(|value| value.get("status")).and_then(kiss_runtime_summary)
    {
        parts.push(summary);
    }
    if let Some(summary) = runtime
        .get("ble_gatt")
        .and_then(|value| value.get("status"))
        .and_then(ble_gatt_runtime_summary)
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
    let closed = count_rows_with_str(peers, "state", "closed");
    let outbound = count_rows_with_str(peers, "direction", "outbound");
    let incoming = count_rows_with_str(peers, "direction", "incoming");
    let bytes_rx = sum_rows_u64(peers, "bytes_rx");
    let bytes_tx = sum_rows_u64(peers, "bytes_tx");
    let mut summary = format!(
        "i2p sam={} accept={} peers={peer_count}",
        value_str(status, "sam_endpoint"),
        value_str(status, "accept_state")
    );
    append_count(&mut summary, "connected", connected);
    append_count(&mut summary, "stale", stale);
    append_count(&mut summary, "reconnecting", reconnecting);
    append_count(&mut summary, "closed", closed);
    append_count(&mut summary, "outbound", outbound);
    append_count(&mut summary, "incoming", incoming);
    append_nonzero_u64(&mut summary, "rx", bytes_rx);
    append_nonzero_u64(&mut summary, "tx", bytes_tx);
    append_optional_str(&mut summary, "err", status.get("last_accept_error"));
    Some(summary)
}

fn auto_runtime_summary(status: &Value) -> Option<String> {
    if !status.is_object() {
        return None;
    }
    let mut summary = format!(
        "auto online={} init={} carrier_changed={} carrier_events={}",
        value_bool(status, "online"),
        value_bool(status, "final_init_done"),
        value_bool(status, "carrier_changed"),
        value_u64(status, "carrier_event_count")
    );
    append_optional_u64(&mut summary, "adopted", status.get("adopted_device_count"));
    append_optional_u64(&mut summary, "added", status.get("adopted_add_count"));
    append_optional_u64(&mut summary, "removed", status.get("adopted_remove_count"));
    append_optional_u64(&mut summary, "replaced", status.get("link_local_replacement_count"));
    if let Some(link_local) = status.get("link_local_update").and_then(Value::as_object) {
        append_optional_str(&mut summary, "link_local", link_local.get("ifname"));
        append_optional_str(&mut summary, "new_ll", link_local.get("new_link_local_address"));
    }
    Some(summary)
}

fn tcp_runtime_summary(status: &Value) -> Option<String> {
    if !status.is_object() {
        return None;
    }
    let mut summary = format!(
        "tcp stream={} endpoint={} reconnects={}",
        value_str(status, "stream_state"),
        value_str(status, "endpoint"),
        value_u64(status, "reconnect_attempts")
    );
    append_optional_u64(&mut summary, "rx", status.get("bytes_rx"));
    append_optional_u64(&mut summary, "tx", status.get("bytes_tx"));
    append_optional_u64(&mut summary, "keepalives", status.get("keepalives_sent"));
    append_optional_u64(&mut summary, "stale", status.get("stale_events"));
    append_optional_u64(&mut summary, "timeouts", status.get("read_timeouts"));
    append_optional_u64(&mut summary, "closed", status.get("closed_events"));
    append_optional_u64(&mut summary, "errors", status.get("error_events"));
    if status.get("liveness_enabled").and_then(Value::as_bool) == Some(true) {
        summary.push_str(" liveness=true");
    }
    append_optional_u64(&mut summary, "bitrate", status.get("forced_bitrate_bps"));
    append_optional_str(&mut summary, "err", status.get("last_error"));
    Some(summary)
}

fn tcp_listener_runtime_summary(status: &Value) -> Option<String> {
    if !status.is_object() {
        return None;
    }
    let mut summary = format!(
        "tcp listener={} bind={} accepted={}",
        value_str(status, "listener_state"),
        value_str(status, "bind_addr"),
        value_u64(status, "accepted_connections")
    );
    append_optional_u64(&mut summary, "accept_errors", status.get("accept_errors"));
    if status.get("client_liveness_enabled").and_then(Value::as_bool) == Some(true) {
        summary.push_str(" child_liveness=true");
    }
    append_optional_u64(&mut summary, "child_bitrate", status.get("client_forced_bitrate_bps"));
    append_optional_str(&mut summary, "latest", status.get("latest_client_endpoint"));
    if let Some(latest_stream) = status.get("latest_stream_status").and_then(Value::as_object) {
        append_optional_str(&mut summary, "latest_state", latest_stream.get("stream_state"));
        append_optional_u64(&mut summary, "latest_rx", latest_stream.get("bytes_rx"));
        append_optional_u64(&mut summary, "latest_tx", latest_stream.get("bytes_tx"));
    }
    append_optional_str(&mut summary, "err", status.get("last_error"));
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
    append_optional_str(&mut summary, "remote", status.get("remote_switch_id"));
    append_optional_u64(&mut summary, "rx", status.get("bytes_rx"));
    append_optional_u64(&mut summary, "tx", status.get("bytes_tx"));
    append_optional_u64(&mut summary, "rx_frames", status.get("frames_rx"));
    append_optional_u64(&mut summary, "tx_frames", status.get("frames_tx"));
    append_optional_u64(&mut summary, "invalid_frames", status.get("invalid_frames"));
    append_optional_str(&mut summary, "last_log", status.get("last_log_event"));
    if let Some(display) = status.get("display").filter(|display| display.is_object()) {
        let width = value_u64(display, "width");
        let height = value_u64(display, "height");
        let complete = value_bool(display, "complete");
        summary.push_str(&format!(" display={width}x{height}/{complete}"));
        let received = value_u64(display, "received_size");
        let total = value_u64(display, "total_size");
        summary.push_str(&format!(" display_bytes={received}/{total}"));
        append_optional_u64(&mut summary, "color", display.get("color_format"));
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
        if let Some(task_count) =
            stats.get("task_cpu").and_then(Value::as_object).map(serde_json::Map::len)
        {
            append_count(&mut summary, "tasks", task_count);
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
    if let Some(probe) = status.get("startup_probe").filter(|value| value.is_object()) {
        append_optional_bool(&mut summary, "detected", probe.get("detected"));
        if let Some(firmware) = probe
            .get("firmware_version")
            .and_then(|value| value.get("label"))
            .and_then(Value::as_str)
        {
            summary.push_str(&format!(" fw={firmware}"));
        }
        append_optional_u64(&mut summary, "platform", probe.get("platform"));
        append_optional_u64(&mut summary, "mcu", probe.get("mcu"));
        append_optional_str(&mut summary, "probe", probe.get("interface_summary"));
    }
    append_optional_str(&mut summary, "err", status.get("last_error"));
    Some(summary)
}

fn vrn76_runtime_summary(status: &Value) -> Option<String> {
    if !status.is_object() {
        return None;
    }
    let mut summary = format!(
        "vrn76 connected={} subscribed={} ready={}",
        value_bool(status, "connected"),
        value_bool(status, "subscribed"),
        value_bool(status, "interface_ready")
    );
    append_optional_u64(
        &mut summary,
        "startup_write_failures",
        status.get("startup_write_failures"),
    );
    append_optional_u64(&mut summary, "pending_payloads", status.get("pending_payloads"));
    append_optional_u64(&mut summary, "pending_writes", status.get("pending_writes"));
    append_optional_u64(&mut summary, "pending_packets", status.get("pending_packets"));
    Some(summary)
}

fn udp_runtime_summary(status: &Value) -> Option<String> {
    if !status.is_object() {
        return None;
    }
    let mut summary = format!(
        "udp state={} role={} bind={}",
        value_str(status, "link_state"),
        value_str(status, "role"),
        value_str(status, "bind_addr")
    );
    append_optional_str(&mut summary, "forward", status.get("forward_addr"));
    append_optional_u64(&mut summary, "peers", status.get("peer_routes"));
    append_optional_u64(&mut summary, "rxp", status.get("packets_rx"));
    append_optional_u64(&mut summary, "txp", status.get("packets_tx"));
    append_optional_u64(&mut summary, "rx", status.get("bytes_rx"));
    append_optional_u64(&mut summary, "tx", status.get("bytes_tx"));
    append_optional_u64(&mut summary, "decode_errors", status.get("decode_errors"));
    append_optional_u64(&mut summary, "rx_queue_errors", status.get("rx_queue_errors"));
    append_optional_u64(&mut summary, "socket_errors", status.get("socket_errors"));
    append_optional_u64(&mut summary, "tx_errors", status.get("tx_errors"));
    append_optional_u64(&mut summary, "dropped_direct", status.get("dropped_direct"));
    append_optional_str(&mut summary, "err", status.get("last_error"));
    Some(summary)
}

fn serial_runtime_summary(status: &Value) -> Option<String> {
    if !status.is_object() {
        return None;
    }
    let mut summary = format!(
        "serial state={} device={} baud={}",
        value_str(status, "link_state"),
        value_str(status, "device"),
        value_u64(status, "baud_rate")
    );
    append_optional_u64(&mut summary, "data_bits", status.get("data_bits"));
    append_optional_str(&mut summary, "parity", status.get("parity"));
    append_optional_u64(&mut summary, "stop_bits", status.get("stop_bits"));
    append_optional_str(&mut summary, "flow", status.get("flow_control"));
    append_optional_u64(&mut summary, "mtu", status.get("mtu"));
    append_optional_u64(&mut summary, "reconnects", status.get("reconnect_attempts"));
    append_optional_u64(&mut summary, "open_errors", status.get("open_errors"));
    append_optional_u64(&mut summary, "rxp", status.get("packets_rx"));
    append_optional_u64(&mut summary, "txp", status.get("packets_tx"));
    append_optional_u64(&mut summary, "rx_frames", status.get("frames_rx"));
    append_optional_u64(&mut summary, "tx_frames", status.get("frames_tx"));
    append_optional_u64(&mut summary, "rx", status.get("bytes_rx"));
    append_optional_u64(&mut summary, "tx", status.get("bytes_tx"));
    append_optional_u64(&mut summary, "decode_errors", status.get("decode_errors"));
    append_optional_u64(&mut summary, "deserialize_errors", status.get("deserialize_errors"));
    append_optional_u64(&mut summary, "rx_queue_errors", status.get("rx_queue_errors"));
    append_optional_u64(&mut summary, "serialize_errors", status.get("serialize_errors"));
    append_optional_u64(&mut summary, "hdlc_encode_errors", status.get("hdlc_encode_errors"));
    append_optional_u64(&mut summary, "tx_errors", status.get("tx_errors"));
    append_optional_u64(&mut summary, "read_errors", status.get("read_errors"));
    append_optional_u64(&mut summary, "eof", status.get("eof_count"));
    append_optional_str(&mut summary, "err", status.get("last_error"));
    Some(summary)
}

fn kiss_runtime_summary(status: &Value) -> Option<String> {
    if !status.is_object() {
        return None;
    }
    let mut summary = format!(
        "kiss state={} bearer={}",
        value_str(status, "link_state"),
        value_str(status, "bearer")
    );
    append_optional_str(&mut summary, "device", status.get("device"));
    append_optional_str(&mut summary, "endpoint", status.get("endpoint"));
    append_optional_u64(&mut summary, "baud", status.get("baud_rate"));
    append_optional_u64(&mut summary, "mtu", status.get("mtu"));
    append_optional_u64(&mut summary, "preamble", status.get("preamble_ms"));
    append_optional_u64(&mut summary, "txtail", status.get("tx_tail_ms"));
    append_optional_bool(&mut summary, "flow", status.get("kiss_flow_control"));
    append_optional_bool(&mut summary, "ax25", status.get("ax25"));
    append_optional_str(&mut summary, "callsign", status.get("callsign"));
    append_optional_u64(&mut summary, "ssid", status.get("ssid"));
    append_optional_str(&mut summary, "id", status.get("id_callsign"));
    append_optional_u64(&mut summary, "id_interval", status.get("id_interval"));
    append_optional_bool(&mut summary, "ready", status.get("interface_ready"));
    append_optional_u64(&mut summary, "pending", status.get("pending_depth"));
    append_optional_u64(&mut summary, "reconnects", status.get("reconnect_attempts"));
    append_optional_u64(&mut summary, "open_errors", status.get("open_errors"));
    append_optional_u64(&mut summary, "connect_errors", status.get("connect_errors"));
    append_optional_u64(&mut summary, "rxp", status.get("packets_rx"));
    append_optional_u64(&mut summary, "txp", status.get("packets_tx"));
    append_optional_u64(&mut summary, "data_rx", status.get("data_frames_rx"));
    append_optional_u64(&mut summary, "data_tx", status.get("data_frames_tx"));
    append_optional_u64(&mut summary, "cmd_rx", status.get("command_frames_rx"));
    append_optional_u64(&mut summary, "ready_rx", status.get("ready_frames_rx"));
    append_optional_u64(&mut summary, "init_tx", status.get("init_frames_tx"));
    append_optional_u64(&mut summary, "shutdown_tx", status.get("shutdown_frames_tx"));
    append_optional_u64(&mut summary, "mgmt_tx", status.get("management_frames_tx"));
    append_optional_u64(&mut summary, "activity_tx", status.get("activity_frames_tx"));
    append_optional_u64(&mut summary, "beacon_tx", status.get("id_beacon_frames_tx"));
    append_optional_u64(&mut summary, "rx", status.get("bytes_rx"));
    append_optional_u64(&mut summary, "tx", status.get("bytes_tx"));
    append_optional_u64(&mut summary, "decode_errors", status.get("decode_errors"));
    append_optional_u64(&mut summary, "deserialize_errors", status.get("deserialize_errors"));
    append_optional_u64(&mut summary, "rx_queue_errors", status.get("rx_queue_errors"));
    append_optional_u64(&mut summary, "serialize_errors", status.get("serialize_errors"));
    append_optional_u64(&mut summary, "read_errors", status.get("read_errors"));
    append_optional_u64(&mut summary, "tx_errors", status.get("tx_errors"));
    append_optional_u64(&mut summary, "eof", status.get("eof_count"));
    append_optional_u64(&mut summary, "flow_timeouts", status.get("flow_control_timeouts"));
    append_optional_u64(&mut summary, "ax25_drops", status.get("ax25_drops"));
    append_optional_u64(&mut summary, "data_drops", status.get("data_notifications_dropped"));
    append_optional_u64(&mut summary, "cmd_drops", status.get("command_notifications_dropped"));
    append_optional_str(&mut summary, "err", status.get("last_error"));
    Some(summary)
}

fn ble_gatt_runtime_summary(status: &Value) -> Option<String> {
    if !status.is_object() {
        return None;
    }
    let mut summary = format!(
        "ble_gatt state={} peripheral={}",
        value_str(status, "link_state"),
        value_str(status, "peripheral_id")
    );
    append_optional_str(&mut summary, "adapter", status.get("adapter"));
    append_optional_str(&mut summary, "service", status.get("service_uuid"));
    append_optional_u64(&mut summary, "mtu", status.get("mtu"));
    append_optional_u64(&mut summary, "scan_ms", status.get("scan_timeout_ms"));
    append_optional_u64(&mut summary, "connect_ms", status.get("connect_timeout_ms"));
    append_optional_bool(&mut summary, "connected", status.get("connected"));
    append_optional_bool(&mut summary, "subscribed", status.get("subscribed"));
    append_optional_u64(&mut summary, "reconnects", status.get("reconnect_attempts"));
    append_optional_u64(&mut summary, "rxp", status.get("packets_rx"));
    append_optional_u64(&mut summary, "txp", status.get("packets_tx"));
    append_optional_u64(&mut summary, "rx_frames", status.get("frames_rx"));
    append_optional_u64(&mut summary, "tx_frames", status.get("frames_tx"));
    append_optional_u64(&mut summary, "notify_rx", status.get("notification_bytes_rx"));
    append_optional_u64(&mut summary, "rx", status.get("bytes_rx"));
    append_optional_u64(&mut summary, "tx", status.get("bytes_tx"));
    append_optional_u64(&mut summary, "chunks_tx", status.get("write_chunks_tx"));
    append_optional_u64(&mut summary, "scan_errors", status.get("scan_errors"));
    append_optional_u64(&mut summary, "connect_errors", status.get("connect_errors"));
    append_optional_u64(&mut summary, "subscribe_errors", status.get("subscribe_errors"));
    append_optional_u64(&mut summary, "probe_write_errors", status.get("probe_write_errors"));
    append_optional_u64(&mut summary, "probe_read_errors", status.get("probe_read_errors"));
    append_optional_u64(&mut summary, "serialize_errors", status.get("serialize_errors"));
    append_optional_u64(&mut summary, "hdlc_encode_errors", status.get("hdlc_encode_errors"));
    append_optional_u64(&mut summary, "hdlc_decode_errors", status.get("hdlc_decode_errors"));
    append_optional_u64(&mut summary, "deserialize_errors", status.get("deserialize_errors"));
    append_optional_u64(&mut summary, "rx_queue_errors", status.get("rx_queue_errors"));
    append_optional_u64(&mut summary, "write_errors", status.get("write_errors"));
    append_optional_u64(&mut summary, "read_errors", status.get("read_errors"));
    append_optional_u64(&mut summary, "buffer_drops", status.get("stale_buffer_drops"));
    append_optional_u64(&mut summary, "cleanup_errors", status.get("cleanup_errors"));
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

fn sum_rows_u64(rows: Option<&Vec<Value>>, key: &str) -> u64 {
    rows.map_or(0, |rows| rows.iter().filter_map(|row| row.get(key).and_then(Value::as_u64)).sum())
}

fn append_count(summary: &mut String, label: &str, value: usize) {
    if value > 0 {
        summary.push_str(&format!(" {label}={value}"));
    }
}

fn append_nonzero_u64(summary: &mut String, label: &str, value: u64) {
    if value > 0 {
        summary.push_str(&format!(" {label}={value}"));
    }
}

fn append_optional_u64(summary: &mut String, label: &str, value: Option<&Value>) {
    if let Some(value) = value.and_then(Value::as_u64) {
        summary.push_str(&format!(" {label}={value}"));
    }
}

fn append_optional_bool(summary: &mut String, label: &str, value: Option<&Value>) {
    if let Some(value) = value.and_then(Value::as_bool) {
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
    fn weave_display_view_reports_framebuffer_and_status_detail() {
        let status = json!({
            "link_state": "connected",
            "wdcl_connected": true,
            "remote_switch_id": "0011223344556677",
            "display": {
                "color_format": 1,
                "width": 128,
                "height": 64,
                "total_size": 4,
                "received_size": 4,
                "complete": true,
                "buffer_hex": "aabbccdd"
            },
            "device_stats": {
                "cpu_load": 42,
                "memory_used_percent_bp": 5125,
                "task_cpu": {
                    "wdcl": {
                        "cpu_load": 7,
                        "samples": 3
                    }
                }
            },
            "last_error": "synthetic weave warning"
        });
        let mut output = Vec::new();

        write_weave_display_view(&mut output, "weave-main", &status).expect("write display view");
        let output = String::from_utf8(output).expect("utf8");

        assert!(output.contains("Weave Display: weave-main"));
        assert!(output.contains("link=connected wdcl=true remote=0011223344556677"));
        assert!(output.contains("size=128x64 complete=true color=1 bytes=4/4"));
        assert!(output.contains("buffer_hex=aabbccdd"));
        assert!(output.contains("stats cpu=42 mem=51.25% tasks=1"));
        assert!(output.contains("err=synthetic weave warning"));

        let report = weave_display_report("weave-main", &status);
        assert_eq!(report["interface"], "weave-main");
        assert_eq!(report["display"]["buffer_hex"], "aabbccdd");
        assert_eq!(report["device_stats"]["task_cpu"]["wdcl"]["samples"], 3);
    }

    #[test]
    fn human_status_includes_interface_runtime_detail() {
        let status = json!({
            "identity_hash": "abc",
            "running": true,
            "interface_count": 14,
            "interfaces": [
                {
                    "name": "auto-main",
                    "type": "auto",
                    "enabled": true,
                    "settings": {
                        "_runtime": {
                            "startup_status": "spawned",
                            "auto": {
                                "carrier_runtime": {
                                    "online": true,
                                    "final_init_done": true,
                                    "carrier_changed": true,
                                    "carrier_event_count": 1,
                                    "adopted_device_count": 1,
                                    "adopted_add_count": 2,
                                    "adopted_remove_count": 1,
                                    "link_local_replacement_count": 1,
                                    "carrier_events": [
                                        {
                                            "event": "carrier_recovered",
                                            "ifname": "eth0"
                                        }
                                    ],
                                    "link_local_update": {
                                        "ifname": "eth0",
                                        "old_link_local_address": "fe80::1234%eth0",
                                        "new_link_local_address": "fe80::5678%eth0"
                                    }
                                }
                            }
                        }
                    }
                },
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
                                        {
                                            "direction": "outbound",
                                            "state": "connected",
                                            "bytes_rx": 10,
                                            "bytes_tx": 20
                                        },
                                        {
                                            "direction": "incoming",
                                            "state": "stale",
                                            "bytes_rx": 30,
                                            "bytes_tx": 40
                                        },
                                        {
                                            "direction": "incoming",
                                            "state": "closed",
                                            "bytes_rx": 5,
                                            "bytes_tx": 0
                                        }
                                    ]
                                }
                            }
                        }
                    }
                },
                {
                    "name": "backbone-main",
                    "type": "backbone_client",
                    "enabled": true,
                    "settings": {
                        "_runtime": {
                            "startup_status": "spawned",
                            "tcp": {
                                "stream_status": {
                                    "endpoint": "127.0.0.1:4242",
                                    "stream_state": "reconnecting",
                                    "reconnect_attempts": 3,
                                    "bytes_rx": 12,
                                    "bytes_tx": 34,
                                    "keepalives_sent": 2,
                                    "stale_events": 1,
                                    "read_timeouts": 1,
                                    "closed_events": 1,
                                    "error_events": 1,
                                    "liveness_enabled": true,
                                    "forced_bitrate_bps": 9600,
                                    "last_error": "tcp stream read timeout"
                                }
                            }
                        }
                    }
                },
                {
                    "name": "backbone-listener",
                    "type": "backbone",
                    "enabled": true,
                    "settings": {
                        "_runtime": {
                            "startup_status": "active",
                            "tcp": {
                                "listener_status": {
                                    "bind_addr": "0.0.0.0:4242",
                                    "listener_state": "listening",
                                    "client_liveness_enabled": true,
                                    "client_forced_bitrate_bps": 9600,
                                    "accepted_connections": 2,
                                    "accept_errors": 1,
                                    "latest_client_endpoint": "127.0.0.1:54000",
                                    "latest_stream_status": {
                                        "stream_state": "connected",
                                        "bytes_rx": 56,
                                        "bytes_tx": 78
                                    },
                                    "last_error": null
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
                                    "remote_switch_id": "0011223344556677",
                                    "bytes_rx": 120,
                                    "bytes_tx": 80,
                                    "frames_rx": 9,
                                    "frames_tx": 7,
                                    "invalid_frames": 1,
                                    "last_log_event": "0xe003",
                                    "display": {
                                        "color_format": 1,
                                        "width": 128,
                                        "height": 64,
                                        "total_size": 1024,
                                        "received_size": 1024,
                                        "complete": true,
                                        "buffer_hex": "aabbccdd"
                                    },
                                    "device_stats": {
                                        "cpu_load": 42,
                                        "memory_used_percent_bp": 5125,
                                        "task_cpu": {
                                            "wdcl": {
                                                "cpu_load": 7,
                                                "samples": 3
                                            }
                                        }
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
                                    "startup_probe": {
                                        "detected": true,
                                        "firmware_version": {
                                            "major": 1,
                                            "minor": 74,
                                            "label": "1.74"
                                        },
                                        "platform": 128,
                                        "mcu": 1,
                                        "interfaces": {
                                            "2": "SX126X",
                                            "3": "SX128X"
                                        },
                                        "interface_summary": "2:SX126X,3:SX128X"
                                    },
                                    "vports": [2, 3]
                                }
                            }
                        }
                    }
                },
                {
                    "name": "vrn76-main",
                    "type": "vrn76_kiss_ble",
                    "enabled": true,
                    "settings": {
                        "peripheral_id": "VR-N76",
                        "_runtime": {
                            "startup_status": "spawned",
                            "vrn76": {
                                "status": {
                                    "connected": true,
                                    "subscribed": true,
                                    "interface_ready": true,
                                    "startup_write_failures": 1,
                                    "pending_payloads": 2,
                                    "pending_writes": 3,
                                    "pending_packets": 4
                                }
                            }
                        }
                    }
                },
                {
                    "name": "udp-main",
                    "type": "udp",
                    "enabled": true,
                    "settings": {
                        "_runtime": {
                            "startup_status": "spawned",
                            "udp": {
                                "status": {
                                    "link_state": "configured",
                                    "role": "peer",
                                    "bind_addr": "127.0.0.1:4242",
                                    "forward_addr": "192.0.2.1:4242",
                                    "peer_routes": 2,
                                    "packets_rx": 3,
                                    "packets_tx": 4,
                                    "bytes_rx": 120,
                                    "bytes_tx": 80,
                                    "decode_errors": 1,
                                    "rx_queue_errors": 2,
                                    "socket_errors": 3,
                                    "tx_errors": 4,
                                    "dropped_direct": 5,
                                    "last_error": "simulated udp decode failure"
                                }
                            }
                        }
                    }
                },
                {
                    "name": "serial-main",
                    "type": "serial",
                    "enabled": true,
                    "settings": {
                        "_runtime": {
                            "startup_status": "spawned",
                            "serial": {
                                "status": {
                                    "link_state": "configured",
                                    "device": "/dev/ttyUSB0",
                                    "baud_rate": 19200,
                                    "data_bits": 7,
                                    "parity": "even",
                                    "stop_bits": 2,
                                    "flow_control": "hardware",
                                    "mtu": 1024,
                                    "reconnect_attempts": 2,
                                    "open_errors": 1,
                                    "packets_rx": 3,
                                    "packets_tx": 4,
                                    "frames_rx": 5,
                                    "frames_tx": 6,
                                    "bytes_rx": 120,
                                    "bytes_tx": 80,
                                    "decode_errors": 1,
                                    "deserialize_errors": 2,
                                    "rx_queue_errors": 3,
                                    "serialize_errors": 4,
                                    "hdlc_encode_errors": 5,
                                    "tx_errors": 6,
                                    "read_errors": 7,
                                    "eof_count": 8,
                                    "last_error": "simulated serial read failure"
                                }
                            }
                        }
                    }
                },
                {
                    "name": "kiss-main",
                    "type": "ax25_kiss",
                    "enabled": true,
                    "settings": {
                        "_runtime": {
                            "startup_status": "spawned",
                            "kiss": {
                                "status": {
                                    "link_state": "configured",
                                    "bearer": "serial",
                                    "device": "/dev/ttyKISS0",
                                    "baud_rate": 1200,
                                    "mtu": 564,
                                    "preamble_ms": 350,
                                    "tx_tail_ms": 20,
                                    "kiss_flow_control": true,
                                    "ax25": true,
                                    "callsign": "N0CALL",
                                    "ssid": 1,
                                    "id_callsign": "MYCALL-0",
                                    "id_interval": 600,
                                    "interface_ready": false,
                                    "pending_depth": 2,
                                    "reconnect_attempts": 3,
                                    "open_errors": 1,
                                    "packets_rx": 4,
                                    "packets_tx": 5,
                                    "data_frames_rx": 6,
                                    "data_frames_tx": 7,
                                    "command_frames_rx": 8,
                                    "ready_frames_rx": 9,
                                    "init_frames_tx": 10,
                                    "shutdown_frames_tx": 11,
                                    "management_frames_tx": 12,
                                    "activity_frames_tx": 13,
                                    "id_beacon_frames_tx": 14,
                                    "bytes_rx": 120,
                                    "bytes_tx": 80,
                                    "decode_errors": 1,
                                    "deserialize_errors": 2,
                                    "rx_queue_errors": 3,
                                    "serialize_errors": 4,
                                    "read_errors": 5,
                                    "tx_errors": 6,
                                    "eof_count": 7,
                                    "flow_control_timeouts": 8,
                                    "ax25_drops": 9,
                                    "data_notifications_dropped": 10,
                                    "command_notifications_dropped": 11,
                                    "last_error": "simulated kiss read failure"
                                }
                            }
                        }
                    }
                },
                {
                    "name": "kiss-wifi",
                    "type": "kiss_tcp_client",
                    "enabled": true,
                    "settings": {
                        "_runtime": {
                            "startup_status": "spawned",
                            "kiss_tcp": {
                                "status": {
                                    "link_state": "configured",
                                    "bearer": "tcp",
                                    "endpoint": "127.0.0.1:8001",
                                    "kiss_flow_control": false,
                                    "ax25": false,
                                    "connect_errors": 2,
                                    "packets_rx": 3,
                                    "packets_tx": 4,
                                    "bytes_rx": 55,
                                    "bytes_tx": 66
                                }
                            }
                        }
                    }
                },
                {
                    "name": "ble-main",
                    "type": "ble_gatt",
                    "enabled": true,
                    "settings": {
                        "_runtime": {
                            "startup_status": "spawned",
                            "ble_gatt": {
                                "status": {
                                    "link_state": "configured",
                                    "adapter": "Bluetooth",
                                    "peripheral_id": "AA:BB:CC:DD:EE:FF",
                                    "service_uuid": "12345678-1234-1234-1234-1234567890ab",
                                    "mtu": 128,
                                    "scan_timeout_ms": 10000,
                                    "connect_timeout_ms": 3000,
                                    "connected": true,
                                    "subscribed": true,
                                    "reconnect_attempts": 2,
                                    "packets_rx": 6,
                                    "packets_tx": 7,
                                    "frames_rx": 8,
                                    "frames_tx": 9,
                                    "notification_bytes_rx": 100,
                                    "bytes_rx": 80,
                                    "bytes_tx": 90,
                                    "write_chunks_tx": 10,
                                    "scan_errors": 1,
                                    "connect_errors": 2,
                                    "subscribe_errors": 3,
                                    "probe_write_errors": 4,
                                    "probe_read_errors": 5,
                                    "serialize_errors": 11,
                                    "hdlc_encode_errors": 12,
                                    "hdlc_decode_errors": 13,
                                    "deserialize_errors": 14,
                                    "rx_queue_errors": 15,
                                    "write_errors": 16,
                                    "read_errors": 17,
                                    "stale_buffer_drops": 18,
                                    "cleanup_errors": 19,
                                    "last_error": "simulated ble read failure"
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
        assert!(output.contains("auto online=true init=true carrier_changed=true carrier_events=1"));
        assert!(output.contains("adopted=1"));
        assert!(output.contains("added=2"));
        assert!(output.contains("removed=1"));
        assert!(output.contains("replaced=1"));
        assert!(output.contains("link_local=eth0"));
        assert!(output.contains("new_ll=fe80::5678%eth0"));
        assert!(output.contains("i2p sam=127.0.0.1:7656 accept=listening peers=3"));
        assert!(output.contains("connected=1"));
        assert!(output.contains("stale=1"));
        assert!(output.contains("closed=1"));
        assert!(output.contains("outbound=1"));
        assert!(output.contains("incoming=2"));
        assert!(output.contains("rx=45"));
        assert!(output.contains("tx=60"));
        assert!(output.contains("tcp stream=reconnecting endpoint=127.0.0.1:4242 reconnects=3"));
        assert!(output.contains("keepalives=2"));
        assert!(output.contains("stale=1"));
        assert!(output.contains("timeouts=1"));
        assert!(output.contains("errors=1"));
        assert!(output.contains("liveness=true"));
        assert!(output.contains("bitrate=9600"));
        assert!(output.contains("err=tcp stream read timeout"));
        assert!(output.contains("tcp listener=listening bind=0.0.0.0:4242 accepted=2"));
        assert!(output.contains("accept_errors=1"));
        assert!(output.contains("child_liveness=true"));
        assert!(output.contains("child_bitrate=9600"));
        assert!(output.contains("latest=127.0.0.1:54000"));
        assert!(output.contains("latest_state=connected"));
        assert!(output.contains("latest_rx=56"));
        assert!(output.contains("latest_tx=78"));
        assert!(output.contains("weave link=connected endpoints=2 wdcl=true"));
        assert!(output.contains("remote=0011223344556677"));
        assert!(output.contains("rx_frames=9"));
        assert!(output.contains("tx_frames=7"));
        assert!(output.contains("invalid_frames=1"));
        assert!(output.contains("last_log=0xe003"));
        assert!(output.contains("display=128x64/true"));
        assert!(output.contains("display_bytes=1024/1024"));
        assert!(output.contains("color=1"));
        assert!(output.contains("cpu=42"));
        assert!(output.contains("mem=51.25%"));
        assert!(output.contains("tasks=1"));
        assert!(output.contains("rnode bearer=serial online=true detected=true"));
        assert!(output.contains("fw=1.52"));
        assert!(output.contains("freq=915000000"));
        assert!(output.contains("bat=88"));
        assert!(output.contains("rnode_multi stream=running selected=2 vports=2"));
        assert!(output.contains("detected=true"));
        assert!(output.contains("fw=1.74"));
        assert!(output.contains("platform=128"));
        assert!(output.contains("mcu=1"));
        assert!(output.contains("probe=2:SX126X,3:SX128X"));
        assert!(output.contains("vrn76 connected=true subscribed=true ready=true"));
        assert!(output.contains("startup_write_failures=1"));
        assert!(output.contains("pending_payloads=2"));
        assert!(output.contains("pending_writes=3"));
        assert!(output.contains("pending_packets=4"));
        assert!(output.contains("udp state=configured role=peer bind=127.0.0.1:4242"));
        assert!(output.contains("forward=192.0.2.1:4242"));
        assert!(output.contains("peers=2"));
        assert!(output.contains("rxp=3"));
        assert!(output.contains("txp=4"));
        assert!(output.contains("rx=120"));
        assert!(output.contains("tx=80"));
        assert!(output.contains("decode_errors=1"));
        assert!(output.contains("rx_queue_errors=2"));
        assert!(output.contains("socket_errors=3"));
        assert!(output.contains("tx_errors=4"));
        assert!(output.contains("dropped_direct=5"));
        assert!(output.contains("err=simulated udp decode failure"));
        assert!(output.contains("serial state=configured device=/dev/ttyUSB0 baud=19200"));
        assert!(output.contains("data_bits=7"));
        assert!(output.contains("flow=hardware"));
        assert!(output.contains("reconnects=2"));
        assert!(output.contains("open_errors=1"));
        assert!(output.contains("rx_frames=5"));
        assert!(output.contains("tx_frames=6"));
        assert!(output.contains("deserialize_errors=2"));
        assert!(output.contains("serialize_errors=4"));
        assert!(output.contains("hdlc_encode_errors=5"));
        assert!(output.contains("read_errors=7"));
        assert!(output.contains("eof=8"));
        assert!(output.contains("err=simulated serial read failure"));
        assert!(output.contains("kiss state=configured bearer=serial device=/dev/ttyKISS0"));
        assert!(output.contains("ax25=true"));
        assert!(output.contains("callsign=N0CALL"));
        assert!(output.contains("ready=false"));
        assert!(output.contains("pending=2"));
        assert!(output.contains("data_rx=6"));
        assert!(output.contains("cmd_rx=8"));
        assert!(output.contains("beacon_tx=14"));
        assert!(output.contains("flow_timeouts=8"));
        assert!(output.contains("ax25_drops=9"));
        assert!(output.contains("data_drops=10"));
        assert!(output.contains("cmd_drops=11"));
        assert!(output.contains("err=simulated kiss read failure"));
        assert!(output.contains("kiss state=configured bearer=tcp endpoint=127.0.0.1:8001"));
        assert!(output.contains("connect_errors=2"));
        assert!(output.contains("rx=55"));
        assert!(output.contains("tx=66"));
        assert!(output.contains("ble_gatt state=configured peripheral=AA:BB:CC:DD:EE:FF"));
        assert!(output.contains("service=12345678-1234-1234-1234-1234567890ab"));
        assert!(output.contains("connected=true"));
        assert!(output.contains("subscribed=true"));
        assert!(output.contains("notify_rx=100"));
        assert!(output.contains("chunks_tx=10"));
        assert!(output.contains("scan_errors=1"));
        assert!(output.contains("probe_read_errors=5"));
        assert!(output.contains("hdlc_decode_errors=13"));
        assert!(output.contains("buffer_drops=18"));
        assert!(output.contains("err=simulated ble read failure"));
        assert!(output.contains("pipe state=respawning open=false respawns=2"));
        assert!(output.contains("err=spawn cat failed"));
    }

    #[test]
    fn interface_endpoint_uses_family_specific_settings() {
        assert_eq!(
            interface_endpoint(&json!({
                "host": "127.0.0.1",
                "port": 4242,
                "settings": {
                    "target_host": "192.0.2.10",
                    "target_port": 4242
                }
            })),
            "127.0.0.1:4242->192.0.2.10:4242"
        );
        assert_eq!(
            interface_endpoint(&json!({
                "settings": {
                    "socket_path": "@rns/default"
                }
            })),
            "@rns/default"
        );
        assert_eq!(
            interface_endpoint(&json!({
                "settings": {
                    "device": "/dev/ttyACM0"
                }
            })),
            "/dev/ttyACM0"
        );
        assert_eq!(
            interface_endpoint(&json!({
                "settings": {
                    "peripheral_id": "VR-N76"
                }
            })),
            "ble:VR-N76"
        );
        assert_eq!(
            interface_endpoint(&json!({
                "settings": {
                    "command": "cat"
                }
            })),
            "cat"
        );
        assert_eq!(
            interface_endpoint(&json!({
                "settings": {
                    "sam_host": "127.0.0.1",
                    "sam_port": 7656
                }
            })),
            "sam:127.0.0.1:7656"
        );
        assert_eq!(
            interface_endpoint(&json!({
                "settings": {
                    "peers": ["alpha.b32.i2p", "beta.b32.i2p"]
                }
            })),
            "peers:2"
        );
        assert_eq!(
            interface_endpoint(&json!({
                "settings": {
                    "group_id": "field-net"
                }
            })),
            "group:field-net"
        );
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
