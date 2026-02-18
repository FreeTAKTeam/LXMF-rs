use reticulum::rpc::codec::{decode_frame, encode_frame};
use reticulum::rpc::RpcRequest;

#[test]
fn round_trips_framed_messagepack() {
    let msg = RpcRequest { id: 1, method: "status".into(), params: None };
    let bytes = encode_frame(&msg).unwrap();
    let decoded: RpcRequest = decode_frame(&bytes).unwrap();
    assert_eq!(decoded.method, "status");
}
