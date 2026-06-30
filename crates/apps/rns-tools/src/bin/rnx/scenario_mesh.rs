use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Child;
use std::process::Command as ProcessCommand;
use std::time::Duration;

use crate::harness::{
    cleanup_child, derive_preferred_transport_port, ensure_rpc_ok, poll_for_any_peer, reserve_port,
    rpc_call, spawn_daemon, wait_for_ready,
};
use crate::DeliveryMode;

use crate::scenario::{run_delivery_mode, run_paper_workflow, selected_mesh_delivery_modes};

const RNPATH_SMOKE_TAG_HEX: &str = "01020304";

struct MeshNodeProcess {
    rpc: String,
    destination_hash: String,
    propagation_hash: Option<String>,
    child: Child,
}

struct MeshRuntime {
    temp_dirs: Vec<tempfile::TempDir>,
    node_processes: Vec<MeshNodeProcess>,
}

pub(crate) fn run_mesh_sim(
    nodes: usize,
    base_rpc_port: u16,
    timeout_secs: u64,
    keep: bool,
    modes: Vec<DeliveryMode>,
) -> io::Result<()> {
    if !(3..=10).contains(&nodes) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "nodes must be in range 3..=10"));
    }

    let timeout = Duration::from_secs(timeout_secs);
    let selected_modes = selected_mesh_delivery_modes(&modes);
    let propagation_enabled = selected_modes.contains(&DeliveryMode::Propagated);
    let mut runtime = start_mesh_nodes(nodes, base_rpc_port, timeout, propagation_enabled, keep)?;

    let mut request_id = 10_u64;
    let first = 0_usize;
    let last = nodes - 1;

    let result = (|| -> io::Result<()> {
        let node_processes = &runtime.node_processes;

        announce_mesh_nodes(node_processes, &mut request_id)?;
        wait_for_mesh_peer_visibility(node_processes, timeout, &mut request_id)?;

        if propagation_enabled {
            announce_mesh_nodes(node_processes, &mut request_id)?;
            for (idx, node) in node_processes.iter().enumerate() {
                let target = node_processes[(idx + 1) % node_processes.len()]
                    .propagation_hash
                    .clone()
                    .ok_or_else(|| {
                        io::Error::other("mesh node did not report propagation destination hash")
                    })?;
                let response = rpc_call(
                    &node.rpc,
                    request_id,
                    "set_outbound_propagation_node",
                    Some(serde_json::json!({ "peer": target })),
                )?;
                ensure_rpc_ok(response, "set_outbound_propagation_node (mesh)")?;
                request_id = request_id.wrapping_add(1);
            }
        }

        for mode in selected_modes {
            match mode {
                DeliveryMode::Direct | DeliveryMode::Opportunistic | DeliveryMode::Propagated => {
                    run_delivery_mode(
                        mode,
                        &node_processes[first].rpc,
                        &node_processes[last].rpc,
                        &node_processes[first].destination_hash,
                        &node_processes[last].destination_hash,
                        timeout,
                        &mut request_id,
                    )?;
                    run_delivery_mode(
                        mode,
                        &node_processes[last].rpc,
                        &node_processes[first].rpc,
                        &node_processes[last].destination_hash,
                        &node_processes[first].destination_hash,
                        timeout,
                        &mut request_id,
                    )?;
                }
                DeliveryMode::Paper => {
                    run_paper_workflow(
                        &node_processes[first].rpc,
                        &node_processes[last].rpc,
                        &node_processes[first].destination_hash,
                        &node_processes[last].destination_hash,
                        timeout,
                        &mut request_id,
                    )?;
                }
            }
        }

        println!("MESH ok: nodes={} announce propagation established across mesh", nodes);
        println!("MESH ok: multi-hop delivery workflows completed");
        Ok(())
    })();

    runtime.cleanup(keep);
    result
}

pub(crate) fn run_rnpath_smoke(
    nodes: usize,
    base_rpc_port: u16,
    timeout_secs: u64,
    keep: bool,
) -> io::Result<()> {
    if !(4..=10).contains(&nodes) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "nodes must be in range 4..=10"));
    }

    let timeout = Duration::from_secs(timeout_secs);
    let mut runtime = start_mesh_nodes(nodes, base_rpc_port, timeout, false, keep)?;
    let mut request_id = 10_u64;

    let result = (|| -> io::Result<()> {
        let node_processes = &runtime.node_processes;
        announce_mesh_nodes(node_processes, &mut request_id)?;
        wait_for_mesh_peer_visibility(node_processes, timeout, &mut request_id)?;

        let source = 0_usize;
        let target = nodes / 2;
        let target_hash = &node_processes[target].destination_hash;
        let rnpath_output =
            run_rnpath_binary(&node_processes[source].rpc, target_hash, timeout_secs, None, None)?;
        let path_result = parse_rnpath_output(&rnpath_output, target_hash)?;
        let hops = path_result.get("hops").and_then(serde_json::Value::as_u64);
        let next_hop = required_path_field(&path_result, "next_hop")?;
        let interface = normalized_hash_field(&path_result, "interface")?;
        let scoped_output = run_rnpath_binary(
            &node_processes[source].rpc,
            target_hash,
            timeout_secs,
            Some(&interface),
            Some(RNPATH_SMOKE_TAG_HEX),
        )?;
        let scoped_result = parse_rnpath_output(&scoped_output, target_hash)?;
        validate_scoped_rnpath_result(
            &scoped_result,
            &interface,
            RNPATH_SMOKE_TAG_HEX,
            next_hop,
            hops,
        )?;

        let hop_display =
            hops.map(|value| value.to_string()).unwrap_or_else(|| "unknown".to_owned());
        println!(
            "RNPATH ok: node={} target={} destination={} next_hop={} interface={} reported_hops={}",
            source, target, target_hash, next_hop, interface, hop_display
        );
        println!(
            "RNPATH ok: scoped request on_iface={} tag_hex={} echoed by daemon",
            interface, RNPATH_SMOKE_TAG_HEX
        );
        println!("RNPATH ok: local non-neighbor mesh daemon path smoke completed");
        Ok(())
    })();

    runtime.cleanup(keep);
    result
}

fn start_mesh_nodes(
    nodes: usize,
    base_rpc_port: u16,
    timeout: Duration,
    propagation_enabled: bool,
    keep: bool,
) -> io::Result<MeshRuntime> {
    let mut reserved_ports = HashSet::new();
    let mut rpc_listeners = Vec::with_capacity(nodes);
    let mut rpc_ports = Vec::with_capacity(nodes);
    let mut transport_listeners = Vec::with_capacity(nodes);
    let mut transport_ports = Vec::with_capacity(nodes);

    for idx in 0..nodes {
        let offset = u16::try_from(idx).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "nodes index exceeds u16 range")
        })?;
        let preferred_rpc = base_rpc_port
            .checked_add(offset)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "rpc port overflow"))?;
        let rpc_listener = reserve_port(preferred_rpc, &reserved_ports)?;
        let rpc_port = rpc_listener.local_addr()?.port();
        reserved_ports.insert(rpc_port);
        rpc_ports.push(rpc_port);
        rpc_listeners.push(rpc_listener);
    }

    for rpc_port in &rpc_ports {
        let preferred_transport = derive_preferred_transport_port(*rpc_port, 100)?;
        let transport_listener = reserve_port(preferred_transport, &reserved_ports)?;
        let transport_port = transport_listener.local_addr()?.port();
        reserved_ports.insert(transport_port);
        transport_ports.push(transport_port);
        transport_listeners.push(transport_listener);
    }

    let mut temp_dirs = Vec::with_capacity(nodes);
    let mut db_paths = Vec::with_capacity(nodes);
    let mut config_paths = Vec::with_capacity(nodes);
    for idx in 0..nodes {
        let dir = tempfile::TempDir::new()?;
        let db_path = dir.path().join(format!("reticulum-{idx}.db"));
        let config_path = dir.path().join(format!("reticulum-{idx}.toml"));
        fs::write(&config_path, build_mesh_client_config(idx, &transport_ports))?;
        db_paths.push(db_path);
        config_paths.push(config_path);
        temp_dirs.push(dir);
    }

    drop(rpc_listeners);
    drop(transport_listeners);

    let mut node_processes = Vec::with_capacity(nodes);
    for idx in 0..nodes {
        let rpc = format!("127.0.0.1:{}", rpc_ports[idx]);
        let transport = format!("127.0.0.1:{}", transport_ports[idx]);
        let mut child = match spawn_daemon(
            &rpc,
            &db_paths[idx],
            &transport,
            &config_paths[idx],
            propagation_enabled,
        ) {
            Ok(child) => child,
            Err(err) => {
                cleanup_mesh_children(&mut node_processes, keep);
                return Err(err);
            }
        };
        let ready = match wait_for_ready(
            child.stdout.take().ok_or_else(|| io::Error::other("missing daemon stdout"))?,
            timeout,
        ) {
            Ok(ready) => ready,
            Err(err) => {
                cleanup_mesh_children(&mut node_processes, keep);
                cleanup_child(&mut child, keep);
                return Err(err);
            }
        };
        let destination_hash = ready
            .delivery_hash
            .clone()
            .ok_or_else(|| io::Error::other("daemon did not report destination hash"))?;

        node_processes.push(MeshNodeProcess {
            rpc,
            destination_hash,
            propagation_hash: ready.propagation_hash,
            child,
        });
    }

    Ok(MeshRuntime { temp_dirs, node_processes })
}

fn announce_mesh_nodes(node_processes: &[MeshNodeProcess], request_id: &mut u64) -> io::Result<()> {
    for node in node_processes {
        rpc_call(&node.rpc, *request_id, "announce_now", None)?;
        *request_id = (*request_id).wrapping_add(1);
    }
    Ok(())
}

fn wait_for_mesh_peer_visibility(
    node_processes: &[MeshNodeProcess],
    timeout: Duration,
    request_id: &mut u64,
) -> io::Result<()> {
    for node in node_processes {
        let discovered =
            poll_for_any_peer(&node.rpc, timeout, *request_id, Some(&node.destination_hash))?;
        if discovered.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "mesh propagation failed: a node did not discover any peer",
            ));
        }
        *request_id = (*request_id).wrapping_add(1);
    }
    Ok(())
}

fn run_rnpath_binary(
    rpc: &str,
    destination_hash: &str,
    timeout_secs: u64,
    on_iface: Option<&str>,
    tag_hex: Option<&str>,
) -> io::Result<Vec<u8>> {
    let mut command = ProcessCommand::new(rnpath_rs_path()?);
    command
        .arg(destination_hash)
        .arg("--rpc")
        .arg(rpc)
        .arg("--timeout")
        .arg(timeout_secs.to_string())
        .arg("--json");
    if let Some(on_iface) = on_iface {
        command.arg("--on-iface").arg(on_iface);
    }
    if let Some(tag_hex) = tag_hex {
        command.arg("--tag-hex").arg(tag_hex);
    }
    let output = command.output()?;

    if !output.status.success() {
        return Err(io::Error::other(format!(
            "rnpath-rs failed with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(output.stdout)
}

fn parse_rnpath_output(output: &[u8], destination_hash: &str) -> io::Result<serde_json::Value> {
    let result: serde_json::Value = serde_json::from_slice(output)?;
    if result.get("path_found").and_then(serde_json::Value::as_bool) != Some(true) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("rnpath-rs did not report path_found=true: {result}"),
        ));
    }
    if result.get("status").and_then(serde_json::Value::as_str) != Some("found") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("rnpath-rs did not report status=found: {result}"),
        ));
    }
    if result.get("destination_hash").and_then(serde_json::Value::as_str) != Some(destination_hash)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("rnpath-rs reported unexpected destination hash: {result}"),
        ));
    }
    Ok(result)
}

fn validate_scoped_rnpath_result(
    result: &serde_json::Value,
    on_iface: &str,
    tag_hex: &str,
    expected_next_hop: &str,
    expected_hops: Option<u64>,
) -> io::Result<()> {
    let reported_iface = required_path_field(result, "on_iface")?;
    if reported_iface != on_iface {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("rnpath-rs reported unexpected on_iface: {result}"),
        ));
    }
    let interface_scope = required_path_field(result, "interface_scope")?;
    if interface_scope != on_iface {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("rnpath-rs reported unexpected interface_scope: {result}"),
        ));
    }
    let reported_tag = required_path_field(result, "tag_hex")?;
    if reported_tag != tag_hex {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("rnpath-rs reported unexpected tag_hex: {result}"),
        ));
    }
    let next_hop = required_path_field(result, "next_hop")?;
    if next_hop != expected_next_hop {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("scoped rnpath-rs result changed next_hop: {result}"),
        ));
    }
    let interface = normalized_hash_field(result, "interface")?;
    if interface != on_iface {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("scoped rnpath-rs result changed interface metadata: {result}"),
        ));
    }
    if result.get("hops").and_then(serde_json::Value::as_u64) != expected_hops {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("scoped rnpath-rs result changed hop metadata: {result}"),
        ));
    }
    Ok(())
}

fn normalized_hash_field(result: &serde_json::Value, key: &str) -> io::Result<String> {
    let value = required_path_field(result, key)?;
    let normalized =
        value.strip_prefix('/').and_then(|stripped| stripped.strip_suffix('/')).unwrap_or(value);
    if normalized.len() != 32 || !normalized.as_bytes().iter().all(u8::is_ascii_hexdigit) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("rnpath-rs reported non-hash {key}: {result}"),
        ));
    }
    Ok(normalized.to_ascii_lowercase())
}

fn required_path_field<'a>(result: &'a serde_json::Value, key: &str) -> io::Result<&'a str> {
    let value = result.get(key).and_then(serde_json::Value::as_str).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, format!("rnpath-rs omitted {key}: {result}"))
    })?;
    if value.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("rnpath-rs reported empty {key}: {result}"),
        ));
    }
    Ok(value)
}

fn rnpath_rs_path() -> io::Result<PathBuf> {
    let binary_name = format!("rnpath-rs{}", std::env::consts::EXE_SUFFIX);
    let exe = std::env::current_exe()?;
    let dir = exe.parent().ok_or_else(|| io::Error::other("missing exe parent"))?;
    let candidate = dir.join(&binary_name);
    if candidate.exists() {
        return Ok(candidate);
    }

    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(&binary_name);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "rnpath-rs binary not found; build it with `cargo build -p reticulumd --bin reticulumd -p rns-tools --bin rnx --bin rnpath-rs` before running `rnx rnpath-smoke`",
    ))
}

fn cleanup_mesh_children(node_processes: &mut [MeshNodeProcess], keep: bool) {
    for node in node_processes {
        cleanup_child(&mut node.child, keep);
    }
}

impl MeshRuntime {
    fn cleanup(&mut self, keep: bool) {
        cleanup_mesh_children(&mut self.node_processes, keep);
        self.temp_dirs.clear();
    }
}

fn build_mesh_client_config(node_index: usize, transport_ports: &[u16]) -> String {
    let node_count = transport_ports.len();
    let next = (node_index + 1) % node_count;
    let previous = (node_index + node_count - 1) % node_count;
    let mut neighbors = vec![next];
    if previous != next {
        neighbors.push(previous);
    }

    let mut config = String::new();
    for neighbor in neighbors {
        config.push_str(&format!(
            "[[interfaces]]\ntype = \"tcp_client\"\nenabled = true\nhost = \"127.0.0.1\"\nport = {}\n\n",
            transport_ports[neighbor]
        ));
    }
    config
}
