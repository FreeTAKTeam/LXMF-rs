impl RpcDaemon {
    fn handle_rpc_legacy_transport_accessor(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        let error_prefix = match request.method.as_str() {
            "link_count" => "LINK_COUNT",
            "active_link_count" => "ACTIVE_LINK_COUNT",
            "lowest_interface_bitrate" => "LOWEST_INTERFACE_BITRATE",
            "medium_path_timeout" => "MEDIUM_PATH_TIMEOUT",
            _ => unreachable!("matched transport accessor"),
        };
        let Some(bridge) = self
            .path_lookup_bridge
            .lock()
            .expect("path_lookup_bridge mutex poisoned")
            .clone()
        else {
            return Ok(RpcResponse {
                id: request.id,
                result: None,
                error: Some(RpcError::new(
                    format!("{error_prefix}_UNAVAILABLE"),
                    "transport status bridge is not configured",
                )),
            });
        };
        let value = match request.method.as_str() {
            "link_count" => bridge.link_count().map(|value| json!(value)),
            "active_link_count" => bridge.active_link_count().map(|value| json!(value)),
            "lowest_interface_bitrate" => {
                bridge.lowest_interface_bitrate().map(|value| json!(value))
            }
            "medium_path_timeout" => bridge.medium_path_timeout().map(|value| json!(value)),
            _ => unreachable!("matched transport accessor"),
        };
        match value {
            Ok(value) => Ok(RpcResponse { id: request.id, result: Some(value), error: None }),
            Err(err) => Ok(RpcResponse {
                id: request.id,
                result: None,
                error: Some(RpcError::new(
                    format!("{error_prefix}_FAILED"),
                    err.to_string(),
                )),
            }),
        }
    }
}
