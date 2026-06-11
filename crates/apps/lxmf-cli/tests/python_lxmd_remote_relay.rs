mod support;

use serde_json::{json, Value};
use std::env;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use support::python_lxmd_remote_relay::*;

const REMOTE_PATH_RESPONSE_MIN: Duration = Duration::from_millis(900);

#[test]
#[ignore = "requires local Python Reticulum/LXMF repos and daemon runtime"]
fn rust_to_python_lxmd_relay_remote_path_e2e() {
    let lxmd_bin = resolve_test_binary("lxmd", option_env!("CARGO_BIN_EXE_lxmd"));
    let reticulumd_bin = resolve_test_binary("reticulumd", option_env!("CARGO_BIN_EXE_reticulumd"));
    let workspace_root =
        Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(3).expect("workspace root");

    let python_bin = env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let reticulum_repo = env::var("RETICULUM_PY_REPO").unwrap_or_else(|_| {
        workspace_root.parent().expect("workspace parent").join("reticulum").display().to_string()
    });
    let lxmf_repo = env::var("LXMF_PY_REPO").unwrap_or_else(|_| {
        workspace_root.parent().expect("workspace parent").join("lxmf").display().to_string()
    });

    assert!(Path::new(&reticulum_repo).exists(), "reticulum repo not found: {reticulum_repo}");
    assert!(Path::new(&lxmf_repo).exists(), "lxmf repo not found: {lxmf_repo}");

    let temp = tempfile::tempdir().expect("tempdir");

    let upstream_server_port = ReservedPort::reserve();
    let upstream_server_port_num = upstream_server_port.port();
    let sender_rpc = ReservedPort::reserve();
    let sender_transport = ReservedPort::reserve();
    let rust_relay_rpc = ReservedPort::reserve();
    let rust_relay_transport = ReservedPort::reserve();
    let recipient_rpc = ReservedPort::reserve();
    let recipient_transport = ReservedPort::reserve();

    let python_lxmd_dir = temp.path().join("python-relay-lxmd");
    let python_rns_dir = temp.path().join("python-relay-rns");
    let sender_dir = temp.path().join("rust-sender");
    let rust_relay_dir = temp.path().join("rust-relay");
    let recipient_dir = temp.path().join("rust-recipient");

    write_python_lxmd_config(&python_lxmd_dir, "Python Relay");
    write_python_rns_config(&python_rns_dir, upstream_server_port_num);
    write_rust_config(
        &sender_dir,
        &rust_node_config(
            "rust-sender",
            sender_rpc.port(),
            Some(sender_transport.port()),
            &[tcp_client_interface("sender-uplink", upstream_server_port_num)],
        ),
    );
    write_rust_config(
        &rust_relay_dir,
        &rust_node_config(
            "rust-relay",
            rust_relay_rpc.port(),
            Some(rust_relay_transport.port()),
            &[tcp_client_interface("relay-uplink", upstream_server_port_num)],
        ),
    );
    write_rust_config(
        &recipient_dir,
        &rust_node_config(
            "rust-recipient",
            recipient_rpc.port(),
            Some(recipient_transport.port()),
            &[tcp_client_interface("recipient-uplink", rust_relay_transport.port())],
        ),
    );

    let mut python_relay = Some(spawn_python_lxmd_relay(
        &python_bin,
        &reticulum_repo,
        &lxmf_repo,
        &python_lxmd_dir,
        &python_rns_dir,
        &mut [upstream_server_port],
    ));
    let mut sender = None;
    let mut rust_relay = None;
    let mut recipient = None;

    let outcome: Result<(), String> = (|| {
        wait_for_python_port(
            upstream_server_port_num,
            python_relay.as_mut().expect("python relay child"),
            "python-relay",
        )?;

        rust_relay = Some(spawn_lxmd(
            &lxmd_bin,
            &reticulumd_bin,
            rust_relay_rpc.port(),
            &rust_relay_dir,
            &mut [rust_relay_rpc, rust_relay_transport],
        ));
        wait_for_ready(
            rust_relay.as_ref().expect("rust relay child").rpc_port(),
            rust_relay.as_mut().expect("rust relay child"),
            "rust-relay",
        )?;

        recipient = Some(spawn_lxmd(
            &lxmd_bin,
            &reticulumd_bin,
            recipient_rpc.port(),
            &recipient_dir,
            &mut [recipient_rpc, recipient_transport],
        ));
        wait_for_ready(
            recipient.as_ref().expect("recipient child").rpc_port(),
            recipient.as_mut().expect("recipient child"),
            "rust-recipient",
        )?;

        sender = Some(spawn_lxmd(
            &lxmd_bin,
            &reticulumd_bin,
            sender_rpc.port(),
            &sender_dir,
            &mut [sender_rpc, sender_transport],
        ));
        wait_for_ready(
            sender.as_ref().expect("sender child").rpc_port(),
            sender.as_mut().expect("sender child"),
            "rust-sender",
        )?;

        let sender_rpc = sender.as_ref().expect("sender child").rpc_port();
        let recipient_rpc = recipient.as_ref().expect("recipient child").rpc_port();

        let recipient_status = daemon_status(recipient_rpc)?;
        let sender_status = daemon_status(sender_rpc)?;
        let recipient_hash = status_hash(&recipient_status)
            .unwrap_or_else(|| panic!("rust-recipient delivery hash: {recipient_status}"));
        let sender_hash = status_hash(&sender_status)
            .unwrap_or_else(|| panic!("rust-sender delivery hash: {sender_status}"));

        rpc_call(recipient_rpc, "announce_now", None)?;

        let message_id = format!(
            "python-relay-remote-{}",
            SystemTime::now().duration_since(UNIX_EPOCH).expect("time").as_millis()
        );
        let delivery_started_at = Instant::now();
        rpc_call(
            sender_rpc,
            "send_message_v2",
            Some(json!({
                "id": message_id,
                "source": sender_hash,
                "destination": recipient_hash,
                "title": "",
                "content": "hello through python relay",
                "method": "direct"
            })),
        )?;

        wait_for_inbound_message(recipient_rpc, "hello through python relay")?;

        let delivery_elapsed = delivery_started_at.elapsed();
        if delivery_elapsed < REMOTE_PATH_RESPONSE_MIN {
            return Err(format!(
                "python relay remote path response completed too quickly: {:?} < {:?}",
                delivery_elapsed, REMOTE_PATH_RESPONSE_MIN
            ));
        }

        Ok(())
    })();

    let sender_rpc = sender.as_ref().map_or(0, SpawnedNode::rpc_port);
    let rust_relay_rpc = rust_relay.as_ref().map_or(0, SpawnedNode::rpc_port);
    let recipient_rpc = recipient.as_ref().map_or(0, SpawnedNode::rpc_port);

    let failure_details = if let Err(err) = &outcome {
        Some(format!(
            "{err}\n\n{}\n\n{}\n\n{}\n\n{}",
            collect_python_diagnostics("python-relay", python_relay.as_mut()),
            collect_node_diagnostics("rust-sender", sender_rpc, sender.as_mut()),
            collect_node_diagnostics("rust-relay", rust_relay_rpc, rust_relay.as_mut()),
            collect_node_diagnostics("rust-recipient", recipient_rpc, recipient.as_mut()),
        ))
    } else {
        None
    };

    if let Some(node) = sender.as_mut() {
        terminate_child(&mut node.child);
    }
    if let Some(node) = recipient.as_mut() {
        terminate_child(&mut node.child);
    }
    if let Some(node) = rust_relay.as_mut() {
        terminate_child(&mut node.child);
    }
    if let Some(node) = python_relay.as_mut() {
        terminate_child(&mut node.child);
    }

    if let Some(details) = failure_details {
        panic!("python lxmd remote relay flow failed:\n{details}");
    }
}

#[test]
#[ignore = "requires local Python Reticulum/LXMF repos and daemon runtime"]
fn rust_selects_python_lxmd_propagation_node_e2e() {
    let lxmd_bin = resolve_test_binary("lxmd", option_env!("CARGO_BIN_EXE_lxmd"));
    let reticulumd_bin = resolve_test_binary("reticulumd", option_env!("CARGO_BIN_EXE_reticulumd"));
    let workspace_root =
        Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(3).expect("workspace root");

    let python_bin = env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let reticulum_repo = env::var("RETICULUM_PY_REPO").unwrap_or_else(|_| {
        workspace_root.parent().expect("workspace parent").join("reticulum").display().to_string()
    });
    let lxmf_repo = env::var("LXMF_PY_REPO").unwrap_or_else(|_| {
        workspace_root.parent().expect("workspace parent").join("lxmf").display().to_string()
    });

    assert!(Path::new(&reticulum_repo).exists(), "reticulum repo not found: {reticulum_repo}");
    assert!(Path::new(&lxmf_repo).exists(), "lxmf repo not found: {lxmf_repo}");

    let temp = tempfile::tempdir().expect("tempdir");

    let upstream_server_port = ReservedPort::reserve();
    let upstream_server_port_num = upstream_server_port.port();
    let sender_rpc = ReservedPort::reserve();
    let sender_transport = ReservedPort::reserve();

    let python_lxmd_dir = temp.path().join("python-propagation-lxmd");
    let python_rns_dir = temp.path().join("python-propagation-rns");
    let sender_dir = temp.path().join("rust-sender");

    write_python_lxmd_propagation_config(&python_lxmd_dir, "Python Propagation");
    write_python_rns_config(&python_rns_dir, upstream_server_port_num);
    write_rust_config(
        &sender_dir,
        &rust_node_config(
            "rust-sender",
            sender_rpc.port(),
            Some(sender_transport.port()),
            &[tcp_client_interface("sender-uplink", upstream_server_port_num)],
        ),
    );

    let mut python_node = Some(spawn_python_lxmd_relay(
        &python_bin,
        &reticulum_repo,
        &lxmf_repo,
        &python_lxmd_dir,
        &python_rns_dir,
        &mut [upstream_server_port],
    ));
    let mut sender = None;

    let outcome: Result<(), String> = (|| {
        wait_for_python_port(
            upstream_server_port_num,
            python_node.as_mut().expect("python propagation child"),
            "python-propagation-node",
        )?;

        sender = Some(spawn_lxmd(
            &lxmd_bin,
            &reticulumd_bin,
            sender_rpc.port(),
            &sender_dir,
            &mut [sender_rpc, sender_transport],
        ));
        wait_for_ready(
            sender.as_ref().expect("sender child").rpc_port(),
            sender.as_mut().expect("sender child"),
            "rust-sender",
        )?;

        let python_propagation_hash =
            python_destination_hash(&python_bin, &reticulum_repo, &python_lxmd_dir, "propagation")?;
        let sender_rpc = sender.as_ref().expect("sender child").rpc_port();

        let selected = rpc_call(
            sender_rpc,
            "set_outbound_propagation_node",
            Some(json!({ "peer": python_propagation_hash })),
        )?;
        if selected["peer"].as_str() != Some(python_propagation_hash.as_str()) {
            return Err(format!("selected propagation node mismatch: {selected}"));
        }

        let listed = rpc_call(sender_rpc, "list_propagation_nodes", None)?;
        let listed_selected = listed["nodes"].as_array().is_some_and(|nodes| {
            nodes.iter().any(|node| {
                node["peer"].as_str() == Some(python_propagation_hash.as_str())
                    && node["selected"].as_bool() == Some(true)
            })
        });
        if !listed_selected {
            return Err(format!(
                "selected Python propagation node not visible in list_propagation_nodes: {listed}"
            ));
        }

        Ok(())
    })();

    let sender_rpc = sender.as_ref().map_or(0, SpawnedNode::rpc_port);

    let failure_details = if let Err(err) = &outcome {
        Some(format!(
            "{err}\n\n{}\n\n{}",
            collect_python_diagnostics("python-propagation-node", python_node.as_mut()),
            collect_node_diagnostics("rust-sender", sender_rpc, sender.as_mut()),
        ))
    } else {
        None
    };

    if let Some(node) = sender.as_mut() {
        terminate_child(&mut node.child);
    }
    if let Some(node) = python_node.as_mut() {
        terminate_child(&mut node.child);
    }

    if let Some(details) = failure_details {
        panic!("python lxmd propagation node selection failed:\n{details}");
    }
}

#[test]
#[ignore = "requires local Python Reticulum/LXMF repos and daemon runtime"]
fn python_to_rust_lxmd_relay_remote_path_e2e() {
    let lxmd_bin = resolve_test_binary("lxmd", option_env!("CARGO_BIN_EXE_lxmd"));
    let reticulumd_bin = resolve_test_binary("reticulumd", option_env!("CARGO_BIN_EXE_reticulumd"));
    let workspace_root =
        Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(3).expect("workspace root");
    let helper_script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("support")
        .join("python_lxmf_endpoint.py");

    let python_bin = env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let reticulum_repo = env::var("RETICULUM_PY_REPO").unwrap_or_else(|_| {
        workspace_root.parent().expect("workspace parent").join("reticulum").display().to_string()
    });
    let lxmf_repo = env::var("LXMF_PY_REPO").unwrap_or_else(|_| {
        workspace_root.parent().expect("workspace parent").join("lxmf").display().to_string()
    });

    assert!(Path::new(&reticulum_repo).exists(), "reticulum repo not found: {reticulum_repo}");
    assert!(Path::new(&lxmf_repo).exists(), "lxmf repo not found: {lxmf_repo}");
    assert!(helper_script.exists(), "python helper script not found: {}", helper_script.display());

    let temp = tempfile::tempdir().expect("tempdir");

    let upstream_relay_rpc = ReservedPort::reserve();
    let upstream_relay_transport = ReservedPort::reserve();
    let downstream_relay_rpc = ReservedPort::reserve();
    let downstream_relay_transport = ReservedPort::reserve();
    let python_sender_control = ReservedPort::reserve();
    let python_recipient_control = ReservedPort::reserve();

    let upstream_relay_dir = temp.path().join("rust-upstream-relay");
    let downstream_relay_dir = temp.path().join("rust-downstream-relay");
    let python_sender_storage = temp.path().join("python-sender-storage");
    let python_sender_rns = temp.path().join("python-sender-rns");
    let python_recipient_storage = temp.path().join("python-recipient-storage");
    let python_recipient_rns = temp.path().join("python-recipient-rns");

    write_rust_config(
        &upstream_relay_dir,
        &rust_node_config(
            "rust-upstream-relay",
            upstream_relay_rpc.port(),
            Some(upstream_relay_transport.port()),
            &[],
        ),
    );
    write_rust_config(
        &downstream_relay_dir,
        &rust_node_config(
            "rust-downstream-relay",
            downstream_relay_rpc.port(),
            Some(downstream_relay_transport.port()),
            &[tcp_client_interface("downstream-uplink", upstream_relay_transport.port())],
        ),
    );
    write_python_client_rns_config(&python_sender_rns, upstream_relay_transport.port());
    write_python_client_rns_config(&python_recipient_rns, downstream_relay_transport.port());

    let mut upstream_relay = None;
    let mut downstream_relay = None;
    let mut python_sender = None;
    let mut python_recipient = None;

    let outcome: Result<(), String> = (|| {
        upstream_relay = Some(spawn_lxmd(
            &lxmd_bin,
            &reticulumd_bin,
            upstream_relay_rpc.port(),
            &upstream_relay_dir,
            &mut [upstream_relay_rpc, upstream_relay_transport],
        ));
        wait_for_ready(
            upstream_relay.as_ref().expect("upstream relay child").rpc_port(),
            upstream_relay.as_mut().expect("upstream relay child"),
            "rust-upstream-relay",
        )?;

        downstream_relay = Some(spawn_lxmd(
            &lxmd_bin,
            &reticulumd_bin,
            downstream_relay_rpc.port(),
            &downstream_relay_dir,
            &mut [downstream_relay_rpc, downstream_relay_transport],
        ));
        wait_for_ready(
            downstream_relay.as_ref().expect("downstream relay child").rpc_port(),
            downstream_relay.as_mut().expect("downstream relay child"),
            "rust-downstream-relay",
        )?;

        python_recipient = Some(spawn_python_endpoint(
            &python_bin,
            &reticulum_repo,
            &lxmf_repo,
            &helper_script,
            "python-recipient",
            "Python Recipient",
            &python_recipient_rns,
            &python_recipient_storage,
            python_recipient_control.port(),
            &mut [python_recipient_control],
        ));
        wait_for_python_endpoint_ready(
            python_recipient.as_ref().expect("python recipient").control_port,
            python_recipient.as_mut().expect("python recipient"),
            "python-recipient",
        )?;

        python_sender = Some(spawn_python_endpoint(
            &python_bin,
            &reticulum_repo,
            &lxmf_repo,
            &helper_script,
            "python-sender",
            "Python Sender",
            &python_sender_rns,
            &python_sender_storage,
            python_sender_control.port(),
            &mut [python_sender_control],
        ));
        wait_for_python_endpoint_ready(
            python_sender.as_ref().expect("python sender").control_port,
            python_sender.as_mut().expect("python sender"),
            "python-sender",
        )?;

        let sender_control = python_sender.as_ref().expect("python sender").control_port;
        let recipient_control = python_recipient.as_ref().expect("python recipient").control_port;

        let recipient_status = python_control_call(recipient_control, "status", None)?;
        let recipient_hash = recipient_status
            .get("delivery_destination_hash")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("missing python recipient delivery hash: {recipient_status}"))?;

        python_control_call(recipient_control, "announce", None)?;

        let delivery_started_at = Instant::now();
        python_control_call(
            sender_control,
            "send_message",
            Some(json!({
                "destination": recipient_hash,
                "title": "",
                "content": "hello through rust relay",
            })),
        )?;

        wait_for_python_inbound_message(recipient_control, "hello through rust relay")?;

        let delivery_elapsed = delivery_started_at.elapsed();
        if delivery_elapsed < REMOTE_PATH_RESPONSE_MIN {
            return Err(format!(
                "rust relay remote path response completed too quickly: {:?} < {:?}",
                delivery_elapsed, REMOTE_PATH_RESPONSE_MIN
            ));
        }

        Ok(())
    })();

    let upstream_relay_rpc = upstream_relay.as_ref().map_or(0, SpawnedNode::rpc_port);
    let downstream_relay_rpc = downstream_relay.as_ref().map_or(0, SpawnedNode::rpc_port);
    let python_sender_control = python_sender.as_ref().map_or(0, |node| node.control_port);
    let python_recipient_control = python_recipient.as_ref().map_or(0, |node| node.control_port);

    let failure_details = if let Err(err) = &outcome {
        Some(format!(
            "{err}\n\n{}\n\n{}\n\n{}\n\n{}",
            collect_node_diagnostics(
                "rust-upstream-relay",
                upstream_relay_rpc,
                upstream_relay.as_mut()
            ),
            collect_node_diagnostics(
                "rust-downstream-relay",
                downstream_relay_rpc,
                downstream_relay.as_mut()
            ),
            collect_python_endpoint_diagnostics(
                "python-sender",
                python_sender_control,
                python_sender.as_mut(),
            ),
            collect_python_endpoint_diagnostics(
                "python-recipient",
                python_recipient_control,
                python_recipient.as_mut(),
            ),
        ))
    } else {
        None
    };

    if let Some(node) = python_sender.as_mut() {
        terminate_child(&mut node.child);
    }
    if let Some(node) = python_recipient.as_mut() {
        terminate_child(&mut node.child);
    }
    if let Some(node) = downstream_relay.as_mut() {
        terminate_child(&mut node.child);
    }
    if let Some(node) = upstream_relay.as_mut() {
        terminate_child(&mut node.child);
    }

    if let Some(details) = failure_details {
        panic!("python to rust lxmd remote relay flow failed:\n{details}");
    }
}
