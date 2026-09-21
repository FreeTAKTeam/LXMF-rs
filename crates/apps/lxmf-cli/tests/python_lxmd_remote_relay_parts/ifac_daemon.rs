#[test]
#[ignore = "requires local Python Reticulum/LXMF repos and daemon runtime"]
fn python_rust_lxmd_ifac_bidirectional_daemon_e2e() {
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
    let rust_rpc = ReservedPort::reserve();
    let rust_transport = ReservedPort::reserve();
    let python_control = ReservedPort::reserve();
    let rust_rpc_port = rust_rpc.port();
    let rust_transport_port = rust_transport.port();
    let python_control_port = python_control.port();

    let rust_dir = temp.path().join("rust-ifac-daemon");
    let python_storage = temp.path().join("python-ifac-storage");
    let python_rns = temp.path().join("python-ifac-rns");
    write_rust_config(
        &rust_dir,
        &rust_node_config(
            "rust-ifac-daemon",
            rust_rpc_port,
            None,
            &[tcp_server_ifac_interface("ifac-server", rust_transport_port)],
        ),
    );
    write_python_client_rns_config_with_ifac(&python_rns, rust_transport_port);

    let mut rust_node = None;
    let mut python_node = None;
    let outcome: Result<(), String> = (|| {
        rust_node = Some(spawn_lxmd(
            &lxmd_bin,
            &reticulumd_bin,
            rust_rpc_port,
            &rust_dir,
            &mut [rust_rpc, rust_transport],
        ));
        wait_for_ready(
            rust_rpc_port,
            rust_node.as_mut().expect("Rust IFAC daemon"),
            "rust-ifac-daemon",
        )?;

        python_node = Some(spawn_python_endpoint(
            &python_bin,
            &reticulum_repo,
            &lxmf_repo,
            &helper_script,
            "python-ifac-peer",
            "Python IFAC peer",
            &python_rns,
            &python_storage,
            python_control_port,
            &mut [python_control],
        ));
        wait_for_python_endpoint_ready(
            python_control_port,
            python_node.as_mut().expect("Python IFAC peer"),
            "python-ifac-peer",
        )?;

        let rust_status = daemon_status(rust_rpc_port)?;
        let rust_hash = status_hash(&rust_status)
            .ok_or_else(|| format!("missing Rust delivery destination hash: {rust_status}"))?;
        let python_status = python_control_call(python_control_port, "status", None)?;
        let python_hash = python_status
            .get("delivery_destination_hash")
            .and_then(Value::as_str)
            .filter(|hash| !hash.is_empty())
            .ok_or_else(|| format!("missing Python delivery destination hash: {python_status}"))?
            .to_string();

        python_control_call(python_control_port, "announce", None)?;
        rpc_call(rust_rpc_port, "announce_now", None)?;

        rpc_call(
            rust_rpc_port,
            "send_message_v2",
            Some(json!({
                "id": "rust-to-python-ifac",
                "source": rust_hash,
                "destination": python_hash,
                "title": "",
                "content": "rust-to-python-ifac",
                "method": "direct"
            })),
        )?;
        python_control_call(
            python_control_port,
            "wait_message",
            Some(json!({ "content": "rust-to-python-ifac", "timeout": 60.0 })),
        )?;

        python_control_call(
            python_control_port,
            "send_message",
            Some(json!({
                "destination": rust_hash,
                "title": "",
                "content": "python-to-rust-ifac"
            })),
        )?;
        wait_for_inbound_message(rust_rpc_port, "python-to-rust-ifac")?;

        let python_link = python_control_call(python_control_port, "link_status", None)?;
        if python_link.get("status_name").and_then(Value::as_str) != Some("active") {
            return Err(format!("Python IFAC delivery link was not active: {python_link}"));
        }

        Ok(())
    })();

    let failure_details = if let Err(err) = &outcome {
        Some(format!(
            "{err}\n\n{}\n\n{}",
            collect_node_diagnostics(
                "rust-ifac-daemon",
                rust_rpc_port,
                rust_node.as_mut(),
            ),
            collect_python_endpoint_diagnostics(
                "python-ifac-peer",
                python_control_port,
                python_node.as_mut(),
            ),
        ))
    } else {
        None
    };

    if let Some(node) = python_node.as_mut() {
        terminate_child(&mut node.child);
    }
    if let Some(node) = rust_node.as_mut() {
        terminate_child(&mut node.child);
    }

    if let Some(details) = failure_details {
        panic!("Python/Rust IFAC daemon flow failed:\n{details}");
    }
}
