impl RpcDaemon {
    pub(super) fn handle_rpc_legacy_propagation(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        match request.method.as_str() {
            "get_delivery_policy"
            | "set_delivery_policy"
            | "propagation_status"
            | "propagation_peer_maintenance"
            | "propagation_enable" => self.handle_rpc_legacy_propagation_policy(request),
            "propagation_ingest"
            | "propagation_fetch"
            | "get_outbound_propagation_cost"
            | "get_outbound_propagation_node"
            | "set_outbound_propagation_node"
            | "list_propagation_nodes"
            | "propagation_remote_status" => self.handle_rpc_legacy_propagation_nodes(request),
            "propagation_remote_sync" => self.handle_rpc_legacy_remote_sync(request),
            "propagation_remote_download"
            | "propagation_acknowledge_sync_completion"
            | "propagation_remote_fetch" => self.handle_rpc_legacy_remote_download_fetch(request),
            "propagation_remote_unpeer" => self.handle_rpc_legacy_remote_unpeer(request),
            _ => unreachable!("legacy propagation route: {}", request.method),
        }
    }
}
