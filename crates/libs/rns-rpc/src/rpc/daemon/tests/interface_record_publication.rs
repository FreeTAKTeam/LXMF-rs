#[test]
fn queued_interface_rpc_cannot_overwrite_records_published_by_the_native_owner() {
    struct NativePublishingBridge {
        daemon: std::sync::Weak<RpcDaemon>,
        committed: Vec<InterfaceRecord>,
    }
    impl InterfaceMutationBridge for NativePublishingBridge {
        fn publishes_interface_records(&self) -> bool {
            true
        }

        fn apply_interfaces(
            &self,
            interfaces: Vec<InterfaceRecord>,
        ) -> std::io::Result<Vec<InterfaceRecord>> {
            // Cover the fast-worker ordering: native commit/rejection happens
            // before the synchronous caller finishes accepting the command.
            self.daemon.upgrade().expect("daemon").replace_interfaces(self.committed.clone());
            Ok(interfaces)
        }
    }
    for method in ["set_interfaces", "reload_config"] {
        let daemon = Arc::new(RpcDaemon::test_instance());
        let committed = vec![tcp_interface("active", "127.0.0.1", 4242)];
        daemon.replace_interfaces(committed.clone());
        let bridge = Arc::new(NativePublishingBridge {
            daemon: Arc::downgrade(&daemon),
            committed: committed.clone(),
        });
        daemon.set_interface_mutation_bridge(bridge);
        let params = json!({"interfaces": [tcp_interface("queued", "127.0.0.1", 4243)]});
        let response = daemon.handle_rpc(rpc_request(1, method, params)).expect("interface RPC");
        assert!(response.error.is_none(), "{method}: {response:?}");
        assert_eq!(daemon.interface_records(), committed, "{method} must preserve owner truth");
        let result = response.result.expect("acknowledgement");
        assert_eq!(result["queued"], true);
        assert_eq!(result["complete"], false);
        let events = daemon
            .handle_rpc(rpc_request(2, "sdk_poll_events_v2", json!({"cursor": null, "max": 16})))
            .expect("event replay")
            .result
            .expect("event batch");
        assert!(events["events"].as_array().expect("events").iter().all(|event| {
            !matches!(event["event_type"].as_str(), Some("interfaces_updated" | "config_reloaded"))
        }), "queue admission must not emit completion: {events}");
    }
}
