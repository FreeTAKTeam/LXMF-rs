use std::thread;

fn require_attached_local_client(rpc_port: u16, label: &str) -> Result<(), String> {
    let status = rpc_call(rpc_port, "list_interfaces", None)?;
    let interfaces = status
        .get("interfaces")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{label} list_interfaces did not return an interfaces array: {status}"))?;
    let local = interfaces
        .iter()
        .find(|entry| entry.get("type").and_then(Value::as_str) == Some("local_client"))
        .ok_or_else(|| format!("{label} did not report a local_client interface: {status}"))?;
    let startup_status = local
        .get("settings")
        .and_then(|settings| settings.get("_runtime"))
        .and_then(|runtime| runtime.get("startup_status"))
        .and_then(Value::as_str);
    if startup_status != Some("attached") {
        return Err(format!(
            "{label} local_client startup status was {startup_status:?}, expected attached: {status}"
        ));
    }
    Ok(())
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
