use reticulum::rpc::{
    codec::decode_frame, codec::encode_frame, RpcDaemon, RpcRequest, RpcResponse,
};

#[test]
fn handles_framed_rpc_request() {
    let daemon = RpcDaemon::test_instance();
    let req = RpcRequest { id: 7, method: "status".into(), params: None };
    let framed = encode_frame(&req).unwrap();
    let response_bytes = reticulum::rpc::handle_framed_request(&daemon, &framed).unwrap();
    let resp: RpcResponse = decode_frame(&response_bytes).unwrap();
    assert_eq!(resp.id, 7);
    assert!(resp.result.is_some());
}
