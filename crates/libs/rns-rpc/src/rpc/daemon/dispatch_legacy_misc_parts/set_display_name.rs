impl RpcDaemon {
    pub(super) fn handle_set_display_name(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        let display_name = request
            .params
            .as_ref()
            .and_then(|params| params.get("display_name"))
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "display_name is required")
            })?
            .to_owned();
        // without an announce bridge nothing is announced, so there is no name to replace
        let Some(bridge) = &self.announce_bridge else {
            return Ok(RpcResponse {
                id: request.id,
                result: Some(json!({ "display_name": display_name, "applied": false })),
                error: None,
            });
        };
        if let Err(err) = bridge.set_display_name(display_name.as_str()) {
            return Ok(RpcResponse {
                id: request.id,
                result: None,
                error: Some(RpcError::new("DISPLAY_NAME_UPDATE_FAILED", err.to_string())),
            });
        }
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "display_name": display_name, "applied": true })),
            error: None,
        })
    }
}
