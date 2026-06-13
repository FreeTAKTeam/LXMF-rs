impl RpcDaemon {
    pub(super) fn handle_rpc_legacy_messages(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        match request.method.as_str() {
            "list_messages"
            | "sdk_poll_events_v2"
            | "list_announces"
            | "list_peers"
            | "list_interfaces"
            | "set_interfaces"
            | "reload_config" => self.handle_rpc_legacy_message_catalog(request),
            "peer_sync" => self.handle_rpc_legacy_peer_sync(request),
            "peer_unpeer"
            | "sdk_send_batch_v2"
            | "send_message"
            | "send_message_v2"
            | "sdk_send_v2"
            | "receive_message"
            | "record_receipt"
            | "sdk_cancel_message_v2"
            | "message_delivery_trace"
            | "get_outbound_progress"
            | "get_outbound_lxm_stamp_cost"
            | "get_outbound_lxm_propagation_stamp_cost" => {
                self.handle_rpc_legacy_message_delivery(request)
            }
            _ => unreachable!("legacy message route: {}", request.method),
        }
    }
}
