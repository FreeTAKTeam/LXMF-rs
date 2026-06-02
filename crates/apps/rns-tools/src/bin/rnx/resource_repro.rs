use std::collections::HashSet;
use std::fs;
use std::io;
use std::time::Duration;

use crate::{harness, DeliveryMode};
use harness::{
    cleanup_child, ensure_rpc_ok, poll_for_inbound_content, poll_for_peer, reserve_port, rpc_call,
    wait_for_ready,
};
use rns_rpc::e2e_harness::{build_send_params, build_tcp_client_config, timestamp_millis};

const RESOURCE_THRESHOLD_CONTENT_BYTES: usize = 420;

#[derive(Debug, Clone)]
struct ResourceReproPayload {
    label: &'static str,
    content: String,
    expect_resource: bool,
}

pub(crate) fn run_resource_repro(
    a_port: u16,
    b_port: u16,
    server_host: String,
    server_port: u16,
    timeout_secs: u64,
    large_bytes: usize,
    keep: bool,
) -> io::Result<()> {
    let timeout = Duration::from_secs(timeout_secs);
    let mut reserved_ports = HashSet::new();
    let a_rpc_listener = reserve_port(a_port, &reserved_ports)?;
    let a_rpc_port = a_rpc_listener.local_addr()?.port();
    reserved_ports.insert(a_rpc_port);
    let b_rpc_listener = reserve_port(b_port, &reserved_ports)?;
    let b_rpc_port = b_rpc_listener.local_addr()?.port();

    let a_rpc = format!("127.0.0.1:{a_rpc_port}");
    let b_rpc = format!("127.0.0.1:{b_rpc_port}");
    let shared_config = build_shared_tcp_client_config(&server_host, server_port);

    let a_dir = tempfile::TempDir::new()?;
    let b_dir = tempfile::TempDir::new()?;
    let a_db = a_dir.path().join("reticulum.db");
    let b_db = b_dir.path().join("reticulum.db");
    let a_config = a_dir.path().join("reticulum.toml");
    let b_config = b_dir.path().join("reticulum.toml");
    fs::write(&a_config, &shared_config)?;
    fs::write(&b_config, &shared_config)?;

    log::info!(
        "[resource-repro] tcp_server_path={}:{} a_rpc={} b_rpc={} large_bytes={}",
        server_host,
        server_port,
        a_rpc,
        b_rpc,
        large_bytes
    );

    drop(a_rpc_listener);
    let mut a_child =
        harness::spawn_daemon_with_optional_transport(&a_rpc, &a_db, None, &a_config, false, true)?;
    let a_ready = wait_for_ready(
        a_child.stdout.take().ok_or_else(|| io::Error::other("missing daemon A stdout"))?,
        timeout,
    );
    let a_ready = match a_ready {
        Ok(ready) => ready,
        Err(err) => {
            cleanup_child(&mut a_child, keep);
            return Err(err);
        }
    };

    drop(b_rpc_listener);
    let mut b_child =
        harness::spawn_daemon_with_optional_transport(&b_rpc, &b_db, None, &b_config, false, true)?;
    let b_ready = wait_for_ready(
        b_child.stdout.take().ok_or_else(|| io::Error::other("missing daemon B stdout"))?,
        timeout,
    );
    let b_ready = match b_ready {
        Ok(ready) => ready,
        Err(err) => {
            cleanup_child(&mut a_child, keep);
            cleanup_child(&mut b_child, keep);
            return Err(err);
        }
    };

    let a_delivery_hash = a_ready
        .delivery_hash
        .clone()
        .ok_or_else(|| io::Error::other("daemon A did not report delivery destination hash"))?;
    let b_delivery_hash = b_ready
        .delivery_hash
        .clone()
        .ok_or_else(|| io::Error::other("daemon B did not report delivery destination hash"))?;
    log::trace!(
        "RESOURCE_REPRO node_ready source_destination={} target_destination={} tcp_server={}:{}",
        a_delivery_hash,
        b_delivery_hash,
        server_host,
        server_port
    );

    let mut req_id = 1u64;
    announce_and_wait_for_peer(
        &a_rpc,
        &b_rpc,
        &a_delivery_hash,
        &b_delivery_hash,
        timeout,
        &mut req_id,
    )?;

    let payloads = resource_repro_payloads(large_bytes);
    run_resource_repro_case(
        "packet-baseline",
        &a_rpc,
        &b_rpc,
        &a_delivery_hash,
        &b_delivery_hash,
        &payloads[0],
        timeout,
        &mut req_id,
    )?;
    run_resource_repro_case(
        "case-a-fresh-link-resource",
        &b_rpc,
        &a_rpc,
        &b_delivery_hash,
        &a_delivery_hash,
        &payloads[1],
        timeout,
        &mut req_id,
    )?;
    run_resource_repro_case(
        "case-b-reused-link-resource",
        &a_rpc,
        &b_rpc,
        &a_delivery_hash,
        &b_delivery_hash,
        &payloads[1],
        timeout,
        &mut req_id,
    )?;
    run_resource_repro_case(
        "case-c-sequential-resource-1",
        &a_rpc,
        &b_rpc,
        &a_delivery_hash,
        &b_delivery_hash,
        &payloads[2],
        timeout,
        &mut req_id,
    )?;
    run_resource_repro_case(
        "case-c-sequential-resource-2",
        &a_rpc,
        &b_rpc,
        &a_delivery_hash,
        &b_delivery_hash,
        &payloads[2],
        timeout,
        &mut req_id,
    )?;

    cleanup_child(&mut a_child, keep);
    cleanup_child(&mut b_child, keep);
    log::trace!("RESOURCE_REPRO ok: packet and resource direct-link cases delivered");
    Ok(())
}

fn resource_repro_payloads(large_bytes: usize) -> Vec<ResourceReproPayload> {
    let large_bytes = large_bytes.max(1024);
    vec![
        ResourceReproPayload {
            label: "packet",
            content: "packet-baseline-lxmf-rs".to_string(),
            expect_resource: false,
        },
        ResourceReproPayload {
            label: "resource-threshold",
            content: format!("resource-threshold:{}", "r".repeat(RESOURCE_THRESHOLD_CONTENT_BYTES)),
            expect_resource: true,
        },
        ResourceReproPayload {
            label: "resource-large",
            content: format!("resource-large:{}", "R".repeat(large_bytes)),
            expect_resource: true,
        },
    ]
}

fn build_shared_tcp_client_config(host: &str, port: u16) -> String {
    build_tcp_client_config(host, port)
}

fn announce_and_wait_for_peer(
    a_rpc: &str,
    b_rpc: &str,
    a_delivery_hash: &str,
    b_delivery_hash: &str,
    timeout: Duration,
    request_id: &mut u64,
) -> io::Result<()> {
    rpc_call(b_rpc, *request_id, "announce_now", None)?;
    *request_id = (*request_id).wrapping_add(1);
    rpc_call(a_rpc, *request_id, "announce_now", None)?;
    *request_id = (*request_id).wrapping_add(1);

    let a_sees_b = poll_for_peer(a_rpc, b_delivery_hash, timeout, *request_id)?;
    *request_id = (*request_id).wrapping_add(1);
    if !a_sees_b {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("daemon A did not discover daemon B ({b_delivery_hash})"),
        ));
    }

    let b_sees_a = poll_for_peer(b_rpc, a_delivery_hash, timeout, *request_id)?;
    *request_id = (*request_id).wrapping_add(1);
    if !b_sees_a {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("daemon B did not discover daemon A ({a_delivery_hash})"),
        ));
    }
    Ok(())
}

fn run_resource_repro_case(
    case_label: &str,
    sender_rpc: &str,
    receiver_rpc: &str,
    source_destination: &str,
    target_destination: &str,
    payload: &ResourceReproPayload,
    timeout: Duration,
    request_id: &mut u64,
) -> io::Result<()> {
    let message_id = format!("resource-repro-{}-{}", case_label, timestamp_millis());
    let params = build_direct_send_params(
        &message_id,
        source_destination,
        target_destination,
        &payload.content,
    );
    log::trace!(
        "RESOURCE_REPRO send_start case={} payload={} source_destination={} target_destination={} message_id={} bytes={} expected={}",
        case_label,
        payload.label,
        source_destination,
        target_destination,
        message_id,
        payload.content.len(),
        if payload.expect_resource { "resource" } else { "packet" }
    );

    let response = rpc_call(sender_rpc, *request_id, "send_message_v2", Some(params))?;
    ensure_rpc_ok(response, "send_message_v2 (resource repro)")?;
    *request_id = (*request_id).wrapping_add(1);

    let delivered = poll_for_inbound_content(receiver_rpc, &payload.content, timeout, *request_id)?;
    *request_id = (*request_id).wrapping_add(1);
    if !delivered {
        let statuses =
            delivery_trace_statuses(sender_rpc, &message_id, *request_id).unwrap_or_default();
        *request_id = (*request_id).wrapping_add(1);
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!(
                "case '{case_label}' did not deliver message '{message_id}' (trace_statuses={statuses:?})"
            ),
        ));
    }

    let expected_status =
        if payload.expect_resource { "sent: link resource" } else { "sent: link" };
    let trace_contains_status = poll_for_delivery_trace_status(
        sender_rpc,
        &message_id,
        expected_status,
        timeout,
        *request_id,
    )?;
    *request_id = (*request_id).wrapping_add(1);
    let statuses = delivery_trace_statuses(sender_rpc, &message_id, *request_id)?;
    *request_id = (*request_id).wrapping_add(1);
    if !trace_contains_status {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!(
                "case '{case_label}' delivered but trace did not contain '{expected_status}' (trace_statuses={statuses:?})"
            ),
        ));
    }

    log::trace!(
        "RESOURCE_REPRO receiver_import case={} message_id={} outcome=delivered trace_statuses={:?}",
        case_label, message_id, statuses
    );
    Ok(())
}

fn build_direct_send_params(
    message_id: &str,
    source: &str,
    destination: &str,
    content: &str,
) -> serde_json::Value {
    let mut params = build_send_params(message_id, source, destination, content);
    if let Some(object) = params.as_object_mut() {
        object.insert("method".to_string(), serde_json::json!(mode_label(DeliveryMode::Direct)));
    }
    params
}

fn mode_label(mode: DeliveryMode) -> &'static str {
    match mode {
        DeliveryMode::Direct => "direct",
        DeliveryMode::Opportunistic => "opportunistic",
        DeliveryMode::Propagated => "propagated",
        DeliveryMode::Paper => "paper",
    }
}

fn poll_for_delivery_trace_status(
    rpc: &str,
    message_id: &str,
    expected_status: &str,
    timeout: Duration,
    mut request_id: u64,
) -> io::Result<bool> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let statuses = delivery_trace_statuses(rpc, message_id, request_id)?;
        request_id = request_id.wrapping_add(1);
        if statuses.iter().any(|status| status.contains(expected_status)) {
            return Ok(true);
        }
        if std::time::Instant::now() >= deadline {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn delivery_trace_statuses(
    rpc: &str,
    message_id: &str,
    request_id: u64,
) -> io::Result<Vec<String>> {
    let response = rpc_call(
        rpc,
        request_id,
        "message_delivery_trace",
        Some(serde_json::json!({ "message_id": message_id })),
    )?;
    let result = ensure_rpc_ok(response, "message_delivery_trace")?;
    Ok(result
        .and_then(|value| value.get("transitions").cloned())
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|transition| {
            transition.get("status").and_then(|value| value.as_str()).map(str::to_owned)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_repro_payloads_cover_packet_boundary_and_large_resource() {
        let payloads = resource_repro_payloads(1024);

        assert_eq!(payloads.len(), 3);
        assert_eq!(payloads[0].label, "packet");
        assert!(payloads[0].content.len() < payloads[1].content.len());
        assert_eq!(payloads[1].label, "resource-threshold");
        assert_eq!(payloads[2].label, "resource-large");
        assert!(payloads[2].content.len() >= 1024);
    }

    #[test]
    fn shared_tcp_client_config_points_both_nodes_at_same_server() {
        let config = build_shared_tcp_client_config("134.122.46.48", 37428);

        assert!(config.contains("type = \"tcp_client\""));
        assert!(config.contains("host = \"134.122.46.48\""));
        assert!(config.contains("port = 37428"));
    }
}
