#[cfg(test)]
mod tests {
    use super::*;

    fn rpc_request(id: u64, method: &str, params: JsonValue) -> RpcRequest {
        RpcRequest { id, method: method.to_string(), params: Some(params) }
    }

    #[test]
    fn rpc_return_matches_the_legacy_messagepack_frame_boundary() {
        let response = json!({"ok": true, "value": 7});
        let frame = RpcDaemon::rpc_return(&response).expect("encode legacy RPC response");
        let decoded: JsonValue = crate::rpc::codec::decode_frame(&frame).expect("decode response");
        assert_eq!(decoded, response);
    }

    include!("tests/negotiate_security.rs");
    include!("tests/events_basic.rs");
    include!("tests/announce_scheduler.rs");
    include!("tests/release_domains.rs");
    include!("tests/runtime_state.rs");
    include!("tests/store_forward_policy.rs");
    include!("tests/event_sink_bridges.rs");
    include!("tests/interface_mutation_policy.rs");
    include!("tests/interface_mutation_policy_reload.rs");
    include!("tests/path_rpc.rs");
    include!("tests/blackhole_rpc.rs");
    include!("tests/router_management.rs");
    include!("tests/rnode_management.rs");
    include!("tests/weave_display_control.rs");
    include!("tests/status_snapshot.rs");
}
