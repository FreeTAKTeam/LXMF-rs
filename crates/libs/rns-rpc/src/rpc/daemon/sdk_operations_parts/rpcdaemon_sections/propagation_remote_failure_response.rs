impl RpcDaemon {
    fn propagation_remote_failure_response(
        &self,
        request_id: u64,
        method: &str,
        params: &JsonValue,
        err: &std::io::Error,
    ) -> RpcResponse {
        let remote = params
            .get("remote")
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .unwrap_or_default();
        let peer = params
            .get("peer")
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let propagation = self.propagation_state.lock().expect("propagation mutex poisoned").clone();
        let failure_kind = match err.kind() {
            std::io::ErrorKind::PermissionDenied => "no_access",
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => "timeout",
            _ => "error",
        };
        let result = json!({
            "synced": false,
            "postponed": true,
            "postpone_reason": failure_kind,
            "failure_kind": failure_kind,
            "error": err.to_string(),
        });
        let payload = match method {
            "propagation_remote_sync" => {
                let peer_sync = peer
                    .and_then(|peer| {
                        self.event_queue
                            .lock()
                            .expect("event_queue mutex poisoned")
                            .iter()
                            .rev()
                            .find(|event| {
                                event.event_type == "peer_sync"
                                    && event.payload["peer"].as_str() == Some(peer)
                            })
                            .map(|event| event.payload.clone())
                    })
                    .unwrap_or(JsonValue::Null);
                json!({
                    "remote": remote,
                    "peer": peer,
                    "propagation": propagation,
                    "peer_sync": peer_sync,
                    "result": result,
                })
            }
            "propagation_remote_unpeer" => json!({
                "remote": remote,
                "peer": peer,
                "removed": false,
                "propagation": propagation,
                "messages": JsonValue::Null,
                "result": result,
            }),
            _ => json!({
                "remote": remote,
                "propagation": propagation,
                "result": result,
            }),
        };
        RpcResponse {
            id: request_id,
            result: Some(payload),
            error: None,
        }
    }
}
