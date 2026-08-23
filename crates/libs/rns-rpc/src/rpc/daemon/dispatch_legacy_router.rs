use super::*;

impl RpcDaemon {
    pub(super) fn handle_rpc_legacy(
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
            | "reload_config"
            | "peer_sync"
            | "peer_unpeer"
            | "send_message"
            | "send_message_v2"
            | "sdk_send_v2"
            | "sdk_send_batch_v2"
            | "receive_message"
            | "record_receipt"
            | "sdk_cancel_message_v2"
            | "message_delivery_trace"
            | "get_outbound_progress"
            | "get_outbound_lxm_stamp_cost"
            | "get_outbound_lxm_propagation_stamp_cost" => self.handle_rpc_legacy_messages(request),
            "get_delivery_policy"
            | "set_delivery_policy"
            | "set_authentication"
            | "requires_authentication"
            | "allow"
            | "disallow"
            | "ignore_destination"
            | "unignore_destination"
            | "prioritise"
            | "unprioritise"
            | "allow_destination"
            | "disallow_destination"
            | "prioritise_destination"
            | "allow_control"
            | "disallow_control"
            | "propagation_status"
            | "propagation_enable"
            | "propagation_ingest"
            | "propagation_fetch"
            | "get_outbound_propagation_cost"
            | "get_outbound_propagation_node"
            | "set_outbound_propagation_node"
            | "list_propagation_nodes"
            | "propagation_peer_maintenance"
            | "propagation_remote_status"
            | "propagation_remote_sync"
            | "propagation_remote_fetch"
            | "propagation_remote_download"
            | "propagation_acknowledge_sync_completion"
            | "propagation_remote_unpeer" => self.handle_rpc_legacy_propagation(request),
            "paper_ingest_uri"
            | "stamp_policy_get"
            | "stamp_policy_set"
            | "ticket_generate"
            | "get_outbound_stamp_cost"
            | "path_status"
            | "next_hop"
            | "next_hop_if_name"
            | "first_hop_timeout"
            | "request_path"
            | "drop_path"
            | "drop_all_via"
            | "drop_announce_queues"
            | "get_rate_table"
            | "get_packet_rssi"
            | "get_packet_snr"
            | "get_packet_q"
            | "discovered_interfaces"
            | "router_stats"
            | "router_storage_policy_get"
            | "router_storage_policy_set"
            | "link_count"
            | "active_link_count"
            | "lowest_interface_bitrate"
            | "medium_path_timeout"
            | "announce_now"
            | "announce_delivery"
            | "announce_received"
            | "get_blackholed_identities"
            | "blackhole_identity"
            | "unblackhole_identity" => self.handle_rpc_legacy_misc(request),
            "rnode_management" | "weave_remote_display_control" => {
                self.handle_rpc_legacy_misc(request)
            }
            "clear_messages" | "clear_resources" | "clear_peers" | "clear_all" => {
                self.handle_rpc_legacy_clear(request)
            }
            _ => Ok(RpcResponse {
                id: request.id,
                result: None,
                error: Some(RpcError::new("NOT_IMPLEMENTED", "method not implemented")),
            }),
        }
    }
}
