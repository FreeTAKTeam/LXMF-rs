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
    fn reload_config_hot_applies_udp_only_diff() {
        let daemon = RpcDaemon::test_instance();
        daemon.replace_interfaces(vec![udp_interface("udp-main", "127.0.0.1", 4242)]);
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                32,
                "reload_config",
                json!({
                    "interfaces": [{
                        "type": "udp",
                        "enabled": true,
                        "host": "127.0.0.1",
                        "port": 4248,
                        "name": "udp-main"
                    }]
                }),
            ))
            .expect("reload_config response");

        assert!(response.error.is_none(), "unexpected reload error: {response:?}");
        let result = response.result.expect("result");
        assert_eq!(result["hot_applied_legacy_tcp_only"], json!(false));
        assert_eq!(result["hot_applied_interface_mutation"], json!(true));
        assert_eq!(bridge.applied(), vec![vec![udp_interface("udp-main", "127.0.0.1", 4248)]]);
    }

    #[test]
    fn reload_config_hot_applies_pipe_only_diff() {
        let daemon = RpcDaemon::test_instance();
        daemon.replace_interfaces(vec![pipe_interface("pipe-cat", "cat")]);
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                38,
                "reload_config",
                json!({
                    "interfaces": [{
                        "type": "pipe",
                        "enabled": true,
                        "name": "pipe-cat",
                        "settings": { "command": "cat -u" }
                    }]
                }),
            ))
            .expect("reload_config response");

        assert!(response.error.is_none(), "unexpected reload error: {response:?}");
        let result = response.result.expect("result");
        assert_eq!(result["hot_applied_legacy_tcp_only"], json!(false));
        assert_eq!(result["hot_applied_interface_mutation"], json!(true));
        assert_eq!(bridge.applied(), vec![vec![pipe_interface("pipe-cat", "cat -u")]]);
    }

    #[test]
    fn reload_config_hot_applies_multicast_udp_only_diff() {
        let daemon = RpcDaemon::test_instance();
        daemon.replace_interfaces(vec![udp_interface("udp-mcast", "239.255.0.1", 4242)]);
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                34,
                "reload_config",
                json!({
                    "interfaces": [{
                        "type": "udp",
                        "enabled": true,
                        "host": "239.255.0.1",
                        "port": 4248,
                        "name": "udp-mcast"
                    }]
                }),
            ))
            .expect("reload_config response");

        assert!(response.error.is_none(), "unexpected reload error: {response:?}");
        let result = response.result.expect("result");
        assert_eq!(result["hot_applied_legacy_tcp_only"], json!(false));
        assert_eq!(result["hot_applied_interface_mutation"], json!(true));
        assert_eq!(
            bridge.applied(),
            vec![vec![udp_interface("udp-mcast", "239.255.0.1", 4248)]]
        );
    }

    #[test]
    fn reload_config_hot_applies_udp_forward_ip_with_shared_port() {
        let daemon = RpcDaemon::test_instance();
        daemon.replace_interfaces(vec![udp_interface("udp-peer", "127.0.0.1", 4242)]);
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                36,
                "reload_config",
                json!({
                    "interfaces": [{
                        "type": "udp",
                        "enabled": true,
                        "host": "127.0.0.1",
                        "port": 4242,
                        "name": "udp-peer",
                        "settings": { "forward_ip": "127.0.0.2" }
                    }]
                }),
            ))
            .expect("reload_config response");

        assert!(response.error.is_none(), "unexpected reload error: {response:?}");
        let result = response.result.expect("result");
        assert_eq!(result["hot_applied_legacy_tcp_only"], json!(false));
        assert_eq!(result["hot_applied_interface_mutation"], json!(true));
        let applied = bridge.applied();
        assert_eq!(applied.len(), 1);
        let iface = &applied[0][0];
        assert_eq!(iface.name.as_deref(), Some("udp-peer"));
        assert_eq!(iface.port, Some(4242));
        assert_eq!(
            iface.settings
                .as_ref()
                .and_then(|settings| settings.get("forward_ip"))
                .and_then(|value| value.as_str()),
            Some("127.0.0.2")
        );
    }

    #[test]
    fn reload_config_hot_applies_multicast_udp_forward_target() {
        let daemon = RpcDaemon::test_instance();
        daemon.replace_interfaces(vec![udp_interface("udp-peer", "127.0.0.1", 4242)]);
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                35,
                "reload_config",
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
            .expect("reload_config response");

        assert!(response.error.is_none(), "unexpected reload error: {response:?}");
        let result = response.result.expect("result");
        assert_eq!(result["hot_applied_legacy_tcp_only"], json!(false));
        assert_eq!(result["hot_applied_interface_mutation"], json!(true));
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
    fn reload_config_hot_applies_device_udp() {
        let daemon = RpcDaemon::test_instance();
        let original_interfaces = vec![udp_interface("udp-device", "127.0.0.1", 4242)];
        daemon.replace_interfaces(original_interfaces.clone());
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                37,
                "reload_config",
                json!({
                    "interfaces": [{
                        "type": "udp",
                        "enabled": true,
                        "host": "127.0.0.1",
                        "port": 4242,
                        "name": "udp-device",
                        "settings": { "device": "eth0" }
                    }]
                }),
            ))
            .expect("reload_config response");

        assert!(response.error.is_none(), "unexpected error: {response:?}");
        let result = response.result.expect("reload result");
        assert_eq!(result["hot_applied_interface_mutation"], json!(true));
        let mut expected = udp_interface("udp-device", "127.0.0.1", 4242);
        expected.settings = Some(json!({ "device": "eth0" }));
        assert_eq!(bridge.applied(), vec![vec![expected.clone()]]);
        assert_eq!(
            *daemon.interfaces.lock().expect("interfaces mutex poisoned"),
            vec![expected]
        );
    }

    #[test]
    fn reload_config_reports_out_of_range_udp_forward_port_requires_restart_without_partial_apply() {
        let daemon = RpcDaemon::test_instance();
        let original_interfaces = vec![udp_interface("udp-peer", "127.0.0.1", 4242)];
        daemon.replace_interfaces(original_interfaces.clone());
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                38,
                "reload_config",
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
            .expect("reload_config response");

        assert_reload_restart_required_without_apply(&daemon, &bridge, response, original_interfaces);
    }

    #[test]
    fn reload_config_reports_partial_udp_forward_target_requires_restart_without_partial_apply() {
        let daemon = RpcDaemon::test_instance();
        let original_interfaces = vec![udp_interface("udp-peer", "127.0.0.1", 4242)];
        daemon.replace_interfaces(original_interfaces.clone());
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                39,
                "reload_config",
                json!({
                    "interfaces": [{
                        "type": "udp",
                        "enabled": true,
                        "host": "127.0.0.1",
                        "port": 4242,
                        "name": "udp-peer",
                        "settings": { "target_host": "127.0.0.1" }
                    }]
                }),
            ))
            .expect("reload_config response");

        assert_reload_restart_required_without_apply(&daemon, &bridge, response, original_interfaces);
    }

    #[test]
    fn reload_config_hot_applies_prefer_ipv6_tcp_server() {
        let daemon = RpcDaemon::test_instance();
        daemon.replace_interfaces(vec![tcp_server_interface("listener", "127.0.0.1", 4242)]);
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                38,
                "reload_config",
                json!({
                    "interfaces": [{
                        "type": "tcp_server",
                        "enabled": true,
                        "host": "127.0.0.1",
                        "port": 4248,
                        "name": "listener",
                        "settings": {
                            "prefer_ipv6": true
                        }
                    }]
                }),
            ))
            .expect("reload_config response");

        assert!(response.error.is_none(), "unexpected error: {response:?}");
        let mut expected = tcp_server_interface("listener", "127.0.0.1", 4248);
        expected.settings = Some(json!({ "prefer_ipv6": true }));
        assert_eq!(bridge.applied(), vec![vec![expected.clone()]]);
        assert_eq!(
            *daemon.interfaces.lock().expect("interfaces mutex poisoned"),
            vec![expected]
        );
    }

    #[test]
    fn reload_config_hot_applies_loopback_tcp_server_only_diff() {
        let daemon = RpcDaemon::test_instance();
        daemon.replace_interfaces(vec![tcp_server_interface("listener", "127.0.0.1", 4242)]);
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                28,
                "reload_config",
                json!({
                    "interfaces": [
                        {
                            "type": "tcp_server",
                            "enabled": true,
                            "host": "127.0.0.1",
                            "port": 4248,
                            "name": "listener"
                        }
                    ]
                }),
            ))
            .expect("reload_config response");

        assert!(response.error.is_none(), "unexpected reload error: {response:?}");
        let result = response.result.expect("result");
        assert_eq!(result["reloaded"], json!(true));
        assert_eq!(result["hot_applied_legacy_tcp_only"], json!(false));
        assert_eq!(result["hot_applied_interface_mutation"], json!(true));
        assert_eq!(
            bridge.applied(),
            vec![vec![tcp_server_interface("listener", "127.0.0.1", 4248)]]
        );

        let interfaces = daemon.interfaces.lock().expect("interfaces mutex poisoned").clone();
        assert_eq!(interfaces[0].kind, "tcp_server");
        assert_eq!(interfaces[0].port, Some(4248));
    }

    #[test]
    fn reload_config_hot_applies_localhost_tcp_server_only_diff() {
        let daemon = RpcDaemon::test_instance();
        daemon.replace_interfaces(vec![tcp_server_interface("listener", "localhost", 4242)]);
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                132,
                "reload_config",
                json!({
                    "interfaces": [
                        {
                            "type": "tcp_server",
                            "enabled": true,
                            "host": "localhost",
                            "port": 4248,
                            "name": "listener"
                        }
                    ]
                }),
            ))
            .expect("reload_config response");

        assert!(response.error.is_none(), "unexpected reload error: {response:?}");
        let result = response.result.expect("result");
        assert_eq!(result["reloaded"], json!(true));
        assert_eq!(result["hot_applied_legacy_tcp_only"], json!(false));
        assert_eq!(result["hot_applied_interface_mutation"], json!(true));
        assert_eq!(
            bridge.applied(),
            vec![vec![tcp_server_interface("listener", "localhost", 4248)]]
        );

        let interfaces = daemon.interfaces.lock().expect("interfaces mutex poisoned").clone();
        assert_eq!(interfaces[0].kind, "tcp_server");
        assert_eq!(interfaces[0].host.as_deref(), Some("localhost"));
        assert_eq!(interfaces[0].port, Some(4248));
    }

    #[test]
    fn reload_config_hot_applies_wildcard_tcp_server_only_diff() {
        let daemon = RpcDaemon::test_instance();
        daemon.replace_interfaces(vec![tcp_server_interface("listener", "0.0.0.0", 4242)]);
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                138,
                "reload_config",
                json!({
                    "interfaces": [
                        {
                            "type": "tcp_server",
                            "enabled": true,
                            "host": "0.0.0.0",
                            "port": 4248,
                            "name": "listener"
                        }
                    ]
                }),
            ))
            .expect("reload_config response");

        assert!(response.error.is_none(), "unexpected reload error: {response:?}");
        let result = response.result.expect("result");
        assert_eq!(result["reloaded"], json!(true));
        assert_eq!(result["hot_applied_legacy_tcp_only"], json!(false));
        assert_eq!(result["hot_applied_interface_mutation"], json!(true));
        assert_eq!(
            bridge.applied(),
            vec![vec![tcp_server_interface("listener", "0.0.0.0", 4248)]]
        );

        let interfaces = daemon.interfaces.lock().expect("interfaces mutex poisoned").clone();
        assert_eq!(interfaces[0].kind, "tcp_server");
        assert_eq!(interfaces[0].host.as_deref(), Some("0.0.0.0"));
        assert_eq!(interfaces[0].port, Some(4248));
    }

    #[test]
    fn reload_config_hot_applies_hostname_tcp_server() {
        let daemon = RpcDaemon::test_instance();
        daemon.replace_interfaces(vec![tcp_server_interface("listener", "localhost", 4242)]);
        let bridge = std::sync::Arc::new(RecordingInterfaceMutationBridge::new());
        daemon.set_interface_mutation_bridge(bridge.clone());

        let response = daemon
            .handle_rpc(rpc_request(
                135,
                "reload_config",
                json!({
                    "interfaces": [
                        {
                            "type": "tcp_server",
                            "enabled": true,
                            "host": "example.invalid",
                            "port": 4248,
                            "name": "listener"
                        }
                    ]
                }),
            ))
            .expect("reload_config response");

        assert!(response.error.is_none(), "unexpected error: {response:?}");
        let expected = tcp_server_interface("listener", "example.invalid", 4248);
        assert_eq!(bridge.applied(), vec![vec![expected.clone()]]);
        assert_eq!(
            *daemon.interfaces.lock().expect("interfaces mutex poisoned"),
            vec![expected]
        );
    }

    #[test]
    fn reload_config_rejects_tcp_kind_swap_as_restart_required() {
        let daemon = RpcDaemon::test_instance();
        daemon.replace_interfaces(vec![tcp_interface("primary", "127.0.0.1", 4242)]);

        let response = daemon
            .handle_rpc(rpc_request(
                29,
                "reload_config",
                json!({
                    "interfaces": [
                        {
                            "type": "tcp_server",
                            "enabled": true,
                            "host": "127.0.0.1",
                            "port": 4242,
                            "name": "primary"
                        }
                    ]
                }),
            ))
            .expect("reload_config response");

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
