impl RpcDaemon {
    fn handle_rpc_legacy(&self, request: RpcRequest) -> Result<RpcResponse, std::io::Error> {
        match request.method.as_str() {
            "list_messages" | "sdk_poll_events_v2" | "list_announces" | "list_peers" | "list_interfaces" | "set_interfaces" | "reload_config" | "peer_sync" | "peer_unpeer" | "send_message" | "send_message_v2" | "sdk_send_v2" | "receive_message" | "record_receipt" | "sdk_cancel_message_v2" | "message_delivery_trace" => self.handle_rpc_legacy_messages(request),
            "get_delivery_policy" | "set_delivery_policy" | "propagation_status" | "propagation_enable" | "propagation_ingest" | "propagation_fetch" | "get_outbound_propagation_node" | "set_outbound_propagation_node" | "list_propagation_nodes" => self.handle_rpc_legacy_propagation(request),
            "paper_ingest_uri" | "stamp_policy_get" | "stamp_policy_set" | "ticket_generate" | "announce_now" | "announce_received" => self.handle_rpc_legacy_misc(request),
            "clear_messages" | "clear_resources" | "clear_peers" | "clear_all" => self.handle_rpc_legacy_clear(request),
            _ => Ok(RpcResponse {
                id: request.id,
                result: None,
                error: Some(RpcError::new("NOT_IMPLEMENTED", "method not implemented")),
            }),
        }
    }

}
