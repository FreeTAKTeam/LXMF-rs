impl RpcDaemon {
    fn handle_rpc_named_interface_management(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.as_ref();
        let operation = params
            .and_then(|params| params.get("operation"))
            .and_then(JsonValue::as_str);
        let name = params
            .and_then(|params| params.get("name"))
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty());
        let Some((operation, name)) = operation.zip(name).filter(|(operation, _)| {
            matches!(*operation, "attach" | "detach" | "reload")
        }) else {
            let error = std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "operation must be attach, detach, or reload and name must be nonempty",
            );
            return Ok(Self::named_interface_error_response(request.id, error));
        };

        let bridge = self
            .interface_mutation_bridge
            .lock()
            .expect("interface mutation bridge mutex poisoned")
            .clone();
        let Some(bridge) = bridge else {
            return Ok(Self::named_interface_error_response(
                request.id,
                std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "named interface management is not configured",
                ),
            ));
        };
        match bridge.manage_named_interface(operation, name) {
            Ok(result) if result.get("complete").and_then(JsonValue::as_bool) == Some(true) => {
                Ok(RpcResponse { id: request.id, result: Some(result), error: None })
            }
            Ok(_) => Ok(Self::named_interface_error_response(
                request.id,
                std::io::Error::other("management bridge did not report completion"),
            )),
            Err(error) => Ok(Self::named_interface_error_response(request.id, error)),
        }
    }

    fn named_interface_error_response(id: u64, error: std::io::Error) -> RpcResponse {
        let (code, machine_code) = match error.kind() {
            std::io::ErrorKind::InvalidInput => ("CONFIG_INVALID_INTERFACE", "INVALID_INTERFACE_NAME"),
            std::io::ErrorKind::NotFound => ("CONFIG_INTERFACE_NOT_FOUND", "INTERFACE_NOT_FOUND"),
            std::io::ErrorKind::AlreadyExists => ("CONFIG_INTERFACE_ALREADY_ACTIVE", "INTERFACE_ALREADY_ACTIVE"),
            std::io::ErrorKind::PermissionDenied => ("CONFIG_INTERFACE_MANAGEMENT_DENIED", "INTERFACE_MANAGEMENT_DENIED"),
            std::io::ErrorKind::Unsupported => ("CONFIG_INTERFACE_MANAGEMENT_UNAVAILABLE", "INTERFACE_MANAGEMENT_UNAVAILABLE"),
            std::io::ErrorKind::TimedOut => ("CONFIG_INTERFACE_MANAGEMENT_TIMEOUT", "INTERFACE_MANAGEMENT_TIMEOUT"),
            _ => ("CONFIG_INTERFACE_APPLY_FAILED", "INTERFACE_MUTATION_FAILED"),
        };
        let mut rpc_error = RpcError::new(code, error.to_string());
        rpc_error.machine_code = Some(machine_code.to_string());
        rpc_error.category = Some("Config".to_string());
        rpc_error.retryable = Some(matches!(error.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock));
        RpcResponse { id, result: None, error: Some(rpc_error) }
    }
}
