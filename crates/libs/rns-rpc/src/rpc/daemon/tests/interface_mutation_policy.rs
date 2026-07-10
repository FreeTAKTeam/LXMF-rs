    fn tcp_interface(name: &str, host: &str, port: u16) -> InterfaceRecord {
        InterfaceRecord {
            kind: "tcp_client".to_string(),
            enabled: true,
            host: Some(host.to_string()),
            port: Some(port),
            name: Some(name.to_string()),
            settings: None,
        }
    }

    fn tcp_server_interface(name: &str, host: &str, port: u16) -> InterfaceRecord {
        InterfaceRecord {
            kind: "tcp_server".to_string(),
            enabled: true,
            host: Some(host.to_string()),
            port: Some(port),
            name: Some(name.to_string()),
            settings: None,
        }
    }

    fn udp_interface(name: &str, host: &str, port: u16) -> InterfaceRecord {
        InterfaceRecord {
            kind: "udp".to_string(),
            enabled: true,
            host: Some(host.to_string()),
            port: Some(port),
            name: Some(name.to_string()),
            settings: None,
        }
    }

    fn udp_forward_interface(
        name: &str,
        host: &str,
        port: u16,
        target_host: &str,
        target_port: u16,
    ) -> InterfaceRecord {
        let mut iface = udp_interface(name, host, port);
        iface.settings = Some(json!({
            "target_host": target_host,
            "target_port": target_port
        }));
        iface
    }

    fn pipe_interface(name: &str, command: &str) -> InterfaceRecord {
        InterfaceRecord {
            kind: "pipe".to_string(),
            enabled: true,
            host: None,
            port: None,
            name: Some(name.to_string()),
            settings: Some(json!({ "command": command })),
        }
    }

    struct RecordingInterfaceMutationBridge {
        applied: std::sync::Mutex<Vec<Vec<InterfaceRecord>>>,
    }

    impl RecordingInterfaceMutationBridge {
        fn new() -> Self {
            Self { applied: std::sync::Mutex::new(Vec::new()) }
        }

        fn applied(&self) -> Vec<Vec<InterfaceRecord>> {
            self.applied.lock().expect("applied mutex poisoned").clone()
        }
    }

    impl InterfaceMutationBridge for RecordingInterfaceMutationBridge {
        fn apply_interfaces(
            &self,
            interfaces: Vec<InterfaceRecord>,
        ) -> Result<Vec<InterfaceRecord>, std::io::Error> {
            self.applied.lock().expect("applied mutex poisoned").push(interfaces.clone());
            Ok(interfaces)
        }
    }

    struct FailingInterfaceMutationBridge;

    impl InterfaceMutationBridge for FailingInterfaceMutationBridge {
        fn apply_interfaces(
            &self,
            _interfaces: Vec<InterfaceRecord>,
        ) -> Result<Vec<InterfaceRecord>, std::io::Error> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "interface mutation worker is not running",
            ))
        }
    }

    fn assert_restart_required(response: RpcResponse) {
        let error = response.error.expect("expected restart-required error");
        assert_eq!(error.code, "CONFIG_RESTART_REQUIRED");
        assert_eq!(
            error.machine_code.as_deref(),
            Some("UNSUPPORTED_MUTATION_KIND_REQUIRES_RESTART")
        );
    }

    fn assert_reload_restart_required_without_apply(
        daemon: &RpcDaemon,
        bridge: &RecordingInterfaceMutationBridge,
        response: RpcResponse,
        expected_interfaces: Vec<InterfaceRecord>,
    ) {
        assert_restart_required(response);
        assert!(bridge.applied().is_empty(), "restart-required reload must not hot-apply");
        let interfaces = daemon.interfaces.lock().expect("interfaces mutex poisoned").clone();
        assert_eq!(interfaces, expected_interfaces);
    }

    #[test]
    fn set_interfaces_rejects_startup_only_interface_kinds() {
        let daemon = RpcDaemon::test_instance();
        daemon.replace_interfaces(vec![tcp_interface("primary", "127.0.0.1", 4242)]);

        let response = daemon
            .handle_rpc(rpc_request(
                1,
                "set_interfaces",
                json!({
                    "interfaces": [
                        {
                            "type": "tcp_client",
                            "enabled": true,
                            "host": "127.0.0.1",
                            "port": 4242,
                            "name": "primary"
                        },
                        {
                            "type": "ble_gatt",
                            "enabled": true,
                            "name": "ble-main",
                            "settings": {
                                "peripheral_id": "AA:BB:CC"
                            }
                        },
                        {
                            "type": "local",
                            "enabled": true,
                            "host": "127.0.0.1",
                            "port": 37428,
                            "name": "local-main"
                        }
                    ]
                }),
            ))
            .expect("set_interfaces response");

        let error = response.error.expect("expected restart-required error");
        assert_eq!(error.code, "CONFIG_RESTART_REQUIRED");
        assert_eq!(
            error.machine_code.as_deref(),
            Some("UNSUPPORTED_MUTATION_KIND_REQUIRES_RESTART")
        );

        let interfaces = daemon.interfaces.lock().expect("interfaces mutex poisoned").clone();
        assert_eq!(interfaces, vec![tcp_interface("primary", "127.0.0.1", 4242)]);
    }

    #[test]
    fn set_interfaces_updates_legacy_tcp_entries() {
        let daemon = RpcDaemon::test_instance();

        let response = daemon
            .handle_rpc(rpc_request(
                2,
                "set_interfaces",
                json!({
                    "interfaces": [
                        {
                            "type": "tcp_client",
                            "enabled": true,
                            "host": "rmap.world",
                            "port": 4242,
                            "name": "rmap"
                        }
                    ]
                }),
            ))
            .expect("set_interfaces response");

        assert!(response.error.is_none(), "unexpected error: {response:?}");
        assert_eq!(response.result.expect("result")["updated"], json!(true));

        let interfaces = daemon.interfaces.lock().expect("interfaces mutex poisoned").clone();
        assert_eq!(interfaces.len(), 1);
        assert_eq!(interfaces[0].kind, "tcp_client");
        assert_eq!(interfaces[0].host.as_deref(), Some("rmap.world"));
        assert_eq!(interfaces[0].port, Some(4242));
    }

    #[test]
    fn set_interfaces_invokes_interface_mutation_bridge_for_legacy_tcp() {
        let daemon = RpcDaemon::test_instance();
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                22,
                "set_interfaces",
                json!({
                    "interfaces": [
                        {
                            "type": "tcp_client",
                            "enabled": true,
                            "host": "rmap.world",
                            "port": 4242,
                            "name": "rmap"
                        }
                    ]
                }),
            ))
            .expect("set_interfaces response");

        assert!(response.error.is_none(), "unexpected error: {response:?}");
        let applied = bridge.applied();
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0], vec![tcp_interface("rmap", "rmap.world", 4242)]);
    }

    #[test]
    fn set_interfaces_hot_applies_loopback_tcp_server_records() {
        let daemon = RpcDaemon::test_instance();
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                27,
                "set_interfaces",
                json!({
                    "interfaces": [
                        {
                            "type": "tcp_server",
                            "enabled": true,
                            "host": "127.0.0.1",
                            "port": 4242,
                            "name": "listener"
                        }
                    ]
                }),
            ))
            .expect("set_interfaces response");

        assert!(response.error.is_none(), "unexpected error: {response:?}");
        assert_eq!(response.result.expect("result")["updated"], json!(true));
        assert_eq!(
            bridge.applied(),
            vec![vec![tcp_server_interface("listener", "127.0.0.1", 4242)]]
        );

        let interfaces = daemon.interfaces.lock().expect("interfaces mutex poisoned").clone();
        assert_eq!(interfaces, vec![tcp_server_interface("listener", "127.0.0.1", 4242)]);
    }

    #[test]
    fn set_interfaces_hot_applies_empty_interface_set() {
        let daemon = RpcDaemon::test_instance();
        daemon.replace_interfaces(vec![tcp_server_interface("listener", "127.0.0.1", 4242)]);
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                28,
                "set_interfaces",
                json!({
                    "interfaces": []
                }),
            ))
            .expect("set_interfaces response");

        assert!(response.error.is_none(), "unexpected error: {response:?}");
        assert_eq!(response.result.expect("result")["updated"], json!(true));
        assert_eq!(bridge.applied(), vec![Vec::<InterfaceRecord>::new()]);
        assert!(daemon.interfaces.lock().expect("interfaces mutex poisoned").is_empty());
    }

    #[test]
    fn set_interfaces_hot_applies_non_loopback_tcp_server() {
        let daemon = RpcDaemon::test_instance();
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                127,
                "set_interfaces",
                json!({
                    "interfaces": [
                        {
                            "type": "tcp_server",
                            "enabled": true,
                            "host": "192.0.2.1",
                            "port": 4242,
                            "name": "listener"
                        }
                    ]
                }),
            ))
            .expect("set_interfaces response");

        assert!(response.error.is_none(), "unexpected error: {response:?}");
        assert_eq!(
            bridge.applied(),
            vec![vec![tcp_server_interface("listener", "192.0.2.1", 4242)]]
        );
    }

    #[test]
    fn set_interfaces_hot_applies_localhost_tcp_server_records() {
        let daemon = RpcDaemon::test_instance();
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                130,
                "set_interfaces",
                json!({
                    "interfaces": [
                        {
                            "type": "tcp_server",
                            "enabled": true,
                            "host": "localhost",
                            "port": 4242,
                            "name": "listener"
                        }
                    ]
                }),
            ))
            .expect("set_interfaces response");

        assert!(response.error.is_none(), "unexpected error: {response:?}");
        assert_eq!(
            bridge.applied(),
            vec![vec![tcp_server_interface("listener", "localhost", 4242)]]
        );

        let interfaces = daemon.interfaces.lock().expect("interfaces mutex poisoned").clone();
        assert_eq!(interfaces, vec![tcp_server_interface("listener", "localhost", 4242)]);
    }

    #[test]
    fn set_interfaces_hot_applies_wildcard_tcp_server_records() {
        let daemon = RpcDaemon::test_instance();
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                136,
                "set_interfaces",
                json!({
                    "interfaces": [
                        {
                            "type": "tcp_server",
                            "enabled": true,
                            "host": "0.0.0.0",
                            "port": 4242,
                            "name": "listener"
                        }
                    ]
                }),
            ))
            .expect("set_interfaces response");

        assert!(response.error.is_none(), "unexpected error: {response:?}");
        assert_eq!(
            bridge.applied(),
            vec![vec![tcp_server_interface("listener", "0.0.0.0", 4242)]]
        );

        let interfaces = daemon.interfaces.lock().expect("interfaces mutex poisoned").clone();
        assert_eq!(interfaces, vec![tcp_server_interface("listener", "0.0.0.0", 4242)]);
    }

    #[test]
    fn set_interfaces_hot_applies_hostname_tcp_server() {
        let daemon = RpcDaemon::test_instance();
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                133,
                "set_interfaces",
                json!({
                    "interfaces": [
                        {
                            "type": "tcp_server",
                            "enabled": true,
                            "host": "example.invalid",
                            "port": 4242,
                            "name": "listener"
                        }
                    ]
                }),
            ))
            .expect("set_interfaces response");

        assert!(response.error.is_none(), "unexpected error: {response:?}");
        assert_eq!(
            bridge.applied(),
            vec![vec![tcp_server_interface("listener", "example.invalid", 4242)]]
        );
    }

    #[test]
    fn set_interfaces_hot_applies_device_tcp_server() {
        let daemon = RpcDaemon::test_instance();
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                128,
                "set_interfaces",
                json!({
                    "interfaces": [
                        {
                            "type": "tcp_server",
                            "enabled": true,
                            "host": "127.0.0.1",
                            "port": 4242,
                            "name": "listener",
                            "settings": {
                                "device": "eth0"
                            }
                        }
                    ]
                }),
            ))
            .expect("set_interfaces response");

        assert!(response.error.is_none(), "unexpected error: {response:?}");
        let mut expected = tcp_server_interface("listener", "127.0.0.1", 4242);
        expected.settings = Some(json!({ "device": "eth0" }));
        assert_eq!(bridge.applied(), vec![vec![expected]]);
    }

    #[test]
    fn set_interfaces_hot_applies_device_only_tcp_server() {
        let daemon = RpcDaemon::test_instance();
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                1_280,
                "set_interfaces",
                json!({
                    "interfaces": [{
                        "type": "tcp_server",
                        "enabled": true,
                        "port": 4242,
                        "name": "listener",
                        "settings": {
                            "device": "eth0",
                            "prefer_ipv6": true
                        }
                    }]
                }),
            ))
            .expect("set_interfaces response");

        assert!(response.error.is_none(), "unexpected error: {response:?}");
        let expected = InterfaceRecord {
            kind: "tcp_server".to_string(),
            enabled: true,
            host: None,
            port: Some(4242),
            name: Some("listener".to_string()),
            settings: Some(json!({ "device": "eth0", "prefer_ipv6": true })),
        };
        assert_eq!(bridge.applied(), vec![vec![expected]]);
    }

    #[test]
    fn set_interfaces_hot_applies_prefer_ipv6_tcp_server() {
        let daemon = RpcDaemon::test_instance();
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                137,
                "set_interfaces",
                json!({
                    "interfaces": [
                        {
                            "type": "tcp_server",
                            "enabled": true,
                            "host": "127.0.0.1",
                            "port": 4242,
                            "name": "listener",
                            "settings": {
                                "prefer_ipv6": true
                            }
                        }
                    ]
                }),
            ))
            .expect("set_interfaces response");

        assert!(response.error.is_none(), "unexpected error: {response:?}");
        let mut expected = tcp_server_interface("listener", "127.0.0.1", 4242);
        expected.settings = Some(json!({ "prefer_ipv6": true }));
        assert_eq!(bridge.applied(), vec![vec![expected]]);
    }

    #[test]
    fn set_interfaces_rejects_duplicate_tcp_server_bind_addresses() {
        let daemon = RpcDaemon::test_instance();

        let err = daemon
            .handle_rpc(rpc_request(
                129,
                "set_interfaces",
                json!({
                    "interfaces": [
                        {
                            "type": "tcp_server",
                            "enabled": true,
                            "host": "127.0.0.1",
                            "port": 4242,
                            "name": "listener-a"
                        },
                        {
                            "type": "tcp_server",
                            "enabled": true,
                            "host": "127.0.0.1",
                            "port": 4242,
                            "name": "listener-b"
                        }
                    ]
                }),
            ))
            .expect_err("duplicate tcp_server binds should be rejected");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("duplicate legacy tcp_server bind address"));
    }

    #[test]
    fn set_interfaces_rejects_duplicate_localhost_tcp_server_alias_bind_addresses() {
        let daemon = RpcDaemon::test_instance();

        let err = daemon
            .handle_rpc(rpc_request(
                134,
                "set_interfaces",
                json!({
                    "interfaces": [
                        {
                            "type": "tcp_server",
                            "enabled": true,
                            "host": "localhost",
                            "port": 4242,
                            "name": "listener-a"
                        },
                        {
                            "type": "tcp_server",
                            "enabled": true,
                            "host": "localhost",
                            "port": 4242,
                            "name": "listener-b"
                        }
                    ]
                }),
            ))
            .expect_err("duplicate tcp_server localhost alias binds should be rejected");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("duplicate legacy tcp_server bind address"));
    }

    #[test]
    fn set_interfaces_rejects_duplicate_ipv6_tcp_server_bind_addresses() {
        let daemon = RpcDaemon::test_instance();

        let err = daemon
            .handle_rpc(rpc_request(
                131,
                "set_interfaces",
                json!({
                    "interfaces": [
                        {
                            "type": "tcp_server",
                            "enabled": true,
                            "host": "::1",
                            "port": 4242,
                            "name": "listener-a"
                        },
                        {
                            "type": "tcp_server",
                            "enabled": true,
                            "host": "[::1]",
                            "port": 4242,
                            "name": "listener-b"
                        }
                    ]
                }),
            ))
            .expect_err("duplicate tcp_server IPv6 binds should be rejected");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("duplicate legacy tcp_server bind address"));
    }

    #[test]
    fn set_interfaces_rejects_duplicate_wildcard_tcp_server_bind_addresses() {
        let daemon = RpcDaemon::test_instance();

        let err = daemon
            .handle_rpc(rpc_request(
                137,
                "set_interfaces",
                json!({
                    "interfaces": [
                        {
                            "type": "tcp_server",
                            "enabled": true,
                            "host": "0.0.0.0",
                            "port": 4242,
                            "name": "listener-a"
                        },
                        {
                            "type": "tcp_server",
                            "enabled": true,
                            "host": "127.0.0.1",
                            "port": 4242,
                            "name": "listener-b"
                        }
                    ]
                }),
            ))
            .expect_err("duplicate tcp_server wildcard binds should be rejected");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("duplicate legacy tcp_server bind address"));
    }

    #[test]
    fn set_interfaces_invokes_interface_mutation_bridge_for_udp() {
        let daemon = RpcDaemon::test_instance();
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                30,
                "set_interfaces",
                json!({
                    "interfaces": [{
                        "type": "udp",
                        "enabled": true,
                        "host": "127.0.0.1",
                        "port": 4242,
                        "name": "udp-main"
                    }]
                }),
            ))
            .expect("set_interfaces response");

        assert!(response.error.is_none(), "unexpected error: {response:?}");
        assert_eq!(bridge.applied(), vec![vec![udp_interface("udp-main", "127.0.0.1", 4242)]]);
    }

    #[test]
    fn set_interfaces_hot_applies_pipe_records() {
        let daemon = RpcDaemon::test_instance();
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                37,
                "set_interfaces",
                json!({
                    "interfaces": [{
                        "type": "pipe",
                        "enabled": true,
                        "name": "pipe-cat",
                        "settings": { "command": "cat" }
                    }]
                }),
            ))
            .expect("set_interfaces response");

        assert!(response.error.is_none(), "unexpected error: {response:?}");
        assert_eq!(bridge.applied(), vec![vec![pipe_interface("pipe-cat", "cat")]]);
        let interfaces = daemon.interfaces.lock().expect("interfaces mutex poisoned").clone();
        assert_eq!(interfaces, vec![pipe_interface("pipe-cat", "cat")]);
    }

    #[test]
    fn set_interfaces_hot_applies_multicast_udp_records() {
        let daemon = RpcDaemon::test_instance();
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                31,
                "set_interfaces",
                json!({
                    "interfaces": [{
                        "type": "udp",
                        "enabled": true,
                        "host": "239.255.0.1",
                        "port": 4242,
                        "name": "udp-mcast"
                    }]
                }),
            ))
            .expect("set_interfaces response");

        assert!(response.error.is_none(), "unexpected error: {response:?}");
        assert_eq!(
            bridge.applied(),
            vec![vec![udp_interface("udp-mcast", "239.255.0.1", 4242)]]
        );
    }

    #[test]
    fn set_interfaces_hot_applies_device_udp() {
        let daemon = RpcDaemon::test_instance();
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                33,
                "set_interfaces",
                json!({
                    "interfaces": [{
                        "type": "udp",
                        "enabled": true,
                        "host": "127.0.0.1",
                        "port": 4242,
                        "name": "udp-device",
                        "settings": {
                            "device": "eth0"
                        }
                    }]
                }),
            ))
            .expect("set_interfaces response");

        assert!(response.error.is_none(), "unexpected error: {response:?}");
        let mut expected = udp_interface("udp-device", "127.0.0.1", 4242);
        expected.settings = Some(json!({ "device": "eth0" }));
        assert_eq!(bridge.applied(), vec![vec![expected]]);
    }

    #[test]
    fn set_interfaces_hot_applies_device_only_udp() {
        let daemon = RpcDaemon::test_instance();
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                330,
                "set_interfaces",
                json!({
                    "interfaces": [{
                        "type": "udp",
                        "enabled": true,
                        "port": 4242,
                        "name": "udp-device-only",
                        "settings": { "device": "eth0" }
                    }]
                }),
            ))
            .expect("set_interfaces response");

        assert!(response.error.is_none(), "unexpected error: {response:?}");
        let expected = InterfaceRecord {
            kind: "udp".to_string(),
            enabled: true,
            host: None,
            port: Some(4242),
            name: Some("udp-device-only".to_string()),
            settings: Some(json!({ "device": "eth0" })),
        };
        assert_eq!(bridge.applied(), vec![vec![expected]]);
    }

    #[test]
    fn set_interfaces_reports_out_of_range_udp_forward_port_requires_restart() {
        let daemon = RpcDaemon::test_instance();

        let response = daemon
            .handle_rpc(rpc_request(
                34,
                "set_interfaces",
                json!({
                    "interfaces": [{
                        "type": "udp",
                        "enabled": true,
                        "host": "127.0.0.1",
                        "port": 4242,
                        "name": "udp-peer",
                        "settings": {
                            "target_host": "127.0.0.1",
                            "target_port": 70000
                        }
                    }]
                }),
            ))
            .expect("set_interfaces response");

        let error = response.error.expect("expected restart-required error");
        assert_eq!(error.code, "CONFIG_RESTART_REQUIRED");
        assert_eq!(
            error.machine_code.as_deref(),
            Some("UNSUPPORTED_MUTATION_KIND_REQUIRES_RESTART")
        );
    }

    #[test]
    fn set_interfaces_reports_partial_udp_forward_target_requires_restart() {
        let daemon = RpcDaemon::test_instance();

        let response = daemon
            .handle_rpc(rpc_request(
                35,
                "set_interfaces",
                json!({
                    "interfaces": [{
                        "type": "udp",
                        "enabled": true,
                        "host": "127.0.0.1",
                        "port": 4242,
                        "name": "udp-peer",
                        "settings": {
                            "target_host": "127.0.0.1"
                        }
                    }]
                }),
            ))
            .expect("set_interfaces response");

        assert_restart_required(response);
    }

    #[test]
    fn set_interfaces_hot_applies_multicast_udp_forward_target() {
        let daemon = RpcDaemon::test_instance();
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                36,
                "set_interfaces",
                json!({
                    "interfaces": [{
                        "type": "udp",
                        "enabled": true,
                        "host": "127.0.0.1",
                        "port": 4242,
                        "name": "udp-peer",
                        "settings": {
                            "target_host": "239.255.0.1",
                            "target_port": 4242
                        }
                    }]
                }),
            ))
            .expect("set_interfaces response");

        assert!(response.error.is_none(), "unexpected error: {response:?}");
        assert_eq!(
            bridge.applied(),
            vec![vec![udp_forward_interface(
                "udp-peer",
                "127.0.0.1",
                4242,
                "239.255.0.1",
                4242
            )]]
        );
    }

    #[test]
    fn set_interfaces_keeps_stored_interfaces_unchanged_when_bridge_fails() {
        let daemon = RpcDaemon::test_instance();
        daemon.replace_interfaces(vec![tcp_interface("primary", "127.0.0.1", 4242)]);
        daemon.set_interface_mutation_bridge(std::sync::Arc::new(FailingInterfaceMutationBridge));

        let err = daemon
            .handle_rpc(rpc_request(
                24,
                "set_interfaces",
                json!({
                    "interfaces": [
                        {
                            "type": "tcp_client",
                            "enabled": true,
                            "host": "127.0.0.1",
                            "port": 4248,
                            "name": "primary"
                        }
                    ]
                }),
            ))
            .expect_err("bridge failure should bubble up");
        assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);

        let interfaces = daemon.interfaces.lock().expect("interfaces mutex poisoned").clone();
        assert_eq!(interfaces, vec![tcp_interface("primary", "127.0.0.1", 4242)]);
    }

    #[test]
    fn set_interfaces_rejects_duplicate_legacy_tcp_keys() {
        let daemon = RpcDaemon::test_instance();

        let err = daemon
            .handle_rpc(rpc_request(
                25,
                "set_interfaces",
                json!({
                    "interfaces": [
                        {
                            "type": "tcp_client",
                            "enabled": true,
                            "host": "127.0.0.1",
                            "port": 4242,
                            "name": "duplicate"
                        },
                        {
                            "type": "tcp_client",
                            "enabled": true,
                            "host": "127.0.0.1",
                            "port": 4243,
                            "name": "duplicate"
                        }
                    ]
                }),
            ))
            .expect_err("duplicate keys should be rejected");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);

        let interfaces = daemon.interfaces.lock().expect("interfaces mutex poisoned").clone();
        assert!(interfaces.is_empty());
    }

    #[test]
    fn set_interfaces_rejects_duplicate_udp_bind_addresses() {
        let daemon = RpcDaemon::test_instance();

        let err = daemon
            .handle_rpc(rpc_request(
                35,
                "set_interfaces",
                json!({
                    "interfaces": [
                        {
                            "type": "udp",
                            "enabled": true,
                            "host": "127.0.0.1",
                            "port": 4242,
                            "name": "udp-a"
                        },
                        {
                            "type": "udp",
                            "enabled": true,
                            "host": "127.0.0.1",
                            "port": 4242,
                            "name": "udp-b"
                        }
                    ]
                }),
            ))
            .expect_err("duplicate udp binds should be rejected");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("duplicate legacy udp bind address"));
    }
