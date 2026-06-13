use super::*;

#[derive(Clone, Copy)]
struct SdkOperationSpec {
    id: &'static str,
    group: &'static str,
    kind: &'static str,
    transport_variant: &'static str,
    description: &'static str,
    aliases: &'static [&'static str],
    required_capabilities: &'static [&'static str],
    rpc_method: &'static str,
}

#[derive(Debug, Clone)]
struct ResolvedSdkOperationSpec {
    id: String,
    kind: String,
    rpc_method: &'static str,
}
