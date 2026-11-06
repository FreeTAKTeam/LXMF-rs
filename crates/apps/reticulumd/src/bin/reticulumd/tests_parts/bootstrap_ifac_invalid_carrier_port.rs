#[test]
fn bootstrap_reports_ifac_interface_rejected_by_invalid_carrier_port() {
    for (kind, credentials) in [
        ("udp", "network_name = 'credential-only-secret'"),
        ("i2p", "ifac_netname = 'i2p-secret'"),
        ("I2PInterface", "ifac_netkey = 'i2p-key-secret'"),
        ("udp", "ifac_size = -1, network_name = 'negative-default-secret'"),
        ("udp", "ifac_size = 16, network_name = '   '"),
    ] {
        check_invalid_ifac_carrier_port(kind, credentials);
    }
}

fn check_invalid_ifac_carrier_port(kind: &str, credentials: &str) {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        format!(r#"
interfaces = [
  {{ type = "{kind}", enabled = true, name = "invalid-ifac-port", host = "127.0.0.1", port = 70000, target_host = "127.0.0.1", target_port = 4242, {credentials} }}
]
"#),
    )
    .expect("write invalid carrier config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(db_path, Some(config_path), None, false)).await
    });
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let result = response.result.expect("result");
    assert!(!result.to_string().contains("secret"), "RPC must redact rejected credentials");
    let interfaces = result
        .get("interfaces")
        .and_then(|value| value.as_array())
        .expect("interfaces array");
    assert_eq!(interfaces.len(), 1, "rejected configured row remains visible exactly once");
    let rejected = interfaces
        .iter()
        .find(|entry| entry.get("name").and_then(|value| value.as_str()) == Some("invalid-ifac-port"))
        .expect("invalid configured IFAC interface should remain visible");
    assert_eq!(rejected.get("enabled").and_then(|value| value.as_bool()), Some(true));
    let runtime = rejected
        .get("settings")
        .and_then(|value| value.get("_runtime"))
        .expect("runtime startup diagnostic");
    assert_eq!(runtime.get("startup_status").and_then(|value| value.as_str()), Some("failed"));
    let error = runtime
        .get("startup_error")
        .and_then(|value| value.as_str())
        .expect("safe configuration error");
    assert_eq!(
        error,
        "daemon configuration rejected before interface startup; inspect daemon log"
    );
    assert!(!error.contains("secret"));
}
