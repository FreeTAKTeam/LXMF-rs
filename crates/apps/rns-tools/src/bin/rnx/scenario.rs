use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use crate::{harness, DeliveryMode};
use harness::{
    cleanup_child, derive_preferred_transport_port, ensure_rpc_ok, poll_for_inbound_content,
    poll_for_peer, reserve_port, rpc_call, spawn_daemon, wait_for_ready,
};
use rns_rpc::e2e_harness::{build_send_params, build_tcp_client_config, timestamp_millis};
use rns_rpc::rpc::replay::{execute_trace, load_trace_file, save_capture_file};

pub(crate) fn run_replay(
    trace: PathBuf,
    capture_out: Option<PathBuf>,
    identity_hash: String,
) -> io::Result<()> {
    let trace_data = load_trace_file(&trace).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("failed to load replay trace '{}': {error}", trace.display()),
        )
    })?;
    let daemon = rns_rpc::RpcDaemon::test_instance_with_identity(identity_hash.as_str());
    let capture = execute_trace(&daemon, &trace_data).map_err(io::Error::other)?;
    if let Some(path) = capture_out {
        save_capture_file(&path, &capture).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("failed to write replay capture '{}': {error}", path.display()),
            )
        })?;
    }
    println!(
        "REPLAY ok: trace='{}' steps={} digest={}",
        capture.trace_name, capture.steps_executed, capture.response_digest_sha256
    );
    Ok(())
}

pub(crate) fn run_e2e(
    a_port: u16,
    b_port: u16,
    timeout_secs: u64,
    keep: bool,
    modes: Vec<DeliveryMode>,
) -> io::Result<()> {
    let timeout = Duration::from_secs(timeout_secs);
    let selected_modes = selected_delivery_modes(&modes);
    let propagation_enabled = selected_modes.contains(&DeliveryMode::Propagated);
    let mut reserved_ports = HashSet::new();
    let a_rpc_listener = reserve_port(a_port, &reserved_ports)?;
    let a_rpc_port = a_rpc_listener.local_addr()?.port();
    reserved_ports.insert(a_rpc_port);
    let b_rpc_listener = reserve_port(b_port, &reserved_ports)?;
    let b_rpc_port = b_rpc_listener.local_addr()?.port();
    reserved_ports.insert(b_rpc_port);

    let a_transport_listener =
        reserve_port(derive_preferred_transport_port(a_rpc_port, 100)?, &reserved_ports)?;
    let a_transport_port = a_transport_listener.local_addr()?.port();
    reserved_ports.insert(a_transport_port);
    let b_transport_listener =
        reserve_port(derive_preferred_transport_port(b_rpc_port, 100)?, &reserved_ports)?;
    let b_transport_port = b_transport_listener.local_addr()?.port();

    let a_rpc = format!("127.0.0.1:{}", a_rpc_port);
    let b_rpc = format!("127.0.0.1:{}", b_rpc_port);
    let a_transport = format!("127.0.0.1:{}", a_transport_port);
    let b_transport = format!("127.0.0.1:{}", b_transport_port);

    let a_dir = tempfile::TempDir::new()?;
    let b_dir = tempfile::TempDir::new()?;
    let a_db = a_dir.path().join("reticulum.db");
    let b_db = b_dir.path().join("reticulum.db");
    let a_config = a_dir.path().join("reticulum.toml");
    let b_config = b_dir.path().join("reticulum.toml");

    fs::write(&a_config, build_tcp_client_config("127.0.0.1", b_transport_port))?;
    fs::write(&b_config, build_tcp_client_config("127.0.0.1", a_transport_port))?;

    drop(a_rpc_listener);
    drop(a_transport_listener);
    let mut a_child = spawn_daemon(&a_rpc, &a_db, &a_transport, &a_config, propagation_enabled)?;
    let a_ready = wait_for_ready(
        a_child.stdout.take().ok_or_else(|| io::Error::other("missing daemon stdout"))?,
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
    drop(b_transport_listener);
    let mut b_child = spawn_daemon(&b_rpc, &b_db, &b_transport, &b_config, propagation_enabled)?;
    let b_ready = wait_for_ready(
        b_child.stdout.take().ok_or_else(|| io::Error::other("missing daemon stdout"))?,
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
    eprintln!(
        "[rnx] ready A delivery={:?} propagation={:?}; B delivery={:?} propagation={:?}",
        a_ready.delivery_hash,
        a_ready.propagation_hash,
        b_ready.delivery_hash,
        b_ready.propagation_hash
    );

    let mut req_id = 1u64;
    rpc_call(&b_rpc, req_id, "announce_now", None)?;
    req_id = req_id.wrapping_add(1);
    let b_delivery_hash = b_ready
        .delivery_hash
        .clone()
        .ok_or_else(|| io::Error::other("daemon B did not report delivery destination hash"))?;
    let a_sees_b = poll_for_peer(&a_rpc, &b_delivery_hash, timeout, req_id)?;
    if !a_sees_b {
        cleanup_child(&mut a_child, keep);
        cleanup_child(&mut b_child, keep);
        return Err(io::Error::new(io::ErrorKind::TimedOut, "daemon A did not discover daemon B"));
    }
    req_id = req_id.wrapping_add(1);

    rpc_call(&a_rpc, req_id, "announce_now", None)?;
    req_id = req_id.wrapping_add(1);
    let a_delivery_hash = a_ready
        .delivery_hash
        .clone()
        .ok_or_else(|| io::Error::other("daemon A did not report delivery destination hash"))?;
    let b_sees_a = poll_for_peer(&b_rpc, &a_delivery_hash, timeout, req_id)?;
    if !b_sees_a {
        cleanup_child(&mut a_child, keep);
        cleanup_child(&mut b_child, keep);
        return Err(io::Error::new(io::ErrorKind::TimedOut, "daemon B did not discover daemon A"));
    }
    req_id = req_id.wrapping_add(1);
    std::thread::sleep(Duration::from_millis(1500));

    if propagation_enabled {
        let a_propagation_hash = a_ready.propagation_hash.clone().ok_or_else(|| {
            io::Error::other("daemon A did not report propagation destination hash")
        })?;
        let b_propagation_hash = b_ready.propagation_hash.clone().ok_or_else(|| {
            io::Error::other("daemon B did not report propagation destination hash")
        })?;

        rpc_call(&b_rpc, req_id, "announce_now", None)?;
        req_id = req_id.wrapping_add(1);
        rpc_call(&a_rpc, req_id, "announce_now", None)?;
        req_id = req_id.wrapping_add(1);

        let a_select_response = rpc_call(
            &a_rpc,
            req_id,
            "set_outbound_propagation_node",
            Some(serde_json::json!({ "peer": b_propagation_hash })),
        )?;
        ensure_rpc_ok(a_select_response, "set_outbound_propagation_node (A)")?;
        req_id = req_id.wrapping_add(1);

        let b_select_response = rpc_call(
            &b_rpc,
            req_id,
            "set_outbound_propagation_node",
            Some(serde_json::json!({ "peer": a_propagation_hash })),
        )?;
        ensure_rpc_ok(b_select_response, "set_outbound_propagation_node (B)")?;
        req_id = req_id.wrapping_add(1);
        std::thread::sleep(Duration::from_millis(1500));
    }

    for mode in selected_modes {
        match mode {
            DeliveryMode::Direct | DeliveryMode::Opportunistic | DeliveryMode::Propagated => {
                run_delivery_mode(
                    mode,
                    &a_rpc,
                    &b_rpc,
                    &a_delivery_hash,
                    &b_delivery_hash,
                    timeout,
                    &mut req_id,
                )?;
                run_delivery_mode(
                    mode,
                    &b_rpc,
                    &a_rpc,
                    &b_delivery_hash,
                    &a_delivery_hash,
                    timeout,
                    &mut req_id,
                )?;
            }
            DeliveryMode::Paper => {
                run_paper_workflow(
                    &a_rpc,
                    &b_rpc,
                    &a_delivery_hash,
                    &b_delivery_hash,
                    timeout,
                    &mut req_id,
                )?;
            }
        }
    }

    cleanup_child(&mut a_child, keep);
    cleanup_child(&mut b_child, keep);
    println!("E2E ok: peer discovery A<->B succeeded");
    println!("E2E ok: compatibility delivery modes completed");
    Ok(())
}

pub(crate) fn selected_mesh_delivery_modes(modes: &[DeliveryMode]) -> Vec<DeliveryMode> {
    if modes.is_empty() {
        return vec![DeliveryMode::Direct];
    }
    selected_delivery_modes(modes)
}

fn selected_delivery_modes(modes: &[DeliveryMode]) -> Vec<DeliveryMode> {
    if modes.is_empty() {
        return vec![DeliveryMode::Direct, DeliveryMode::Opportunistic, DeliveryMode::Propagated];
    }
    let mut selected = Vec::new();
    let mut seen = HashSet::new();
    for mode in modes {
        if seen.insert(*mode) {
            selected.push(*mode);
        }
    }
    selected
}

fn mode_label(mode: DeliveryMode) -> &'static str {
    match mode {
        DeliveryMode::Direct => "direct",
        DeliveryMode::Opportunistic => "opportunistic",
        DeliveryMode::Propagated => "propagated",
        DeliveryMode::Paper => "paper",
    }
}

fn build_mode_send_params(
    message_id: &str,
    source: &str,
    destination: &str,
    content: &str,
    mode: DeliveryMode,
) -> serde_json::Value {
    let mut params = build_send_params(message_id, source, destination, content);
    if let Some(object) = params.as_object_mut() {
        object.insert("method".to_string(), serde_json::json!(mode_label(mode)));
        if matches!(mode, DeliveryMode::Propagated) {
            object.insert("include_ticket".to_string(), serde_json::json!(true));
            object.insert("try_propagation_on_fail".to_string(), serde_json::json!(true));
            object.insert("stamp_cost".to_string(), serde_json::json!(8));
        }
    }
    params
}

pub(crate) fn run_delivery_mode(
    mode: DeliveryMode,
    sender_rpc: &str,
    receiver_rpc: &str,
    sender_destination: &str,
    receiver_destination: &str,
    timeout: Duration,
    request_id: &mut u64,
) -> io::Result<()> {
    let label = mode_label(mode);
    let content = format!("hello from rnx e2e ({label})");
    let max_attempts = if matches!(mode, DeliveryMode::Direct) { 4 } else { 1 };

    for attempt in 1..=max_attempts {
        if matches!(mode, DeliveryMode::Direct) {
            rpc_call(receiver_rpc, *request_id, "announce_now", None)?;
            *request_id = (*request_id).wrapping_add(1);
            rpc_call(sender_rpc, *request_id, "announce_now", None)?;
            *request_id = (*request_id).wrapping_add(1);
            std::thread::sleep(Duration::from_millis(750));
        }

        let message_id = format!("e2e-{}-{}", label, timestamp_millis());
        let params = build_mode_send_params(
            &message_id,
            sender_destination,
            receiver_destination,
            &content,
            mode,
        );
        let response = rpc_call(sender_rpc, *request_id, "send_message_v2", Some(params))?;
        ensure_rpc_ok(response, format!("send_message_v2 ({label})").as_str())?;
        *request_id = (*request_id).wrapping_add(1);

        let delivered = poll_for_inbound_content(receiver_rpc, &content, timeout, *request_id)?;
        if !delivered {
            let trace_statuses = delivery_trace_statuses(sender_rpc, &message_id, *request_id)
                .unwrap_or_else(|_| Vec::new());
            if attempt < max_attempts {
                *request_id = (*request_id).wrapping_add(1);
                std::thread::sleep(Duration::from_millis(2000));
                continue;
            }
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "delivery mode '{label}' did not deliver message '{message_id}' (trace_statuses={trace_statuses:?})"
                ),
            ));
        }
        *request_id = (*request_id).wrapping_add(1);

        let trace_contains_status =
            poll_for_delivery_trace_status(sender_rpc, &message_id, label, timeout, *request_id)?;
        if !trace_contains_status {
            if attempt < max_attempts {
                *request_id = (*request_id).wrapping_add(1);
                std::thread::sleep(Duration::from_millis(2000));
                continue;
            }
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("delivery trace for '{message_id}' did not contain mode '{label}'"),
            ));
        }
        *request_id = (*request_id).wrapping_add(1);

        println!("E2E ok: mode={} message {} delivered", label, message_id);
        return Ok(());
    }

    unreachable!("max_attempts is always at least 1")
}

pub(crate) fn run_paper_workflow(
    sender_rpc: &str,
    receiver_rpc: &str,
    sender_destination: &str,
    receiver_destination: &str,
    timeout: Duration,
    request_id: &mut u64,
) -> io::Result<()> {
    let message_id = format!("e2e-paper-{}", timestamp_millis());
    let content = "hello from rnx e2e (paper)";
    let send_params = build_mode_send_params(
        &message_id,
        sender_destination,
        receiver_destination,
        content,
        DeliveryMode::Paper,
    );
    let response = rpc_call(sender_rpc, *request_id, "send_message_v2", Some(send_params))?;
    ensure_rpc_ok(response, "send_message_v2 (paper)")?;
    *request_id = (*request_id).wrapping_add(1);

    let paper_encode_response = rpc_call(
        sender_rpc,
        *request_id,
        "sdk_paper_encode_v2",
        Some(serde_json::json!({ "message_id": message_id })),
    )?;
    let paper_encode_result = ensure_rpc_ok(paper_encode_response, "sdk_paper_encode_v2")?
        .ok_or_else(|| io::Error::other("sdk_paper_encode_v2 missing result body"))?;
    let uri = paper_encode_result
        .get("envelope")
        .and_then(|value| value.get("uri"))
        .and_then(|value| value.as_str())
        .ok_or_else(|| io::Error::other("sdk_paper_encode_v2 missing envelope uri"))?
        .to_string();
    *request_id = (*request_id).wrapping_add(1);

    let paper_decode_response = rpc_call(
        receiver_rpc,
        *request_id,
        "sdk_paper_decode_v2",
        Some(serde_json::json!({ "uri": uri })),
    )?;
    let paper_decode_result = ensure_rpc_ok(paper_decode_response, "sdk_paper_decode_v2")?
        .ok_or_else(|| io::Error::other("sdk_paper_decode_v2 missing result body"))?;
    let accepted =
        paper_decode_result.get("accepted").and_then(|value| value.as_bool()).unwrap_or(false);
    if !accepted {
        return Err(io::Error::other("sdk_paper_decode_v2 returned accepted=false"));
    }
    *request_id = (*request_id).wrapping_add(1);

    let delivered = poll_for_inbound_content(receiver_rpc, content, timeout, *request_id)?;
    if !delivered {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "paper workflow did not deliver decoded message",
        ));
    }
    *request_id = (*request_id).wrapping_add(1);

    println!("E2E ok: mode=paper message {} encoded/decoded", message_id);
    Ok(())
}

fn poll_for_delivery_trace_status(
    rpc: &str,
    message_id: &str,
    expected_mode: &str,
    timeout: Duration,
    mut request_id: u64,
) -> io::Result<bool> {
    let deadline = std::time::Instant::now() + timeout;
    let expected_statuses = expected_delivery_trace_statuses(expected_mode);
    loop {
        let response = rpc_call(
            rpc,
            request_id,
            "message_delivery_trace",
            Some(serde_json::json!({ "message_id": message_id })),
        )?;
        request_id = request_id.wrapping_add(1);
        let result = ensure_rpc_ok(response, "message_delivery_trace")?;
        let has_expected_status = result
            .and_then(|value| value.get("transitions").cloned())
            .and_then(|value| value.as_array().cloned())
            .map(|transitions| {
                transitions.iter().any(|transition| {
                    transition.get("status").and_then(|value| value.as_str()).is_some_and(
                        |status| {
                            expected_statuses
                                .iter()
                                .any(|expected_status| status.contains(expected_status))
                        },
                    )
                })
            })
            .unwrap_or(false);
        if has_expected_status {
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

fn expected_delivery_trace_statuses(expected_mode: &str) -> &'static [&'static str] {
    match expected_mode {
        "direct" => &["sent: direct", "sent: link"],
        "opportunistic" => &["sent: opportunistic"],
        "propagated" => &["sent: propagated", "sent: propagated resource", "delivered"],
        other => panic!("unsupported delivery mode '{other}'"),
    }
}
