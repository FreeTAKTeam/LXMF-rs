#[test]
#[ignore = "requires local Python Reticulum/LXMF repos and daemon runtime"]
fn python_shared_instance_queued_direct_recovers_after_upstream_restart_e2e() {
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
    let helper_script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("support")
        .join("python_lxmf_endpoint.py");

    assert!(Path::new(&reticulum_repo).exists(), "reticulum repo not found: {reticulum_repo}");
    assert!(Path::new(&lxmf_repo).exists(), "lxmf repo not found: {lxmf_repo}");
    assert!(helper_script.exists(), "python helper script not found: {}", helper_script.display());

    let temp = tempfile::tempdir().expect("tempdir");
    let shared_a = ReservedPort::reserve();
    let relay_a_rpc = ReservedPort::reserve();
    let relay_a_transport = ReservedPort::reserve();
    let relay_b_rpc = ReservedPort::reserve();
    let relay_b_transport = ReservedPort::reserve();
    let python_control_a = ReservedPort::reserve();
    let python_control_b = ReservedPort::reserve();
    let rns_rpc_a = ReservedPort::reserve();
    let shared_a_port = shared_a.port();
    let relay_a_rpc_port = relay_a_rpc.port();
    let relay_a_transport_port = relay_a_transport.port();
    let relay_b_rpc_port = relay_b_rpc.port();
    let relay_b_transport_port = relay_b_transport.port();
    let python_control_a_port = python_control_a.port();
    let python_control_b_port = python_control_b.port();
    let rns_rpc_a_port = rns_rpc_a.port();

    let relay_a_dir = temp.path().join("rust-direct-restart-relay-a");
    let relay_b_dir = temp.path().join("rust-direct-restart-relay-b");
    let python_storage_a = temp.path().join("python-direct-restart-peer-a-storage");
    let python_storage_b = temp.path().join("python-direct-restart-peer-b-storage");
    let python_rns_a = temp.path().join("python-direct-restart-peer-a-rns");
    let python_rns_b = temp.path().join("python-direct-restart-peer-b-rns");
    write_python_shared_instance_rns_config_with_control_port(
        &python_rns_a,
        shared_a_port,
        rns_rpc_a_port,
    );
    write_python_client_rns_config(&python_rns_b, relay_b_transport_port);
    write_rust_config(
        &relay_a_dir,
        &format!(
            "{}\n[reticulum]\nenable_transport = true\n",
            rust_node_config(
                "rust-direct-restart-relay-a",
                relay_a_rpc_port,
                Some(relay_a_transport_port),
                &[local_client_interface("python-direct-restart-a", shared_a_port)],
            )
        ),
    );
    write_rust_config(
        &relay_b_dir,
        &format!(
            "{}\n[reticulum]\nenable_transport = true\n",
            rust_node_config(
                "rust-direct-restart-relay-b",
                relay_b_rpc_port,
                Some(relay_b_transport_port),
                &[tcp_client_interface("relay-a-uplink", relay_a_transport_port)],
            )
        ),
    );

    let mut python_peer_a = Some(spawn_python_endpoint(
        &python_bin,
        &reticulum_repo,
        &lxmf_repo,
        &helper_script,
        "python-direct-restart-peer-a",
        "Python DIRECT restart peer A",
        &python_rns_a,
        &python_storage_a,
        python_control_a_port,
        &mut [shared_a, python_control_a, rns_rpc_a],
    ));
    let mut python_peer_b = None;
    let mut relay_a = None;
    let mut relay_b = None;

    let outcome: Result<(), String> = (|| {
        wait_for_python_endpoint_ready(
            python_control_a_port,
            python_peer_a.as_mut().expect("Python peer A"),
            "python-direct-restart-peer-a",
        )?;
        let hash_a = python_control_call(python_control_a_port, "status", None)?
            .get("delivery_destination_hash")
            .and_then(Value::as_str)
            .filter(|hash| !hash.is_empty())
            .ok_or_else(|| "Python peer A has no delivery destination hash".to_string())?
            .to_string();

        relay_a = Some(spawn_lxmd(
            &lxmd_bin,
            &reticulumd_bin,
            relay_a_rpc_port,
            &relay_a_dir,
            &mut [relay_a_rpc, relay_a_transport],
        ));
        wait_for_ready(
            relay_a_rpc_port,
            relay_a.as_mut().expect("Rust relay A"),
            "rust-direct-restart-relay-a",
        )?;
        require_attached_local_client(relay_a_rpc_port, "Rust relay A")?;

        relay_b = Some(spawn_lxmd(
            &lxmd_bin,
            &reticulumd_bin,
            relay_b_rpc_port,
            &relay_b_dir,
            &mut [relay_b_rpc, relay_b_transport],
        ));
        wait_for_ready(
            relay_b_rpc_port,
            relay_b.as_mut().expect("Rust relay B"),
            "rust-direct-restart-relay-b",
        )?;

        python_peer_b = Some(spawn_python_endpoint(
            &python_bin,
            &reticulum_repo,
            &lxmf_repo,
            &helper_script,
            "python-direct-restart-peer-b",
            "Python DIRECT restart peer B",
            &python_rns_b,
            &python_storage_b,
            python_control_b_port,
            &mut [python_control_b],
        ));
        wait_for_python_endpoint_ready(
            python_control_b_port,
            python_peer_b.as_mut().expect("Python peer B"),
            "python-direct-restart-peer-b",
        )?;
        let hash_b = python_control_call(python_control_b_port, "status", None)?
            .get("delivery_destination_hash")
            .and_then(Value::as_str)
            .filter(|hash| !hash.is_empty())
            .ok_or_else(|| "Python peer B has no delivery destination hash".to_string())?
            .to_string();

        for port in [python_control_a_port, python_control_b_port] {
            python_control_call(port, "announce", None)?;
        }
        python_control_call(
            python_control_a_port,
            "wait_path",
            Some(json!({ "destination": &hash_b, "timeout": 30.0 })),
        )?;
        python_control_call(
            python_control_b_port,
            "wait_path",
            Some(json!({ "destination": &hash_a, "timeout": 30.0 })),
        )?;
        wait_for_known_path_without_announce(relay_a_rpc_port, &hash_b)?;
        wait_for_known_path_without_announce(relay_b_rpc_port, &hash_a)?;

        // Preserve the Python sender's recalled destination identity, but force
        // the outbound DIRECT message to queue while its upstream relay is down.
        terminate_child(&mut relay_a.as_mut().expect("Rust relay A").child);
        let payload = "queued-direct-across-upstream-relay-restart";
        let queued = python_control_call(
            python_control_a_port,
            "send_message",
            Some(json!({
                "destination": &hash_b,
                "title": "",
                "content": payload,
                "wait_for_path": false,
                "method": "direct"
            })),
        )?;
        let message_hash = queued
            .get("message_hash")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("Python peer A returned no DIRECT message hash: {queued}"))?
            .to_string();
        let initial_status = python_control_call(
            python_control_a_port,
            "outbound_status",
            Some(json!({ "message_hash": &message_hash })),
        )?;
        if matches!(
            initial_status.get("state_name").and_then(Value::as_str),
            Some("delivered" | "rejected" | "cancelled" | "failed")
        ) {
            return Err(format!(
                "DIRECT message became terminal before the upstream relay restarted: {initial_status}"
            ));
        }

        for suffix in ["", "-wal", "-shm"] {
            let route_store = relay_a_dir
                .join("state")
                .join(format!("reticulum.db{suffix}"));
            if route_store.exists() {
                fs::remove_file(&route_store).map_err(|error| {
                    format!("could not clear relay A route state {}: {error}", route_store.display())
                })?;
            }
        }
        relay_a = Some(spawn_lxmd(
            &lxmd_bin,
            &reticulumd_bin,
            relay_a_rpc_port,
            &relay_a_dir,
            &mut [],
        ));
        wait_for_ready(
            relay_a_rpc_port,
            relay_a.as_mut().expect("restarted Rust relay A"),
            "rust-direct-restart-relay-a-after-restart",
        )?;
        require_attached_local_client(relay_a_rpc_port, "Rust relay A after restart")?;
        let restarted_interfaces = rpc_call(relay_a_rpc_port, "list_interfaces", None)?;
        let restored_paths = restarted_interfaces
            .get("interfaces")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("relay A returned no interfaces after restart: {restarted_interfaces}"))?
            .iter()
            .filter_map(|interface| interface.get("settings"))
            .filter_map(|settings| settings.get("_runtime"))
            .filter_map(|runtime| runtime.get("reticulum"))
            .filter_map(|reticulum| reticulum.get("path_table_restore"))
            .filter_map(|restore| restore.get("restored_active_paths"))
            .filter_map(Value::as_u64)
            .sum::<u64>();
        if restored_paths != 0 {
            return Err(format!("relay A restored {restored_paths} routes after clearing route state"));
        }
        wait_for_connected_tcp_client(relay_b_rpc_port, "relay-a-uplink")?;

        for port in [python_control_a_port, python_control_b_port] {
            python_control_call(port, "announce", None)?;
        }
        for (sender, destination) in [
            (python_control_a_port, &hash_b),
            (python_control_b_port, &hash_a),
        ] {
            python_control_call(
                sender,
                "request_path",
                Some(json!({ "destination": destination })),
            )?;
            python_control_call(
                sender,
                "wait_path",
                Some(json!({ "destination": destination, "timeout": 30.0 })),
            )?;
        }
        wait_for_known_path_without_announce(relay_a_rpc_port, &hash_b)?;

        python_control_call(
            python_control_b_port,
            "wait_message",
            Some(json!({ "content": payload, "timeout": 60.0 })),
        )?;
        let delivered = python_control_call(
            python_control_a_port,
            "wait_outbound_state",
            Some(json!({
                "message_hash": &message_hash,
                "state": "delivered",
                "timeout": 60.0
            })),
        )?;
        if delivered.get("state_name").and_then(Value::as_str) != Some("delivered") {
            return Err(format!("DIRECT outbound did not reach terminal DELIVERED: {delivered}"));
        }
        let inbox = python_control_call(python_control_b_port, "list_messages", None)?;
        let matches = inbox
            .get("messages")
            .and_then(Value::as_array)
            .map(|messages| {
                messages
                    .iter()
                    .filter(|message| message.get("content").and_then(Value::as_str) == Some(payload))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if matches.len() != 1 {
            return Err(format!(
                "expected exactly one receiver copy of {payload:?}, found {}: {inbox}",
                matches.len()
            ));
        }
        Ok(())
    })();

    let failure_details = if let Err(error) = &outcome {
        Some(format!(
            "{error}\n\n{}\n\n{}\n\n{}",
            collect_python_endpoint_diagnostics(
                "python-direct-restart-peer-a",
                python_control_a_port,
                python_peer_a.as_mut(),
            ),
            collect_python_endpoint_diagnostics(
                "python-direct-restart-peer-b",
                python_control_b_port,
                python_peer_b.as_mut(),
            ),
            collect_node_diagnostics(
                "rust-direct-restart-relay-a",
                relay_a_rpc_port,
                relay_a.as_mut(),
            ),
        ))
    } else {
        None
    };

    if let Some(node) = relay_a.as_mut() {
        terminate_child(&mut node.child);
    }
    if let Some(node) = relay_b.as_mut() {
        terminate_child(&mut node.child);
    }
    if let Some(node) = python_peer_a.as_mut() {
        terminate_child(&mut node.child);
    }
    if let Some(node) = python_peer_b.as_mut() {
        terminate_child(&mut node.child);
    }

    if let Some(details) = failure_details {
        panic!("queued DIRECT message relay-restart flow failed:\n{details}");
    }
}
