#[test]
fn bootstrap_accepts_whitespace_ifac_credentials_like_pinned_python() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "udp", enabled = true, name = "ifac-whitespace-credentials", host = "127.0.0.1", port = 0, target_host = "127.0.0.1", target_port = 4242, ifac_size = 128, network_name = " ", pass_phrase = " " }
]
"#,
    )
    .expect("write whitespace IFAC config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(db_path, Some(config_path), None, false)).await
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        let response = context
            .daemon
            .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
            .expect("list_interfaces");
        let result = response.result.expect("result");
        let interfaces = result["interfaces"].as_array().expect("interfaces array");
        let interface = interfaces
            .iter()
            .find(|entry| {
                entry.get("name").and_then(|value| value.as_str())
                    == Some("ifac-whitespace-credentials")
            })
            .expect("configured IFAC UDP interface");
        let settings = interface.get("settings").expect("interface settings");
        assert_eq!(
            settings.get("network_name").and_then(|value| value.as_str()),
            Some(" "),
            "IFAC config should preserve Python's non-empty whitespace network name"
        );
        assert_eq!(
            settings.get("passphrase").and_then(|value| value.as_str()),
            Some(" "),
            "IFAC config should preserve Python's non-empty whitespace passphrase"
        );
        let runtime_status = settings.get("_runtime").expect("runtime status");
        assert_ne!(
            runtime_status.get("startup_status").and_then(|value| value.as_str()),
            Some("failed"),
            "pinned Python treats any non-empty IFAC credential literally, including whitespace"
        );
        let udp_status = runtime_status
            .get("udp")
            .and_then(|value| value.get("status"))
            .expect("UDP worker status");
        if udp_status.get("link_state").and_then(|value| value.as_str()) == Some("bound") {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "UDP listener did not bind with Python-compatible whitespace IFAC credentials: {udp_status}"
        );
        runtime.block_on(async { tokio::time::sleep(Duration::from_millis(10)).await });
    }

    drop(context);
}
