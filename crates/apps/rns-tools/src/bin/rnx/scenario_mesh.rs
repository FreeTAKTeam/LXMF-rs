use std::collections::HashSet;
use std::fs;
use std::io;
use std::process::Child;
use std::time::Duration;

use crate::harness::{
    cleanup_child, derive_preferred_transport_port, ensure_rpc_ok, poll_for_any_peer, reserve_port,
    rpc_call, spawn_daemon, wait_for_ready,
};
use crate::DeliveryMode;

use super::{run_delivery_mode, run_paper_workflow, selected_mesh_delivery_modes};

struct MeshNodeProcess {
    rpc: String,
    destination_hash: String,
    propagation_hash: Option<String>,
    child: Child,
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

    let mut request_id = 10_u64;
    let first = 0_usize;
    let last = nodes - 1;

    let result = (|| -> io::Result<()> {
        for node in &node_processes {
            rpc_call(&node.rpc, request_id, "announce_now", None)?;
            request_id = request_id.wrapping_add(1);
        }

        for node in &node_processes {
            let discovered =
                poll_for_any_peer(&node.rpc, timeout, request_id, Some(&node.destination_hash))?;
            if discovered.is_none() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "mesh propagation failed: a node did not discover any peer",
                ));
            }
            request_id = request_id.wrapping_add(1);
        }

        if propagation_enabled {
            for node in &node_processes {
                rpc_call(&node.rpc, request_id, "announce_now", None)?;
                request_id = request_id.wrapping_add(1);
            }
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

    cleanup_mesh_children(&mut node_processes, keep);
    drop(temp_dirs);
    result
}

fn cleanup_mesh_children(node_processes: &mut [MeshNodeProcess], keep: bool) {
    for node in node_processes {
        cleanup_child(&mut node.child, keep);
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
