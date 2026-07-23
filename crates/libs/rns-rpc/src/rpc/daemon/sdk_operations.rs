include!("sdk_operations_parts/module_prelude.rs");

include!("sdk_operations_parts/sdk_operation_specs.rs");

include!("sdk_operations_parts/identity_operation_specs.rs");

include!("sdk_operations_parts/ticket_operation_specs.rs");

include!("sdk_operations_parts/conversation_operation_specs.rs");

include!("sdk_operations_parts/delivery_operation_specs.rs");

include!("sdk_operations_parts/propagation_operation_specs.rs");

include!("sdk_operations_parts/legacy_operation_specs.rs");

include!("sdk_operations_parts/rns_operation_specs.rs");

include!("sdk_operations_parts/rpcdaemon.rs");

#[cfg(test)]
mod rns_control_tests {
    use super::*;

    #[test]
    fn rns_controls_are_registered_and_delegate_through_sdk_envelopes() {
        let daemon = RpcDaemon::test_instance();
        let registry = daemon.operation_registry_json();
        let entries = registry["entries"].as_array().expect("operation registry array");
        for operation in [
            "rns.runtime.status",
            "rns.transport.path.status",
            "rns.interfaces.discovered",
            "rns.data_plane.links.count",
            "app.router.stats",
        ] {
            assert!(entries.iter().any(|entry| entry["id"] == operation), "{operation}");
        }

        let response = daemon
            .handle_sdk_envelope_execute_v2(RpcRequest {
                id: 41,
                method: "sdk_envelope_execute_v2".to_owned(),
                params: Some(json!({
                    "operation_id": "rns.runtime.status",
                    "kind": "query",
                    "payload": {},
                    "extensions": {}
                })),
            })
            .expect("RNS status envelope");
        assert!(response.error.is_none(), "{:?}", response.error);
        assert_eq!(
            response
                .result
                .as_ref()
                .and_then(|result| result.pointer("/response/operation_id"))
                .and_then(JsonValue::as_str),
            Some("rns.runtime.status")
        );
    }
}
