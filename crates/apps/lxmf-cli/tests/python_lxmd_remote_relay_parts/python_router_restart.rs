fn wait_for_rust_outbound_delivery(rpc_port: u16, message_id: &str) -> Result<Value, String> {
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut last_status = Value::Null;
    while Instant::now() < deadline {
        let response = rpc_call(rpc_port, "list_messages", None)?;
        let record = response
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|messages| {
                messages.iter().find(|message| message["id"].as_str() == Some(message_id))
            });
        if let Some(record) = record {
            last_status = record.clone();
            match record["receipt_status"].as_str() {
                Some("delivered") => return Ok(record.clone()),
                Some("rejected" | "cancelled" | "failed" | "expired") => {
                    return Err(format!("Rust outbound reached terminal failure: {record}"));
                }
                _ => {}
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(format!("Rust outbound {message_id} did not reach delivered: {last_status}"))
}

#[test]
#[ignore = "requires local Python Reticulum/LXMF repos and daemon runtime"]
fn python_lxmf_router_delivery_cache_survives_process_restart_e2e() {
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
    assert!(helper_script.exists(), "Python helper script missing: {}", helper_script.display());

    let temp = tempfile::tempdir().expect("tempdir");
    let shared_instance = ReservedPort::reserve();
    let rust_rpc = ReservedPort::reserve();
    let python_control = ReservedPort::reserve();
    let shared_instance_port = shared_instance.port();
    let rust_rpc_port = rust_rpc.port();
    let python_control_port = python_control.port();
    let rust_dir = temp.path().join("rust-client");
    let python_storage = temp.path().join("python-router-storage");
    let python_rns = temp.path().join("python-rns");
    write_rust_config(
        &rust_dir,
        &rust_node_config(
            "rust-client",
            rust_rpc_port,
            None,
            &[local_client_interface("python-shared", shared_instance_port)],
        ),
    );
    write_python_shared_instance_rns_config(&python_rns, shared_instance_port);

    let mut python_node = Some(spawn_python_endpoint(
        &python_bin,
        &reticulum_repo,
        &lxmf_repo,
        &helper_script,
        "python-router-restart-peer",
        "Python router restart peer",
        &python_rns,
        &python_storage,
        python_control_port,
        &mut [python_control, shared_instance],
    ));
    let mut rust_node = None;
    let outcome: Result<(), String> = (|| {
        wait_for_python_endpoint_ready(
            python_control_port,
            python_node.as_mut().expect("Python peer"),
            "Python LXMRouter before restart",
        )?;
        rust_node = Some(spawn_lxmd(
            &lxmd_bin,
            &reticulumd_bin,
            rust_rpc_port,
            &rust_dir,
            &mut [rust_rpc],
        ));
        wait_for_ready(
            rust_rpc_port,
            rust_node.as_mut().expect("Rust client"),
            "Rust client before Python restart",
        )?;
        require_attached_local_client(rust_rpc_port, "Rust client before Python restart")?;

        let initial_python_status = python_control_call(python_control_port, "status", None)?;
        let initial_destination = initial_python_status["delivery_destination_hash"]
            .as_str()
            .ok_or_else(|| format!("Python status omitted delivery destination hash: {initial_python_status}"))?
            .to_string();
        let initial_identity = initial_python_status["identity_hash"]
            .as_str()
            .ok_or_else(|| format!("Python status omitted identity hash: {initial_python_status}"))?
            .to_string();
        let rust_status = daemon_status(rust_rpc_port)?;
        let rust_hash = status_hash(&rust_status)
            .ok_or_else(|| format!("Rust status omitted delivery destination hash: {rust_status}"))?;

        python_control_call(python_control_port, "announce", None)?;
        rpc_call(rust_rpc_port, "announce_now", None)?;
        wait_for_known_path_without_announce(rust_rpc_port, &initial_destination)?;
        let first_payload = "python-router-cache-before-process-restart";
        let first_send = rpc_call(
            rust_rpc_port,
            "send_message_v2",
            Some(json!({
                "id": "rust-to-python-router-cache-before-restart",
                "source": rust_hash,
                "destination": initial_destination,
                "title": "",
                "content": first_payload,
                "method": "direct"
            })),
        )?;
        let first_id = first_send["message_id"]
            .as_str()
            .ok_or_else(|| format!("Rust send omitted message ID: {first_send}"))?
            .to_string();
        let first_received = python_control_call(
            python_control_port,
            "wait_message",
            Some(json!({ "content": first_payload, "timeout": 60.0 })),
        )?;
        let first_transient_id = first_received["message"]["message_hash"]
            .as_str()
            .ok_or_else(|| format!("Python delivery callback omitted transient ID: {first_received}"))?
            .to_string();
        let before_exit_cache = python_control_call(
            python_control_port,
            "delivery_cache_status",
            Some(json!({ "message_hash": first_transient_id })),
        )?;
        if before_exit_cache["cache_contains"] != Value::Bool(true) {
            return Err(format!("Python did not cache the delivered transient ID: {before_exit_cache}"));
        }
        wait_for_rust_outbound_delivery(rust_rpc_port, &first_id)?;

        python_control_call(python_control_port, "shutdown", None)?;
        let shutdown_deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let exited = python_node
                .as_mut()
                .expect("Python peer during shutdown")
                .child
                .try_wait()
                .map_err(|error| format!("checking Python shutdown: {error}"))?
                .is_some();
            if exited {
                break;
            }
            if Instant::now() >= shutdown_deadline {
                return Err("Python endpoint did not exit after graceful LXMRouter shutdown".into());
            }
            thread::sleep(Duration::from_millis(100));
        }
        python_node = Some(spawn_python_endpoint(
            &python_bin,
            &reticulum_repo,
            &lxmf_repo,
            &helper_script,
            "python-router-restart-peer",
            "Python router restart peer",
            &python_rns,
            &python_storage,
            python_control_port,
            &mut [],
        ));
        wait_for_python_endpoint_ready(
            python_control_port,
            python_node.as_mut().expect("restarted Python peer"),
            "Python LXMRouter after restart",
        )?;
        let restarted_python_status = python_control_call(python_control_port, "status", None)?;
        if restarted_python_status["delivery_destination_hash"].as_str() != Some(&initial_destination)
            || restarted_python_status["identity_hash"].as_str() != Some(&initial_identity)
        {
            return Err(format!(
                "Python delivery identity changed across process restart: before={initial_python_status} after={restarted_python_status}"
            ));
        }
        let restored_cache = python_control_call(
            python_control_port,
            "delivery_cache_status",
            Some(json!({ "message_hash": first_transient_id })),
        )?;
        if restored_cache["cache_contains"] != Value::Bool(true) {
            return Err(format!("Python LXMRouter did not restore its delivered-ID cache: {restored_cache}"));
        }

        require_attached_local_client(rust_rpc_port, "Rust client after Python restart")?;
        python_control_call(python_control_port, "announce", None)?;
        rpc_call(rust_rpc_port, "announce_now", None)?;
        wait_for_known_path_without_announce(rust_rpc_port, &initial_destination)?;
        let after_payload = "rust-to-python-message-after-router-process-restart";
        let after_send = rpc_call(
            rust_rpc_port,
            "send_message_v2",
            Some(json!({
                "id": "rust-to-python-router-cache-after-restart",
                "source": rust_hash,
                "destination": initial_destination,
                "title": "",
                "content": after_payload,
                "method": "direct"
            })),
        )?;
        let after_id = after_send["message_id"]
            .as_str()
            .ok_or_else(|| format!("post-restart Rust send omitted message ID: {after_send}"))?
            .to_string();
        python_control_call(
            python_control_port,
            "wait_message",
            Some(json!({ "content": after_payload, "timeout": 60.0 })),
        )?;
        wait_for_rust_outbound_delivery(rust_rpc_port, &after_id)?;
        let inbox = python_control_call(python_control_port, "list_messages", None)?;
        let after_count = inbox["messages"].as_array().map_or(0, |messages| {
            messages
                .iter()
                .filter(|message| message["content"].as_str() == Some(after_payload))
                .count()
        });
        if after_count != 1 {
            return Err(format!("post-restart message callback count was {after_count}, expected exactly one: {inbox}"));
        }

        let reply = "python-to-rust-message-after-router-process-restart";
        python_control_call(
            python_control_port,
            "send_message",
            Some(json!({ "destination": rust_hash, "title": "", "content": reply })),
        )?;
        wait_for_inbound_message(rust_rpc_port, reply)?;
        Ok(())
    })();

    let failure_details = if let Err(error) = &outcome {
        Some(format!(
            "{error}\n\n{}\n\n{}",
            collect_node_diagnostics("Rust client", rust_rpc_port, rust_node.as_mut()),
            collect_python_endpoint_diagnostics(
                "Python LXMRouter",
                python_control_port,
                python_node.as_mut(),
            ),
        ))
    } else {
        None
    };
    if let Some(node) = rust_node.as_mut() {
        terminate_child(&mut node.child);
    }
    if let Some(node) = python_node.as_mut() {
        terminate_child(&mut node.child);
    }
    if let Some(details) = failure_details {
        panic!("Python LXMRouter process-restart persistence failed:\n{details}");
    }
}
