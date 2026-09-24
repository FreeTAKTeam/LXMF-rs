use std::thread;

fn require_known_path(rpc_port: u16, destination: &str, label: &str) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut last_status = "path status was not queried".to_string();
    while Instant::now() < deadline {
        match rpc_call(
            rpc_port,
            "path_status",
            Some(json!({ "destination": destination })),
        ) {
            Ok(status) => {
                if status["known"].as_bool() == Some(true)
                    || status["path_found"].as_bool() == Some(true)
                {
                    return Ok(());
                }
                last_status = status.to_string();
            }
            Err(error) => last_status = format!("rpc error: {error}"),
        }
        thread::sleep(Duration::from_millis(250));
    }
    Err(format!("{label} path to {destination} was not restored: {last_status}"))
}

fn require_attached_local_client(rpc_port: u16, label: &str) -> Result<(), String> {
    require_attached_local_clients(rpc_port, 1, label)
}

fn require_attached_local_clients(
    rpc_port: u16,
    expected_count: usize,
    label: &str,
) -> Result<(), String> {
    let status = rpc_call(rpc_port, "list_interfaces", None)?;
    let interfaces = status
        .get("interfaces")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{label} list_interfaces did not return an interfaces array: {status}"))?;
    let local_clients = interfaces
        .iter()
        .filter(|entry| entry.get("type").and_then(Value::as_str) == Some("local_client"))
        .collect::<Vec<_>>();
    if local_clients.len() != expected_count {
        return Err(format!(
            "{label} reported {} local_client interfaces, expected {expected_count}: {status}",
            local_clients.len()
        ));
    }
    for (index, local) in local_clients.iter().enumerate() {
        let startup_status = local
            .get("settings")
            .and_then(|settings| settings.get("_runtime"))
            .and_then(|runtime| runtime.get("startup_status"))
            .and_then(Value::as_str);
        if startup_status != Some("attached") {
            return Err(format!(
                "{label} local_client #{index} startup status was {startup_status:?}, expected attached: {status}"
            ));
        }
    }
    Ok(())
}

fn wait_for_connected_tcp_client(rpc_port: u16, name: &str) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut last_status = "interface status was not queried".to_string();
    while Instant::now() < deadline {
        match rpc_call(rpc_port, "list_interfaces", None) {
            Ok(status) => {
                let client = status
                    .get("interfaces")
                    .and_then(Value::as_array)
                    .and_then(|interfaces| {
                        interfaces.iter().find(|interface| {
                            interface.get("name").and_then(Value::as_str) == Some(name)
                                && interface.get("type").and_then(Value::as_str)
                                    == Some("tcp_client")
                        })
                    });
                let stream_state = client
                    .and_then(|interface| interface.get("settings"))
                    .and_then(|settings| settings.get("_runtime"))
                    .and_then(|runtime| runtime.get("tcp"))
                    .and_then(|tcp| tcp.get("stream_status"))
                    .and_then(|stream| stream.get("stream_state"))
                    .and_then(Value::as_str);
                if stream_state == Some("connected") {
                    return Ok(());
                }
                last_status = status.to_string();
            }
            Err(error) => last_status = format!("rpc error: {error}"),
        }
        thread::sleep(Duration::from_millis(250));
    }
    Err(format!("TCP client interface {name} did not reconnect: {last_status}"))
}

#[test]
#[ignore = "requires local Python Reticulum/LXMF repos and daemon runtime"]
fn python_shared_instance_rust_lxmd_application_and_restart_e2e() {
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
    let shared_instance = ReservedPort::reserve();
    let rust_rpc = ReservedPort::reserve();
    let python_control = ReservedPort::reserve();
    let shared_instance_port = shared_instance.port();
    let rust_rpc_port = rust_rpc.port();
    let python_control_port = python_control.port();

    let rust_dir = temp.path().join("rust-shared-client");
    let python_storage = temp.path().join("python-shared-storage");
    let python_rns = temp.path().join("python-shared-rns");
    write_rust_config(
        &rust_dir,
        &rust_node_config(
            "rust-shared-client",
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
        "python-shared-peer",
        "Python shared peer",
        &python_rns,
        &python_storage,
        python_control_port,
        &mut [python_control, shared_instance],
    ));
    let mut rust_node = None;

    let outcome: Result<(), String> = (|| {
        wait_for_python_endpoint_ready(
            python_control_port,
            python_node.as_mut().expect("Python shared peer"),
            "python-shared-peer",
        )?;

        let python_status = python_control_call(python_control_port, "status", None)?;
        if python_status
            .get("reticulum")
            .and_then(|reticulum| reticulum.get("is_shared_instance"))
            != Some(&Value::Bool(true))
        {
            return Err(format!("Python endpoint did not own the shared instance: {python_status}"));
        }

        rust_node = Some(spawn_lxmd(
            &lxmd_bin,
            &reticulumd_bin,
            rust_rpc_port,
            &rust_dir,
            &mut [rust_rpc],
        ));
        wait_for_ready(
            rust_rpc_port,
            rust_node.as_mut().expect("Rust shared client"),
            "rust-shared-client-before-restart",
        )?;
        require_attached_local_client(rust_rpc_port, "Rust shared client before restart")?;

        let rust_status = daemon_status(rust_rpc_port)?;
        let rust_hash = status_hash(&rust_status)
            .ok_or_else(|| format!("missing Rust delivery destination hash: {rust_status}"))?;
        let python_hash = python_status
            .get("delivery_destination_hash")
            .and_then(Value::as_str)
            .filter(|hash| !hash.is_empty())
            .ok_or_else(|| format!("missing Python delivery destination hash: {python_status}"))?
            .to_string();

        python_control_call(python_control_port, "announce", None)?;
        rpc_call(rust_rpc_port, "announce_now", None)?;
        wait_for_known_path_without_announce(rust_rpc_port, &python_hash)?;
        rpc_call(
            rust_rpc_port,
            "send_message_v2",
            Some(json!({
                "id": "rust-to-python-shared-before-restart",
                "source": rust_hash,
                "destination": python_hash,
                "title": "",
                "content": "rust-to-python-shared-before-restart",
                "method": "direct"
            })),
        )?;
        python_control_call(
            python_control_port,
            "wait_message",
            Some(json!({
                "content": "rust-to-python-shared-before-restart",
                "timeout": 60.0
            })),
        )?;

        python_control_call(
            python_control_port,
            "send_message",
            Some(json!({
                "destination": rust_hash,
                "title": "",
                "content": "python-to-rust-shared-before-restart"
            })),
        )?;
        wait_for_inbound_message(rust_rpc_port, "python-to-rust-shared-before-restart")?;

        if let Some(node) = rust_node.as_mut() {
            terminate_child(&mut node.child);
        }

        thread::sleep(Duration::from_secs(1));

        rust_node = Some(spawn_lxmd(
            &lxmd_bin,
            &reticulumd_bin,
            rust_rpc_port,
            &rust_dir,
            &mut [],
        ));
        wait_for_ready(
            rust_rpc_port,
            rust_node.as_mut().expect("Rust shared client after restart"),
            "rust-shared-client-after-restart",
        )?;
        require_attached_local_client(rust_rpc_port, "Rust shared client after restart")?;

        let restarted_status = daemon_status(rust_rpc_port)?;
        let restarted_hash = status_hash(&restarted_status).ok_or_else(|| {
            format!("missing Rust delivery destination hash after restart: {restarted_status}")
        })?;
        if restarted_hash != rust_hash {
            return Err(format!(
                "Rust delivery destination identity changed across restart: before={rust_hash} after={restarted_hash}"
            ));
        }

        python_control_call(python_control_port, "announce", None)?;
        rpc_call(rust_rpc_port, "announce_now", None)?;
        wait_for_known_path_without_announce(rust_rpc_port, &python_hash)?;
        rpc_call(
            rust_rpc_port,
            "send_message_v2",
            Some(json!({
                "id": "rust-to-python-shared-after-restart",
                "source": restarted_hash,
                "destination": python_hash,
                "title": "",
                "content": "rust-to-python-shared-after-restart",
                "method": "direct"
            })),
        )?;
        python_control_call(
            python_control_port,
            "wait_message",
            Some(json!({
                "content": "rust-to-python-shared-after-restart",
                "timeout": 60.0
            })),
        )?;

        python_control_call(
            python_control_port,
            "send_message",
            Some(json!({
                "destination": restarted_hash,
                "title": "",
                "content": "python-to-rust-shared-after-restart"
            })),
        )?;
        wait_for_inbound_message(rust_rpc_port, "python-to-rust-shared-after-restart")?;

        let python_link = python_control_call(python_control_port, "link_status", None)?;
        if python_link.get("status_name").and_then(Value::as_str) != Some("active") {
            return Err(format!("Python shared-instance delivery link was not active: {python_link}"));
        }

        Ok(())
    })();

    let failure_details = if let Err(err) = &outcome {
        Some(format!(
            "{err}\n\n{}\n\n{}",
            collect_node_diagnostics("rust-shared-client", rust_rpc_port, rust_node.as_mut()),
            collect_python_endpoint_diagnostics(
                "python-shared-peer",
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
        panic!("Python/Rust shared-instance daemon flow failed:\n{details}");
    }
}

#[test]
#[ignore = "requires local Python Reticulum/LXMF repos and daemon runtime"]
fn python_shared_instance_two_peer_relay_recovers_after_daemon_restart_e2e() {
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
    let shared_b = ReservedPort::reserve();
    let rust_rpc = ReservedPort::reserve();
    let rust_transport = ReservedPort::reserve();
    let python_control_a = ReservedPort::reserve();
    let python_control_b = ReservedPort::reserve();
    let rns_rpc_a = ReservedPort::reserve();
    let rns_rpc_b = ReservedPort::reserve();
    let shared_a_port = shared_a.port();
    let shared_b_port = shared_b.port();
    let rust_rpc_port = rust_rpc.port();
    let rust_transport_port = rust_transport.port();
    let python_control_a_port = python_control_a.port();
    let python_control_b_port = python_control_b.port();
    let rns_rpc_a_port = rns_rpc_a.port();
    let rns_rpc_b_port = rns_rpc_b.port();

    let rust_dir = temp.path().join("rust-two-shared-peers");
    let python_storage_a = temp.path().join("python-shared-peer-a-storage");
    let python_storage_b = temp.path().join("python-shared-peer-b-storage");
    let python_rns_a = temp.path().join("python-shared-peer-a-rns");
    let python_rns_b = temp.path().join("python-shared-peer-b-rns");
    write_python_shared_instance_rns_config_with_control_port(
        &python_rns_a,
        shared_a_port,
        rns_rpc_a_port,
    );
    write_python_shared_instance_rns_config_with_control_port(
        &python_rns_b,
        shared_b_port,
        rns_rpc_b_port,
    );

    let interfaces = [
        local_client_interface("python-shared-a", shared_a_port),
        local_client_interface("python-shared-b", shared_b_port),
    ];
    let config =
        rust_node_config("rust-two-shared-peers", rust_rpc_port, Some(rust_transport_port), &interfaces);
    write_rust_config(&rust_dir, &config);

    let mut python_peer_a = Some(spawn_python_endpoint(
        &python_bin,
        &reticulum_repo,
        &lxmf_repo,
        &helper_script,
        "python-shared-peer-a",
        "Python shared peer A",
        &python_rns_a,
        &python_storage_a,
        python_control_a_port,
        &mut [shared_a, python_control_a, rns_rpc_a],
    ));
    let mut python_peer_b = Some(spawn_python_endpoint(
        &python_bin,
        &reticulum_repo,
        &lxmf_repo,
        &helper_script,
        "python-shared-peer-b",
        "Python shared peer B",
        &python_rns_b,
        &python_storage_b,
        python_control_b_port,
        &mut [shared_b, python_control_b, rns_rpc_b],
    ));
    let mut rust_node = None;
    let mut destination_hashes = None;

    let outcome: Result<(), String> = (|| {
        wait_for_python_endpoint_ready(
            python_control_a_port,
            python_peer_a.as_mut().expect("Python shared peer A"),
            "python-shared-peer-a",
        )?;
        wait_for_python_endpoint_ready(
            python_control_b_port,
            python_peer_b.as_mut().expect("Python shared peer B"),
            "python-shared-peer-b",
        )?;

        let python_status_a = python_control_call(python_control_a_port, "status", None)?;
        let python_status_b = python_control_call(python_control_b_port, "status", None)?;
        for (label, status) in [
            ("Python shared peer A", &python_status_a),
            ("Python shared peer B", &python_status_b),
        ] {
            if status
                .get("reticulum")
                .and_then(|reticulum| reticulum.get("is_shared_instance"))
                != Some(&Value::Bool(true))
            {
                return Err(format!("{label} did not own its shared instance: {status}"));
            }
        }
        let hash_a = python_status_a
            .get("delivery_destination_hash")
            .and_then(Value::as_str)
            .filter(|hash| !hash.is_empty())
            .ok_or_else(|| format!("Python peer A has no delivery hash: {python_status_a}"))?
            .to_string();
        let hash_b = python_status_b
            .get("delivery_destination_hash")
            .and_then(Value::as_str)
            .filter(|hash| !hash.is_empty())
            .ok_or_else(|| format!("Python peer B has no delivery hash: {python_status_b}"))?
            .to_string();
        destination_hashes = Some((hash_a.clone(), hash_b.clone()));

        rust_node = Some(spawn_lxmd(
            &lxmd_bin,
            &reticulumd_bin,
            rust_rpc_port,
            &rust_dir,
            &mut [rust_rpc, rust_transport],
        ));
        wait_for_ready(
            rust_rpc_port,
            rust_node.as_mut().expect("Rust shared-peer relay"),
            "rust-two-shared-peers-before-restart",
        )?;
        require_attached_local_clients(rust_rpc_port, 2, "Rust shared-peer relay before restart")?;

        python_control_call(python_control_a_port, "announce", None)?;
        python_control_call(python_control_b_port, "announce", None)?;
        rpc_call(rust_rpc_port, "announce_now", None)?;
        wait_for_known_path_without_announce(rust_rpc_port, &hash_a)?;
        wait_for_known_path_without_announce(rust_rpc_port, &hash_b)?;

        for (sender, receiver, destination, before, after) in [
            (
                python_control_a_port,
                python_control_b_port,
                &hash_b,
                "shared-a-to-b-before-restart",
                "shared-a-to-b-before-restart",
            ),
            (
                python_control_b_port,
                python_control_a_port,
                &hash_a,
                "shared-b-to-a-before-restart",
                "shared-b-to-a-before-restart",
            ),
        ] {
            python_control_call(
                sender,
                "wait_path",
                Some(json!({ "destination": destination, "timeout": 5.0 })),
            )?;
            python_control_call(
                sender,
                "send_message",
                Some(json!({ "destination": destination, "title": "", "content": before })),
            )?;
            python_control_call(
                receiver,
                "wait_message",
                Some(json!({ "content": after, "timeout": 30.0 })),
            )?;
        }

        if let Some(node) = rust_node.as_mut() {
            terminate_child(&mut node.child);
        }

        let queued_message = python_control_call(
            python_control_a_port,
            "send_message",
            Some(json!({
                "destination": &hash_b,
                "title": "",
                "content": "queued-a-to-b-across-restart",
                "wait_for_path": false,
                "method": "opportunistic"
            })),
        )?;
        let queued_message_hash = queued_message
            .get("message_hash")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("Python peer A returned no queued message hash: {queued_message}"))?;
        let queued_status = python_control_call(
            python_control_a_port,
            "outbound_status",
            Some(json!({ "message_hash": queued_message_hash })),
        )?;
        if matches!(
            queued_status.get("state_name").and_then(Value::as_str),
            Some("delivered" | "rejected" | "cancelled" | "failed")
        ) {
            return Err(format!(
                "LXMF message did not remain queued while the Rust relay was stopped: {queued_status}"
            ));
        }

        rust_node = Some(spawn_lxmd(
            &lxmd_bin,
            &reticulumd_bin,
            rust_rpc_port,
            &rust_dir,
            &mut [],
        ));
        wait_for_ready(
            rust_rpc_port,
            rust_node.as_mut().expect("Rust shared-peer relay after restart"),
            "rust-two-shared-peers-after-restart",
        )?;
        require_attached_local_clients(rust_rpc_port, 2, "Rust shared-peer relay after restart")?;

        python_control_call(python_control_a_port, "announce", None)?;
        python_control_call(python_control_b_port, "announce", None)?;
        for destination in [&hash_a, &hash_b] {
            require_known_path(rust_rpc_port, destination, "Rust relay relearned")?;
        }

        python_control_call(
            python_control_b_port,
            "wait_message",
            Some(json!({
                "content": "queued-a-to-b-across-restart",
                "timeout": 45.0
            })),
        )?;
        python_control_call(
            python_control_a_port,
            "wait_outbound_state",
            Some(json!({
                "message_hash": queued_message_hash,
                "state": "delivered",
                "timeout": 45.0
            })),
        )?;

        for (sender, receiver, destination, content) in [
            (
                python_control_a_port,
                python_control_b_port,
                &hash_b,
                "raw-a-to-b-after-restart",
            ),
            (
                python_control_b_port,
                python_control_a_port,
                &hash_a,
                "raw-b-to-a-after-restart",
            ),
        ] {
            python_control_call(
                sender,
                "wait_path",
                Some(json!({ "destination": destination, "timeout": 10.0 })),
            )?;
            python_control_call(
                sender,
                "open_raw_link",
                Some(json!({ "destination": destination, "timeout": 20.0 })),
            )?;
            python_control_call(sender, "send_raw", Some(json!({ "content": content })))?;
            python_control_call(
                receiver,
                "wait_raw_message",
                Some(json!({ "content": content, "timeout": 10.0 })),
            )?;
        }

        let python_link_a = python_control_call(python_control_a_port, "raw_link_status", None)?;
        let python_link_b = python_control_call(python_control_b_port, "raw_link_status", None)?;
        if python_link_a.get("status_name").and_then(Value::as_str) != Some("active") {
            return Err(format!("Python peer A raw link was not active: {python_link_a}"));
        }
        if python_link_b.get("status_name").and_then(Value::as_str) != Some("active") {
            return Err(format!("Python peer B raw link was not active: {python_link_b}"));
        }
        Ok(())
    })();

    let failure_details = if let Err(err) = &outcome {
        let route_diagnostics = if let Some((hash_a, hash_b)) = destination_hashes.as_ref() {
            let route_a = rpc_call(
                rust_rpc_port,
                "path_status",
                Some(json!({ "destination": hash_a })),
            )
            .map(|status| status.to_string())
            .unwrap_or_else(|error| format!("rpc error: {error}"));
            let route_b = rpc_call(
                rust_rpc_port,
                "path_status",
                Some(json!({ "destination": hash_b })),
            )
            .map(|status| status.to_string())
            .unwrap_or_else(|error| format!("rpc error: {error}"));
            let interface_traffic = rpc_call(rust_rpc_port, "daemon_status_ex", None)
                .map(|status| {
                    status
                        .get("reticulum")
                        .and_then(|reticulum| reticulum.get("transport"))
                        .and_then(|transport| transport.get("interfaces"))
                        .cloned()
                        .unwrap_or(Value::Null)
                        .to_string()
                })
                .unwrap_or_else(|error| format!("rpc error: {error}"));
            format!(
                "Rust path A: {route_a}\nRust path B: {route_b}\nRust interface traffic: {interface_traffic}"
            )
        } else {
            "Python destination hashes were unavailable".to_string()
        };
        Some(format!(
            "{err}\n\n{route_diagnostics}\n\n{}\n\n{}",
            collect_python_endpoint_diagnostics(
                "python-shared-peer-a",
                python_control_a_port,
                python_peer_a.as_mut(),
            ),
            collect_python_endpoint_diagnostics(
                "python-shared-peer-b",
                python_control_b_port,
                python_peer_b.as_mut(),
            ),
        ))
    } else {
        None
    };

    if let Some(node) = rust_node.as_mut() {
        terminate_child(&mut node.child);
    }
    if let Some(node) = python_peer_a.as_mut() {
        terminate_child(&mut node.child);
    }
    if let Some(node) = python_peer_b.as_mut() {
        terminate_child(&mut node.child);
    }

    if let Some(details) = failure_details {
        panic!("Python/Rust multi-peer daemon-replacement flow failed:\n{details}");
    }
}

#[test]
#[ignore = "requires local Python Reticulum/LXMF repos and daemon runtime"]
fn python_shared_instance_two_rust_relays_recover_after_upstream_restart_e2e() {
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

    let relay_a_dir = temp.path().join("rust-shared-peer-relay-a");
    let relay_b_dir = temp.path().join("rust-shared-peer-relay-b");
    let python_storage_a = temp.path().join("python-shared-peer-a-storage");
    let python_storage_b = temp.path().join("python-shared-peer-b-storage");
    let python_rns_a = temp.path().join("python-shared-peer-a-rns");
    let python_rns_b = temp.path().join("python-shared-peer-b-rns");
    write_python_shared_instance_rns_config_with_control_port(
        &python_rns_a,
        shared_a_port,
        rns_rpc_a_port,
    );
    write_python_client_rns_config(&python_rns_b, relay_b_transport_port);

    let relay_a_config = format!(
        "{}\n[reticulum]\nenable_transport = true\n",
        rust_node_config(
            "rust-shared-peer-relay-a",
            relay_a_rpc_port,
            Some(relay_a_transport_port),
            &[local_client_interface("python-shared-a", shared_a_port)],
        )
    );
    let relay_b_config = format!(
        "{}\n[reticulum]\nenable_transport = true\n",
        rust_node_config(
            "rust-shared-peer-relay-b",
            relay_b_rpc_port,
            Some(relay_b_transport_port),
            &[tcp_client_interface("relay-a-uplink", relay_a_transport_port)],
        )
    );
    write_rust_config(&relay_a_dir, &relay_a_config);
    write_rust_config(&relay_b_dir, &relay_b_config);

    let mut python_peer_a = Some(spawn_python_endpoint(
        &python_bin,
        &reticulum_repo,
        &lxmf_repo,
        &helper_script,
        "python-shared-peer-a",
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
            python_peer_a.as_mut().expect("Python shared peer A"),
            "python-shared-peer-a",
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
            "rust-shared-peer-relay-a",
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
            "rust-shared-peer-relay-b",
        )?;

        python_peer_b = Some(spawn_python_endpoint(
            &python_bin,
            &reticulum_repo,
            &lxmf_repo,
            &helper_script,
            "python-shared-peer-b",
            "Python shared peer B",
            &python_rns_b,
            &python_storage_b,
            python_control_b_port,
            &mut [python_control_b],
        ));
        wait_for_python_endpoint_ready(
            python_control_b_port,
            python_peer_b.as_mut().expect("Python shared peer B"),
            "python-shared-peer-b",
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

        terminate_child(&mut relay_a.as_mut().expect("Rust relay A").child);
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
            "rust-shared-peer-relay-a-after-restart",
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
            return Err(format!(
                "relay A restored {restored_paths} active routes after its route store was cleared"
            ));
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
        wait_for_known_path_without_announce(relay_b_rpc_port, &hash_a)?;

        let mut outbound_message_hashes = Vec::new();
        for (sender, receiver, destination, content) in [
            (python_control_a_port, python_control_b_port, &hash_b, "multi-hop-after-restart-a-to-b"),
            (python_control_b_port, python_control_a_port, &hash_a, "multi-hop-after-restart-b-to-a"),
        ] {
            python_control_call(
                sender,
                "wait_path",
                Some(json!({ "destination": destination, "timeout": 10.0 })),
            )?;
            let outbound = python_control_call(
                sender,
                "send_message",
                Some(json!({ "destination": destination, "title": "", "content": content })),
            )?;
            let message_hash = outbound
                .get("message_hash")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("Python sender returned no LXMF message hash: {outbound}"))?;
            python_control_call(
                receiver,
                "wait_message",
                Some(json!({ "content": content, "timeout": 30.0 })),
            )?;
            outbound_message_hashes.push((sender, message_hash.to_string()));
        }
        for (sender, message_hash) in outbound_message_hashes {
            python_control_call(
                sender,
                "wait_outbound_state",
                Some(json!({ "message_hash": message_hash, "state": "delivered", "timeout": 30.0 })),
            )?;
        }

        for (sender, receiver, destination, content) in [
            (python_control_a_port, python_control_b_port, &hash_b, "raw-multi-hop-after-restart-a-to-b"),
            (python_control_b_port, python_control_a_port, &hash_a, "raw-multi-hop-after-restart-b-to-a"),
        ] {
            python_control_call(
                sender,
                "wait_path",
                Some(json!({ "destination": destination, "timeout": 10.0 })),
            )?;
            python_control_call(
                sender,
                "open_raw_link",
                Some(json!({ "destination": destination, "timeout": 20.0 })),
            )?;
            python_control_call(sender, "send_raw", Some(json!({ "content": content })))?;
            python_control_call(
                receiver,
                "wait_raw_message",
                Some(json!({ "content": content, "timeout": 10.0 })),
            )?;
        }
        for (label, port) in [
            ("Python peer A", python_control_a_port),
            ("Python peer B", python_control_b_port),
        ] {
            let link_status = python_control_call(port, "raw_link_status", None)?;
            if link_status.get("status_name").and_then(Value::as_str) != Some("active") {
                return Err(format!("{label} multi-hop raw Link was not active: {link_status}"));
            }
        }
        Ok(())
    })();

    let failure_details = outcome.as_ref().err().map(|err| err.to_string());

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
        panic!("mixed Python/Rust multi-hop recovery failed:\n{details}");
    }
}
