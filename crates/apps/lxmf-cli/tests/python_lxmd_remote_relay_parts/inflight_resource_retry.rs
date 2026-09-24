fn begin_gated_direct_resource(
    control_port: u16,
    destination: &str,
) -> Result<(String, Value), String> {
    python_control_call(control_port, "arm_resource_gate", None)?;
    let outbound = python_control_call(
        control_port,
        "send_message",
        Some(json!({
            "destination": destination,
            "title": "in-flight direct resource retry",
            "content_bytes": 1_500_000,
            "wait_for_path": false,
            "method": "direct"
        })),
    )?;
    let message_hash = outbound
        .get("message_hash")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("in-flight DIRECT send returned no message hash: {outbound}"))?
        .to_string();
    let first_resource = match python_control_call(
        control_port,
        "wait_resource_gate",
        Some(json!({ "timeout": 120.0 })),
    ) {
        Ok(observation) => observation,
        Err(error) => {
            let status = python_control_call(
                control_port,
                "outbound_status",
                Some(json!({ "message_hash": message_hash })),
            );
            let gate = python_control_call(control_port, "resource_gate_snapshot", None);
            return Err(format!(
                "DIRECT LXMF Resource gate failed: {error}; outbound={status:?}; gate={gate:?}"
            ));
        }
    };
    let sent_parts = first_resource
        .get("sent_parts")
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("gated Resource has no sent_parts count: {first_resource}"))?;
    let total_parts = first_resource
        .get("total_parts")
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("gated Resource has no total_parts count: {first_resource}"))?;
    if sent_parts == 0 || sent_parts >= total_parts {
        return Err(format!(
            "first DIRECT Resource did not stop after starting but before completion: {first_resource}"
        ));
    }
    Ok((message_hash, first_resource))
}

fn verify_direct_resource_retry(
    sender_control_port: u16,
    receiver_control_port: u16,
    message_hash: &str,
    first_resource: &Value,
    timeout: f64,
) -> Result<(), String> {
    let delivered_status = python_control_call(
        sender_control_port,
        "wait_outbound_state",
        Some(json!({
            "message_hash": message_hash,
            "state": "delivered",
            "timeout": timeout
        })),
    )?;

    let received = python_control_call(
        receiver_control_port,
        "wait_message_prefix",
        Some(json!({
            "prefix": "LXMF-RS-INFLIGHT-RESOURCE-Python shared peer A:",
            "timeout": timeout
        })),
    )?;
    if received.get("count").and_then(Value::as_u64) != Some(1) {
        return Err(format!("retried DIRECT message was not received exactly once: {received}"));
    }

    let snapshot = python_control_call(sender_control_port, "resource_gate_snapshot", None)?;
    let observations = snapshot
        .get("observations")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("Resource gate returned no observation array: {snapshot}"))?;
    let first_key = (
        first_resource.get("resource_id").and_then(Value::as_str),
        first_resource.get("link_id").and_then(Value::as_str),
    );
    let has_first = observations.iter().any(|observation| {
        (
            observation.get("resource_id").and_then(Value::as_str),
            observation.get("link_id").and_then(Value::as_str),
        ) == first_key
    });
    let retry_key = (
        delivered_status.get("resource_id").and_then(Value::as_str),
        delivered_status.get("link_id").and_then(Value::as_str),
    );
    if !has_first || retry_key.0.is_none() || retry_key.1.is_none()
        || retry_key.0 == first_key.0
        || retry_key.1 == first_key.1
    {
        return Err(format!(
            "DIRECT LXMF delivery did not retry with both a new Resource and Link; first={first_resource}, delivered={delivered_status}, observations={snapshot}"
        ));
    }

    let content = python_control_call(receiver_control_port, "list_messages", None)?;
    let matching = content
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|message| {
            message
                .get("content")
                .and_then(Value::as_str)
                .is_some_and(|value| value.starts_with("LXMF-RS-INFLIGHT-RESOURCE-Python shared peer A:"))
        })
        .count();
    if matching != 1 {
        let inbox_summary = content
            .get("messages")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|message| {
                let body = message.get("content").and_then(Value::as_str).unwrap_or("");
                (
                    message.get("title").and_then(Value::as_str).unwrap_or(""),
                    body.chars().take(72).collect::<String>(),
                    body.len(),
                )
            })
            .collect::<Vec<_>>();
        return Err(format!(
            "retried DIRECT LXMF message was delivered {matching} times instead of exactly once; inbox summary={inbox_summary:?}"
        ));
    }
    Ok(())
}

fn exercise_inflight_resource_restart(
    lxmd_bin: &Path,
    reticulumd_bin: &Path,
    relay_a_rpc_port: u16,
    relay_a_dir: &Path,
    relay_a: &mut Option<SpawnedNode>,
    relay_b_rpc_port: u16,
    python_control_a_port: u16,
    python_control_b_port: u16,
    hash_a: &str,
    hash_b: &str,
) -> Result<(), String> {
    python_control_call(
        python_control_b_port,
        "set_router_resource_callback_chaining",
        Some(json!({ "enabled": true })),
    )?;
    let (message_hash, first_resource) = begin_gated_direct_resource(python_control_a_port, hash_b)?;
    terminate_child(&mut relay_a.as_mut().expect("Rust relay A").child);
    python_control_call(python_control_a_port, "release_resource_gate", None)?;

    for suffix in ["", "-wal", "-shm"] {
        let route_store = relay_a_dir.join("state").join(format!("reticulum.db{suffix}"));
        if route_store.exists() {
            fs::remove_file(&route_store)
                .map_err(|error| format!("could not clear relay A route state: {error}"))?;
        }
    }
    *relay_a = Some(spawn_lxmd(
        lxmd_bin,
        reticulumd_bin,
        relay_a_rpc_port,
        relay_a_dir,
        &mut [],
    ));
    wait_for_ready(
        relay_a_rpc_port,
        relay_a.as_mut().expect("restarted Rust relay A"),
        "rust-shared-peer-relay-a-after-in-flight-resource-restart",
    )?;
    require_attached_local_client(relay_a_rpc_port, "Rust relay A after in-flight restart")?;
    let interfaces = rpc_call(relay_a_rpc_port, "list_interfaces", None)?;
    let restored_paths = interfaces
        .get("interfaces")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("relay A returned no interfaces after restart: {interfaces}"))?
        .iter()
        .filter_map(|interface| interface.get("settings"))
        .filter_map(|settings| settings.get("_runtime"))
        .filter_map(|runtime| runtime.get("reticulum"))
        .filter_map(|reticulum| reticulum.get("path_table_restore"))
        .filter_map(|restore| restore.get("restored_active_paths"))
        .filter_map(Value::as_u64)
        .sum::<u64>();
    if restored_paths != 0 {
        return Err(format!("relay A restored {restored_paths} active routes from the cleared route store"));
    }
    wait_for_connected_tcp_client(relay_b_rpc_port, "relay-a-uplink")?;

    for port in [python_control_a_port, python_control_b_port] {
        python_control_call(port, "announce", None)?;
    }
    for (sender, destination) in [
        (python_control_a_port, hash_b),
        (python_control_b_port, hash_a),
    ] {
        python_control_call(sender, "request_path", Some(json!({ "destination": destination })))?;
        python_control_call(
            sender,
            "wait_path",
            Some(json!({ "destination": destination, "timeout": 30.0 })),
        )?;
    }
    wait_for_known_path_without_announce(relay_a_rpc_port, hash_b)?;
    wait_for_known_path_without_announce(relay_b_rpc_port, hash_a)?;

    verify_direct_resource_retry(
        python_control_a_port,
        python_control_b_port,
        &message_hash,
        &first_resource,
        150.0,
    )?;
    python_control_call(
        python_control_b_port,
        "set_router_resource_callback_chaining",
        Some(json!({ "enabled": false })),
    )?;
    Ok(())
}

#[test]
#[ignore = "requires local Python Reticulum/LXMF repos and daemon runtime"]
fn python_direct_resource_retry_after_upstream_restart_e2e() {
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

    let relay_a_dir = temp.path().join("inflight-resource-relay-a");
    let relay_b_dir = temp.path().join("inflight-resource-relay-b");
    let python_storage_a = temp.path().join("inflight-resource-python-a-storage");
    let python_storage_b = temp.path().join("inflight-resource-python-b-storage");
    let python_rns_a = temp.path().join("inflight-resource-python-a-rns");
    let python_rns_b = temp.path().join("inflight-resource-python-b-rns");
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
                "inflight-resource-relay-a",
                relay_a_rpc_port,
                Some(relay_a_transport_port),
                &[local_client_interface("python-a-shared", shared_a_port)],
            )
        ),
    );
    write_rust_config(
        &relay_b_dir,
        &format!(
            "{}\n[reticulum]\nenable_transport = true\n",
            rust_node_config(
                "inflight-resource-relay-b",
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
        "inflight-resource-python-a",
        "Python shared peer A",
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
            "inflight-resource-python-a",
        )?;
        let hash_a = python_control_call(python_control_a_port, "status", None)?
            .get("delivery_destination_hash")
            .and_then(Value::as_str)
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
            "inflight-resource-relay-a",
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
            "inflight-resource-relay-b",
        )?;
        wait_for_connected_tcp_client(relay_b_rpc_port, "relay-a-uplink")?;

        python_peer_b = Some(spawn_python_endpoint(
            &python_bin,
            &reticulum_repo,
            &lxmf_repo,
            &helper_script,
            "inflight-resource-python-b",
            "Python peer B",
            &python_rns_b,
            &python_storage_b,
            python_control_b_port,
            &mut [python_control_b],
        ));
        wait_for_python_endpoint_ready(
            python_control_b_port,
            python_peer_b.as_mut().expect("Python peer B"),
            "inflight-resource-python-b",
        )?;
        let hash_b = python_control_call(python_control_b_port, "status", None)?
            .get("delivery_destination_hash")
            .and_then(Value::as_str)
            .ok_or_else(|| "Python peer B has no delivery destination hash".to_string())?
            .to_string();

        for port in [python_control_a_port, python_control_b_port] {
            python_control_call(port, "announce", None)?;
        }
        require_known_path(relay_b_rpc_port, &hash_b, "Rust relay B local Python endpoint")?;
        for (sender, destination) in [
            (python_control_a_port, &hash_b),
            (python_control_b_port, &hash_a),
        ] {
            python_control_call(
                sender,
                "wait_path",
                Some(json!({ "destination": destination, "timeout": 30.0 })),
            )?;
        }
        wait_for_known_path_without_announce(relay_a_rpc_port, &hash_b)?;
        wait_for_known_path_without_announce(relay_b_rpc_port, &hash_a)?;

        exercise_inflight_resource_restart(
            &lxmd_bin,
            &reticulumd_bin,
            relay_a_rpc_port,
            &relay_a_dir,
            &mut relay_a,
            relay_b_rpc_port,
            python_control_a_port,
            python_control_b_port,
            &hash_a,
            &hash_b,
        )
    })();

    let failure = outcome.as_ref().err().cloned();
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
    if let Some(error) = failure {
        panic!("in-flight DIRECT Resource restart acceptance failed:\n{error}");
    }
}
