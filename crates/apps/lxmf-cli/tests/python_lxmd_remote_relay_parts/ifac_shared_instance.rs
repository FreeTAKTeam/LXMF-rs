fn write_python_shared_udp_ifac_rns_config(
    dir: &Path,
    shared_port: u16,
    listen_port: u16,
    forward_port: u16,
) {
    fs::create_dir_all(dir).expect("create Python shared IFAC RNS dir");
    fs::write(
        dir.join("config"),
        format!(
            "[reticulum]\nenable_transport = yes\nshare_instance = yes\nshared_instance_type = tcp\nshared_instance_port = {shared_port}\ndiscover_interfaces = no\n\n[logging]\nloglevel = 7\n\n[interfaces]\n  [[UDP IFAC Shared Instance]]\n    type = UDPInterface\n    enabled = yes\n    listen_ip = 127.0.0.1\n    listen_port = {listen_port}\n    forward_ip = 127.0.0.1\n    forward_port = {forward_port}\n    networkname = {IFAC_NETWORK_NAME}\n    passphrase = {IFAC_PASSPHRASE}\n    ifac_size = {IFAC_SIZE_BITS}\n"
        ),
    )
    .expect("write Python shared IFAC RNS config");
}

#[test]
#[ignore = "requires local Python Reticulum/LXMF repos and daemon runtime"]
fn python_rust_ifac_udp_shared_instance_preserves_local_client_traffic() {
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
    let owner_udp = ReservedPort::reserve();
    let remote_udp = ReservedPort::reserve();
    let owner_control = ReservedPort::reserve();
    let remote_control = ReservedPort::reserve();
    let shared_port = shared_instance.port();
    let rust_rpc_port = rust_rpc.port();
    let owner_udp_port = owner_udp.port();
    let remote_udp_port = remote_udp.port();
    let owner_control_port = owner_control.port();
    let remote_control_port = remote_control.port();

    let rust_dir = temp.path().join("rust-ifac-shared-client");
    let owner_storage = temp.path().join("python-ifac-shared-owner-storage");
    let owner_rns = temp.path().join("python-ifac-shared-owner-rns");
    let remote_storage = temp.path().join("python-ifac-shared-remote-storage");
    let remote_rns = temp.path().join("python-ifac-shared-remote-rns");
    write_rust_config(
        &rust_dir,
        &rust_node_config(
            "rust-ifac-shared-client",
            rust_rpc_port,
            None,
            &[local_client_interface("python-ifac-shared", shared_port)],
        ),
    );
    write_python_shared_udp_ifac_rns_config(
        &owner_rns,
        shared_port,
        owner_udp_port,
        remote_udp_port,
    );
    write_python_udp_rns_config_with_ifac(
        &remote_rns,
        remote_udp_port,
        owner_udp_port,
    );

    let mut owner_node = None;
    let mut remote_node = None;
    let mut rust_node = None;
    let outcome: Result<(), String> = (|| {
        owner_node = Some(spawn_python_endpoint(
            &python_bin,
            &reticulum_repo,
            &lxmf_repo,
            &helper_script,
            "python-ifac-shared-owner",
            "Python IFAC shared owner",
            &owner_rns,
            &owner_storage,
            owner_control_port,
            &mut [owner_control, shared_instance, owner_udp],
        ));
        wait_for_python_endpoint_ready(
            owner_control_port,
            owner_node.as_mut().expect("Python shared owner"),
            "python-ifac-shared-owner",
        )?;
        let owner_status = python_control_call(owner_control_port, "status", None)?;
        if owner_status
            .get("reticulum")
            .and_then(|reticulum| reticulum.get("is_shared_instance"))
            != Some(&Value::Bool(true))
        {
            return Err(format!("Python IFAC owner did not own the shared instance: {owner_status}"));
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
            rust_node.as_mut().expect("Rust IFAC shared client"),
            "rust-ifac-shared-client",
        )?;
        require_attached_local_client(rust_rpc_port, "Rust IFAC shared client")?;

        remote_node = Some(spawn_python_endpoint(
            &python_bin,
            &reticulum_repo,
            &lxmf_repo,
            &helper_script,
            "python-ifac-shared-remote",
            "Python IFAC remote peer",
            &remote_rns,
            &remote_storage,
            remote_control_port,
            &mut [remote_control, remote_udp],
        ));
        wait_for_python_endpoint_ready(
            remote_control_port,
            remote_node.as_mut().expect("Python IFAC remote peer"),
            "python-ifac-shared-remote",
        )?;

        let rust_status = daemon_status(rust_rpc_port)?;
        let rust_hash = status_hash(&rust_status)
            .ok_or_else(|| format!("missing Rust shared-client destination hash: {rust_status}"))?;
        let remote_status = python_control_call(remote_control_port, "status", None)?;
        let remote_hash = remote_status
            .get("delivery_destination_hash")
            .and_then(Value::as_str)
            .filter(|hash| !hash.is_empty())
            .ok_or_else(|| format!("missing remote Python destination hash: {remote_status}"))?
            .to_string();

        python_control_call(remote_control_port, "announce", None)?;
        wait_for_known_path_without_announce(rust_rpc_port, &remote_hash)?;
        rpc_call(rust_rpc_port, "announce_now", None)?;
        python_control_call(
            remote_control_port,
            "wait_path",
            Some(json!({ "destination": rust_hash, "timeout": 30.0 })),
        )?;

        rpc_call(
            rust_rpc_port,
            "send_message_v2",
            Some(json!({
                "id": "rust-shared-ifac-to-python",
                "source": rust_hash,
                "destination": remote_hash,
                "title": "",
                "content": "rust-shared-ifac-to-python",
                "method": "direct"
            })),
        )?;
        python_control_call(
            remote_control_port,
            "wait_message",
            Some(json!({ "content": "rust-shared-ifac-to-python", "timeout": 60.0 })),
        )?;

        python_control_call(
            remote_control_port,
            "send_message",
            Some(json!({
                "destination": rust_hash,
                "title": "",
                "content": "python-to-rust-shared-ifac"
            })),
        )?;
        wait_for_inbound_message(rust_rpc_port, "python-to-rust-shared-ifac")?;

        let owner_status = python_control_call(owner_control_port, "status", None)?;
        if owner_status.get("ifac_violations").and_then(Value::as_u64) != Some(0) {
            return Err(format!("valid shared IFAC traffic raised owner violations: {owner_status}"));
        }
        let client_status = daemon_status(rust_rpc_port)?;
        if transport_ifac_violations(&client_status) != Some(0) {
            return Err(format!("local shared-client traffic raised IFAC violations: {client_status}"));
        }
        Ok(())
    })();

    let failure_details = if let Err(err) = &outcome {
        Some(format!(
            "{err}\n\n{}\n\n{}\n\n{}",
            collect_node_diagnostics("rust-ifac-shared-client", rust_rpc_port, rust_node.as_mut()),
            collect_python_endpoint_diagnostics(
                "python-ifac-shared-owner",
                owner_control_port,
                owner_node.as_mut(),
            ),
            collect_python_endpoint_diagnostics(
                "python-ifac-shared-remote",
                remote_control_port,
                remote_node.as_mut(),
            ),
        ))
    } else {
        None
    };

    if let Some(node) = remote_node.as_mut() {
        terminate_child(&mut node.child);
    }
    if let Some(node) = rust_node.as_mut() {
        terminate_child(&mut node.child);
    }
    if let Some(node) = owner_node.as_mut() {
        terminate_child(&mut node.child);
    }

    if let Some(details) = failure_details {
        panic!("Python/Rust shared-instance IFAC flow failed:\n{details}");
    }
}
