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
    fn reload_config_rejects_mixed_startup_kind_diff_without_partial_apply() {
        let daemon = RpcDaemon::test_instance();
        daemon.replace_interfaces(vec![tcp_interface("primary", "127.0.0.1", 4242)]);

        let response = daemon
            .handle_rpc(rpc_request(
                3,
                "reload_config",
                json!({
                    "interfaces": [
                        {
                            "type": "tcp_client",
                            "enabled": true,
                            "host": "127.0.0.1",
                            "port": 4243,
                            "name": "primary"
                        },
                        {
                            "type": "lora",
                            "enabled": true,
                            "name": "lora-main",
                            "settings": {
                                "region": "US915"
                            }
                        }
                    ]
                }),
            ))
            .expect("reload_config response");

        let error = response.error.expect("expected restart-required error");
        assert_eq!(error.code, "CONFIG_RESTART_REQUIRED");

        let interfaces = daemon.interfaces.lock().expect("interfaces mutex poisoned").clone();
        assert_eq!(interfaces, vec![tcp_interface("primary", "127.0.0.1", 4242)]);
    }

    #[test]
    fn reload_config_hot_applies_legacy_tcp_only_diff() {
        let daemon = RpcDaemon::test_instance();
        daemon.replace_interfaces(vec![tcp_interface("primary", "127.0.0.1", 4242)]);

        let response = daemon
            .handle_rpc(rpc_request(
                4,
                "reload_config",
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
            .expect("reload_config response");

        assert!(response.error.is_none(), "unexpected reload error: {response:?}");
        let result = response.result.expect("result");
        assert_eq!(result["reloaded"], json!(true));
        assert_eq!(result["hot_applied_legacy_tcp_only"], json!(true));

        let interfaces = daemon.interfaces.lock().expect("interfaces mutex poisoned").clone();
        assert_eq!(interfaces[0].port, Some(4248));
    }

    #[test]
    fn reload_config_invokes_interface_mutation_bridge_for_hot_apply() {
        let daemon = RpcDaemon::test_instance();
        daemon.replace_interfaces(vec![tcp_interface("primary", "127.0.0.1", 4242)]);
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                23,
                "reload_config",
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
            .expect("reload_config response");

        assert!(response.error.is_none(), "unexpected reload error: {response:?}");
        let applied = bridge.applied();
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0], vec![tcp_interface("primary", "127.0.0.1", 4248)]);
    }

    #[test]
    fn reload_config_keeps_stored_interfaces_unchanged_when_bridge_fails() {
        let daemon = RpcDaemon::test_instance();
        daemon.replace_interfaces(vec![tcp_interface("primary", "127.0.0.1", 4242)]);
        daemon.set_interface_mutation_bridge(std::sync::Arc::new(FailingInterfaceMutationBridge));

        let err = daemon
            .handle_rpc(rpc_request(
                26,
                "reload_config",
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
    fn reload_config_rejects_empty_interface_set_with_affected_names() {
        let daemon = RpcDaemon::test_instance();
        daemon.replace_interfaces(vec![tcp_interface("primary", "127.0.0.1", 4242)]);

        let response = daemon
            .handle_rpc(rpc_request(
                5,
                "reload_config",
                json!({
                    "interfaces": []
                }),
            ))
            .expect("reload_config response");

        let error = response.error.expect("expected restart-required error");
        assert_eq!(error.code, "CONFIG_RESTART_REQUIRED");
        let details = error.details.expect("details must be present");
        let affected = details
            .get("affected_interfaces")
            .and_then(|value| value.as_array())
            .expect("affected interfaces array");
        assert!(!affected.is_empty(), "affected_interfaces must not be empty");
        assert!(
            affected.iter().any(|item| item.as_str() == Some("primary")),
            "affected interfaces should include removed interface name"
        );

        let interfaces = daemon.interfaces.lock().expect("interfaces mutex poisoned").clone();
        assert_eq!(interfaces, vec![tcp_interface("primary", "127.0.0.1", 4242)]);
    }
