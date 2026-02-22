#[cfg(test)]
mod tests {
    use super::*;

    fn rpc_request(id: u64, method: &str, params: JsonValue) -> RpcRequest {
        RpcRequest { id, method: method.to_string(), params: Some(params) }
    }

    include!("tests/negotiate_security.rs");
    include!("tests/events_basic.rs");
    include!("tests/release_domains.rs");
    include!("tests/runtime_state.rs");
    include!("tests/store_forward_policy.rs");
    include!("tests/event_sink_bridges.rs");
    include!("tests/interface_mutation_policy.rs");
}
