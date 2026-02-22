#![no_main]

use libfuzzer_sys::fuzz_target;
use rns_rpc::rpc::{codec, RpcRequest, RpcResponse};

fuzz_target!(|data: &[u8]| {
    let _ = codec::decode_frame::<RpcRequest>(data);
    let _ = codec::decode_frame::<RpcResponse>(data);
    let _ = rns_rpc::e2e_harness::parse_http_response_body(data);
    if let Ok(text) = core::str::from_utf8(data) {
        let _ = rns_rpc::e2e_harness::parse_rpc_response(text);
    }
});
