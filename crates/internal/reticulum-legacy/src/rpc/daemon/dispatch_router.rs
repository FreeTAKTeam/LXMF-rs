impl RpcDaemon {
    pub fn handle_rpc(&self, request: RpcRequest) -> Result<RpcResponse, std::io::Error> {
        match request.method.as_str() {
            "status" => Ok(RpcResponse {
                id: request.id,
                result: Some(json!({
                    "identity_hash": self.identity_hash,
                    "delivery_destination_hash": self.local_delivery_hash(),
                    "running": true
                })),
                error: None,
            }),
            "daemon_status_ex" => {
                let peer_count = self.peers.lock().expect("peers mutex poisoned").len();
                let interfaces = self.interfaces.lock().expect("interfaces mutex poisoned").clone();
                let message_count =
                    self.store.list_messages(10_000, None).map_err(std::io::Error::other)?.len();
                let delivery_policy =
                    self.delivery_policy.lock().expect("policy mutex poisoned").clone();
                let propagation =
                    self.propagation_state.lock().expect("propagation mutex poisoned").clone();
                let stamp_policy = self.stamp_policy.lock().expect("stamp mutex poisoned").clone();

                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "identity_hash": self.identity_hash,
                        "delivery_destination_hash": self.local_delivery_hash(),
                        "running": true,
                        "peer_count": peer_count,
                        "message_count": message_count,
                        "interface_count": interfaces.len(),
                        "interfaces": interfaces,
                        "delivery_policy": delivery_policy,
                        "propagation": propagation,
                        "stamp_policy": stamp_policy,
                        "capabilities": Self::capabilities(),
                    })),
                    error: None,
                })
            }
            "list_messages" | "list_announces" | "list_peers" | "list_interfaces" | "set_interfaces" | "reload_config" | "peer_sync" | "peer_unpeer" | "send_message" | "send_message_v2" | "receive_message" | "record_receipt" | "message_delivery_trace" => self.handle_rpc_messages(request),
            "get_delivery_policy" | "set_delivery_policy" | "propagation_status" | "propagation_enable" | "propagation_ingest" | "propagation_fetch" | "get_outbound_propagation_node" | "set_outbound_propagation_node" | "list_propagation_nodes" => self.handle_rpc_propagation(request),
            "paper_ingest_uri" | "stamp_policy_get" | "stamp_policy_set" | "ticket_generate" | "announce_now" | "announce_received" => self.handle_rpc_misc(request),
            "clear_messages" | "clear_resources" | "clear_peers" | "clear_all" => self.handle_rpc_clear(request),
            _ => Ok(RpcResponse {
                id: request.id,
                result: None,
                error: Some(RpcError {
                    code: "NOT_IMPLEMENTED".into(),
                    message: "method not implemented".into(),
                }),
            }),
        }
    }
}
