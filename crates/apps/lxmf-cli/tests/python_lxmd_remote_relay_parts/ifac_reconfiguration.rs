const ROTATED_IFAC_PASSPHRASE: &str = "lxmf-rs-issue-605-ifac-rotated-secret";

fn udp_ifac_interface_with_passphrase(
    name: &str,
    listen_port: u16,
    target_port: u16,
    passphrase: &str,
) -> String {
    format!(
        "[[interfaces]]\ntype = \"udp\"\nenabled = true\nname = \"{name}\"\nhost = \"127.0.0.1\"\nport = {listen_port}\ntarget_host = \"127.0.0.1\"\ntarget_port = {target_port}\nifac_size = {IFAC_SIZE_BITS}\nnetwork_name = \"{IFAC_NETWORK_NAME}\"\npassphrase = \"{passphrase}\"\n"
    )
}

fn write_python_udp_rns_config_without_ifac(
    dir: &Path,
    listen_port: u16,
    forward_port: u16,
) {
    std::fs::create_dir_all(dir).expect("create Python plaintext UDP RNS dir");
    std::fs::write(
        dir.join("config"),
        format!(
            "[reticulum]\nenable_transport = no\nshare_instance = no\n\n[logging]\nloglevel = 7\n\n[interfaces]\n  [[UDP Plain Interface]]\n    type = UDPInterface\n    enabled = yes\n    listen_ip = 127.0.0.1\n    listen_port = {listen_port}\n    forward_ip = 127.0.0.1\n    forward_port = {forward_port}\n"
        ),
    )
    .expect("write Python plaintext UDP RNS config");
}

fn wait_for_python_announce_path(
    rust_rpc_port: u16,
    python_control_port: u16,
    destination: &str,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(45);
    let mut next_announce = Instant::now();
    let mut last_status = String::from("path status was not queried");
    while Instant::now() < deadline {
        if Instant::now() >= next_announce {
            python_control_call(python_control_port, "announce", None)?;
            next_announce = Instant::now() + Duration::from_secs(2);
        }
        let status = rpc_call(
            rust_rpc_port,
            "path_status",
            Some(json!({ "destination": destination })),
        )?;
        if status["known"].as_bool() == Some(true) || status["path_found"].as_bool() == Some(true) {
            return Ok(());
        }
        last_status = status.to_string();
        std::thread::sleep(Duration::from_millis(250));
    }

    Err(format!(
        "Python IFAC announce did not establish path to {destination}; last status: {last_status}"
    ))
}

#[test]
#[ignore = "requires local Python Reticulum/LXMF repos and daemon runtime"]
fn python_rust_lxmd_ifac_udp_credential_rotation_and_restart_e2e() {
    use std::net::UdpSocket;

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
    let mut rust_rpc = ReservedPort::reserve();
    let rust_udp_reservation = UdpSocket::bind(("127.0.0.1", 0)).expect("reserve Rust UDP port");
    let python_a_udp_reservation =
        UdpSocket::bind(("127.0.0.1", 0)).expect("reserve original Python UDP port");
    let python_b_udp_reservation =
        UdpSocket::bind(("127.0.0.1", 0)).expect("reserve rotated Python UDP port");
    let python_a_control = ReservedPort::reserve();
    let python_b_control = ReservedPort::reserve();
    let python_plain_control = ReservedPort::reserve();
    let rust_rpc_port = rust_rpc.port();
    let rust_udp_port = rust_udp_reservation.local_addr().expect("Rust UDP address").port();
    let python_a_udp_port =
        python_a_udp_reservation.local_addr().expect("original Python UDP address").port();
    let python_b_udp_port =
        python_b_udp_reservation.local_addr().expect("rotated Python UDP address").port();
    let python_a_control_port = python_a_control.port();
    let python_b_control_port = python_b_control.port();
    let python_plain_control_port = python_plain_control.port();

    let rust_dir = temp.path().join("rust-ifac-rotation-daemon");
    let python_a_storage = temp.path().join("python-ifac-original-storage");
    let python_a_rns = temp.path().join("python-ifac-original-rns");
    let python_b_storage = temp.path().join("python-ifac-rotated-storage");
    let python_b_rns = temp.path().join("python-ifac-rotated-rns");
    write_rust_config(
        &rust_dir,
        &rust_node_config(
            "rust-ifac-rotation-daemon",
            rust_rpc_port,
            None,
            &[udp_ifac_interface("ifac-udp", rust_udp_port, python_a_udp_port)],
        ),
    );
    write_python_udp_rns_config_with_ifac_passphrase(
        &python_a_rns,
        python_a_udp_port,
        rust_udp_port,
        IFAC_PASSPHRASE,
    );
    write_python_udp_rns_config_with_ifac_passphrase(
        &python_b_rns,
        python_b_udp_port,
        rust_udp_port,
        ROTATED_IFAC_PASSPHRASE,
    );

    let mut rust_node = None;
    let mut python_a_node = None;
    let mut python_b_node = None;
    let outcome: Result<(), String> = (|| {
        drop(rust_udp_reservation);
        rust_node = Some(spawn_lxmd(
            &lxmd_bin,
            &reticulumd_bin,
            rust_rpc_port,
            &rust_dir,
            std::slice::from_mut(&mut rust_rpc),
        ));
        wait_for_ready(
            rust_rpc_port,
            rust_node.as_mut().expect("Rust IFAC rotation daemon"),
            "rust-ifac-rotation-daemon",
        )?;

        drop(python_a_udp_reservation);
        python_a_node = Some(spawn_python_endpoint(
            &python_bin,
            &reticulum_repo,
            &lxmf_repo,
            &helper_script,
            "python-ifac-original-peer",
            "Python original IFAC peer",
            &python_a_rns,
            &python_a_storage,
            python_a_control_port,
            &mut [python_a_control],
        ));
        wait_for_python_endpoint_ready(
            python_a_control_port,
            python_a_node.as_mut().expect("original Python IFAC peer"),
            "python-ifac-original-peer",
        )?;

        let rust_status = daemon_status(rust_rpc_port)?;
        let rust_hash = status_hash(&rust_status)
            .ok_or_else(|| format!("missing Rust delivery destination hash: {rust_status}"))?;
        let python_a_status = python_control_call(python_a_control_port, "status", None)?;
        let python_a_hash = python_a_status
            .get("delivery_destination_hash")
            .and_then(Value::as_str)
            .filter(|hash| !hash.is_empty())
            .ok_or_else(|| format!("missing original Python destination hash: {python_a_status}"))?
            .to_string();

        python_control_call(python_a_control_port, "announce", None)?;
        wait_for_known_path_without_announce(rust_rpc_port, &python_a_hash)?;
        rpc_call(rust_rpc_port, "announce_now", None)?;
        python_control_call(
            python_a_control_port,
            "wait_path",
            Some(json!({ "destination": rust_hash, "timeout": 30.0 })),
        )?;
        python_control_call(
            python_a_control_port,
            "send_message",
            Some(json!({
                "destination": rust_hash,
                "title": "",
                "content": "ifac-before-credential-rotation"
            })),
        )?;
        wait_for_inbound_message(rust_rpc_port, "ifac-before-credential-rotation")?;

        let reconfiguration = rpc_call(
            rust_rpc_port,
            "set_interfaces",
            Some(json!({
                "interfaces": [{
                    "type": "udp",
                    "enabled": true,
                    "host": "127.0.0.1",
                    "port": rust_udp_port,
                    "name": "ifac-udp",
                    "settings": {
                        "target_host": "127.0.0.1",
                        "target_port": python_b_udp_port,
                        "ifac_size": IFAC_SIZE_BITS,
                        "network_name": IFAC_NETWORK_NAME,
                        "passphrase": ROTATED_IFAC_PASSPHRASE
                    }
                }]
            })),
        )?;
        if reconfiguration.get("updated").and_then(Value::as_bool) != Some(true) {
            return Err(format!("IFAC credential rotation was not applied: {reconfiguration}"));
        }

        drop(python_b_udp_reservation);
        python_b_node = Some(spawn_python_endpoint(
            &python_bin,
            &reticulum_repo,
            &lxmf_repo,
            &helper_script,
            "python-ifac-rotated-peer",
            "Python rotated IFAC peer",
            &python_b_rns,
            &python_b_storage,
            python_b_control_port,
            &mut [python_b_control],
        ));
        wait_for_python_endpoint_ready(
            python_b_control_port,
            python_b_node.as_mut().expect("rotated Python IFAC peer"),
            "python-ifac-rotated-peer",
        )?;
        let python_b_status = python_control_call(python_b_control_port, "status", None)?;
        let python_b_hash = python_b_status
            .get("delivery_destination_hash")
            .and_then(Value::as_str)
            .filter(|hash| !hash.is_empty())
            .ok_or_else(|| format!("missing rotated Python destination hash: {python_b_status}"))?
            .to_string();

        let rejected_ifac_reconfiguration = rpc_call(
            rust_rpc_port,
            "set_interfaces",
            Some(json!({
                "interfaces": [{
                    "type": "udp",
                    "enabled": true,
                    "host": "127.0.0.1",
                    "port": rust_udp_port,
                    "name": "ifac-udp",
                    "settings": {
                        "target_host": "127.0.0.1",
                        "target_port": python_b_udp_port,
                        "ifac_size": IFAC_SIZE_BITS
                    }
                }]
            })),
        );
        match rejected_ifac_reconfiguration {
            Err(error) => {
                let rpc_error: serde_json::Value = serde_json::from_str(&error)
                    .map_err(|_| "invalid IFAC update returned a malformed RPC error".to_string())?;
                let fields = rpc_error.as_array().ok_or_else(|| {
                    "invalid IFAC update returned an unexpected RPC error shape".to_string()
                })?;
                let code = fields.first().and_then(serde_json::Value::as_str);
                let message = fields.get(1).and_then(serde_json::Value::as_str);
                let machine_code = fields.get(2).and_then(serde_json::Value::as_str);
                if code != Some("CONFIG_INVALID_IFAC")
                    || message != Some("IFAC interface configuration was rejected")
                    || machine_code != Some("INVALID_IFAC_CONFIGURATION")
                {
                    return Err(format!(
                        "invalid IFAC reconfiguration returned unexpected RPC error fields: code={code:?}, message={message:?}, machine_code={machine_code:?}"
                    ));
                }
            }
            Ok(response) => {
                return Err(format!(
                    "invalid IFAC reconfiguration unexpectedly succeeded: {response}"
                ));
            }
        }

        wait_for_python_announce_path(rust_rpc_port, python_b_control_port, &python_b_hash)?;
        rpc_call(rust_rpc_port, "announce_now", None)?;
        python_control_call(
            python_b_control_port,
            "wait_path",
            Some(json!({ "destination": rust_hash, "timeout": 30.0 })),
        )?;
        python_control_call(
            python_b_control_port,
            "send_message",
            Some(json!({
                "destination": rust_hash,
                "title": "",
                "content": "ifac-after-credential-rotation"
            })),
        )?;
        wait_for_inbound_message(rust_rpc_port, "ifac-after-credential-rotation")?;

        let before_old_peer = daemon_status(rust_rpc_port)?;
        let baseline_violations = transport_ifac_violations(&before_old_peer)
            .ok_or_else(|| format!("missing IFAC violation counter: {before_old_peer}"))?;
        python_control_call(python_a_control_port, "announce", None)?;
        wait_for_ifac_violations_at_least(
            rust_rpc_port,
            baseline_violations.saturating_add(1),
        )?;

        write_rust_config(
            &rust_dir,
            &rust_node_config(
                "rust-ifac-rotation-daemon",
                rust_rpc_port,
                None,
                &[udp_ifac_interface_with_passphrase(
                    "ifac-udp",
                    rust_udp_port,
                    python_b_udp_port,
                    ROTATED_IFAC_PASSPHRASE,
                )],
            ),
        );
        if let Some(node) = rust_node.as_mut() {
            terminate_child(&mut node.child);
        }
        std::thread::sleep(Duration::from_secs(1));
        rust_node = Some(spawn_lxmd(
            &lxmd_bin,
            &reticulumd_bin,
            rust_rpc_port,
            &rust_dir,
            std::slice::from_mut(&mut rust_rpc),
        ));
        wait_for_ready(
            rust_rpc_port,
            rust_node.as_mut().expect("Rust IFAC daemon after restart"),
            "rust-ifac-rotation-daemon-after-restart",
        )?;

        let restarted_status = daemon_status(rust_rpc_port)?;
        let restarted_hash = status_hash(&restarted_status)
            .ok_or_else(|| format!("missing Rust destination after restart: {restarted_status}"))?;
        if restarted_hash != rust_hash {
            return Err(format!(
                "Rust IFAC daemon identity changed across restart: before={rust_hash} after={restarted_hash}"
            ));
        }
        rpc_call(rust_rpc_port, "announce_now", None)?;
        python_control_call(
            python_b_control_port,
            "wait_path",
            Some(json!({ "destination": restarted_hash, "timeout": 30.0 })),
        )?;
        python_control_call(
            python_b_control_port,
            "send_message",
            Some(json!({
                "destination": restarted_hash,
                "title": "",
                "content": "ifac-after-daemon-restart"
            })),
        )?;
        wait_for_inbound_message(rust_rpc_port, "ifac-after-daemon-restart")?;

        let before_old_peer_after_restart = daemon_status(rust_rpc_port)?;
        let restart_baseline = transport_ifac_violations(&before_old_peer_after_restart)
            .ok_or_else(|| format!("missing IFAC violation counter after restart: {before_old_peer_after_restart}"))?;
        python_control_call(python_a_control_port, "announce", None)?;
        wait_for_ifac_violations_at_least(rust_rpc_port, restart_baseline.saturating_add(1))?;

        if let Some(node) = python_b_node.as_mut() {
            terminate_child(&mut node.child);
        }
        write_python_udp_rns_config_without_ifac(
            &python_b_rns,
            python_b_udp_port,
            rust_udp_port,
        );
        python_b_node = Some(spawn_python_endpoint(
            &python_bin,
            &reticulum_repo,
            &lxmf_repo,
            &helper_script,
            "python-ifac-plaintext-peer",
            "Python plaintext IFAC peer",
            &python_b_rns,
            &python_b_storage,
            python_plain_control_port,
            &mut [python_plain_control],
        ));
        wait_for_python_endpoint_ready(
            python_plain_control_port,
            python_b_node.as_mut().expect("Python plaintext peer"),
            "python-ifac-plaintext-peer",
        )?;
        let before_plaintext_peer = daemon_status(rust_rpc_port)?;
        let plaintext_baseline = transport_ifac_violations(&before_plaintext_peer)
            .ok_or_else(|| format!("missing IFAC violation counter before plaintext peer: {before_plaintext_peer}"))?;
        python_control_call(python_plain_control_port, "announce", None)?;
        wait_for_ifac_violations_at_least(
            rust_rpc_port,
            plaintext_baseline.saturating_add(1),
        )?;
        Ok(())
    })();

    let failure_details = if let Err(err) = &outcome {
        Some(format!(
            "{err}\n\n{}\n\n{}\n\n{}",
            collect_node_diagnostics(
                "rust-ifac-rotation-daemon",
                rust_rpc_port,
                rust_node.as_mut(),
            ),
            collect_python_endpoint_diagnostics(
                "python-ifac-original-peer",
                python_a_control_port,
                python_a_node.as_mut(),
            ),
            collect_python_endpoint_diagnostics(
                "python-ifac-rotated-peer",
                python_b_control_port,
                python_b_node.as_mut(),
            ),
        ))
    } else {
        None
    };

    if let Some(node) = python_b_node.as_mut() {
        terminate_child(&mut node.child);
    }
    if let Some(node) = python_a_node.as_mut() {
        terminate_child(&mut node.child);
    }
    if let Some(node) = rust_node.as_mut() {
        terminate_child(&mut node.child);
    }

    if let Some(details) = failure_details {
        panic!("UDP IFAC credential rotation/restart flow failed:\n{details}");
    }
}
