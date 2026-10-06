impl RpcDaemon {
    fn handle_rpc_legacy_router_management(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        match request.method.as_str() {
            "router_stats" => self.handle_router_stats(request.id),
            "router_storage_policy_get" => Ok(RpcResponse {
                id: request.id,
                result: Some(self.router_storage_policy()),
                error: None,
            }),
            "router_storage_policy_set" => self.handle_router_storage_policy_set(request),
            _ => unreachable!("router management route: {}", request.method),
        }
    }

    fn handle_router_stats(&self, id: u64) -> Result<RpcResponse, std::io::Error> {
        let storage = self.store.message_storage_stats().map_err(std::io::Error::other)?;
        let propagation_storage =
            self.store.propagation_entry_stats().map_err(std::io::Error::other)?;
        let (propagation_enabled, propagation_node_enabled) = {
            let propagation = self.propagation_state.lock().expect("propagation mutex poisoned");
            (propagation.enabled, propagation.propagation_node_enabled)
        };
        let result = json!({
            "messages": storage.count,
            "message_bytes": storage.bytes,
            "peers": self.peers.lock().expect("peers mutex poisoned").len(),
            "interfaces": self.interfaces.lock().expect("interfaces mutex poisoned").len(),
            "tickets": self.ticket_cache.lock().expect("ticket cache mutex poisoned").len(),
            "propagation_payloads": propagation_storage.entries,
            "outbound_inflight": self.outbound_delivery_handoffs.lock().expect("outbound handoffs mutex poisoned").len(),
            "propagation_enabled": propagation_enabled,
            "propagation_node_enabled": propagation_node_enabled,
            "storage_policy": self.router_storage_policy(),
        });
        Ok(RpcResponse { id, result: Some(result), error: None })
    }

    fn handle_router_storage_policy_set(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let policy: RouterStoragePolicyParams = serde_json::from_value(params)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
        if let Some(limit) = policy.message_limit_bytes {
            self.propagation_state
                .lock()
                .expect("propagation mutex poisoned")
                .message_storage_limit_mb = Some(limit.saturating_add(999_999) / 1_000_000);
        }
        if let Some(limit) = policy.information_limit_bytes {
            *self
                .router_information_storage_limit_bytes
                .lock()
                .expect("information storage policy mutex poisoned") = Some(limit);
        }
        if let Some(retain) = policy.retain_node_lxms {
            *self.router_retain_node_lxms.lock().expect("retain node lxms mutex poisoned") = retain;
        }
        Ok(RpcResponse { id: request.id, result: Some(self.router_storage_policy()), error: None })
    }

    fn router_storage_policy(&self) -> JsonValue {
        let message_limit_bytes = self
            .propagation_state
            .lock()
            .expect("propagation mutex poisoned")
            .message_storage_limit_mb
            .map(|limit| limit.saturating_mul(1_000_000));
        json!({
            "message_limit_bytes": message_limit_bytes,
            "information_limit_bytes": *self.router_information_storage_limit_bytes.lock().expect("information storage policy mutex poisoned"),
            "retain_node_lxms": *self.router_retain_node_lxms.lock().expect("retain node lxms mutex poisoned"),
        })
    }
}
