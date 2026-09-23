#[test]
fn bootstrap_reports_bind_failure_for_ifac_enabled_udp_interface() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    let occupied = std::net::UdpSocket::bind("127.0.0.1:0").expect("reserve UDP port");
    let port = occupied.local_addr().expect("reserved UDP address").port();
    fs::write(
        &config_path,
        format!(
            r#"
interfaces = [
  {{ type = "udp", enabled = true, name = "ifac-startup", host = "127.0.0.1", port = {port}, target_host = "127.0.0.1", target_port = 4242, ifac_size = 128, network_name = "startup-test", passphrase = "non-secret-test-value" }}
]
"#
        ),
    )
    .expect("write IFAC UDP config");

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
        let interfaces = result
            .get("interfaces")
            .and_then(|value| value.as_array())
            .expect("interfaces array");
        let interface = interfaces
            .iter()
            .find(|entry| entry.get("name").and_then(|value| value.as_str()) == Some("ifac-startup"))
            .expect("IFAC UDP interface");
        let runtime_status = interface
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .expect("runtime status");
        let udp_status = runtime_status
            .get("udp")
            .and_then(|value| value.get("status"))
            .expect("UDP status");

        if udp_status.get("link_state").and_then(|value| value.as_str()) == Some("bind_failed") {
            assert_eq!(
                runtime_status.get("startup_status").and_then(|value| value.as_str()),
                Some("spawned"),
                "daemon should expose worker-level failure separately from interface creation"
            );
            let error = udp_status
                .get("last_error")
                .and_then(|value| value.as_str())
                .expect("reported UDP bind error");
            assert!(!error.is_empty());
            assert!(
                !error.contains("non-secret-test-value"),
                "runtime error must not expose the configured IFAC passphrase"
            );
            break;
        }

        assert!(std::time::Instant::now() < deadline, "UDP worker did not report bind failure: {udp_status}");
        runtime.block_on(async { tokio::time::sleep(Duration::from_millis(10)).await });
    }

    drop(context);
    drop(occupied);
}
