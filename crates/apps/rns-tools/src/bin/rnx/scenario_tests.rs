use super::poll_for_delivery_trace_status;
use rns_rpc::rpc::{codec, RpcRequest, RpcResponse};
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};

#[test]
fn direct_trace_accepts_confirmed_delivery_and_rejects_another_modes_handoff() {
    for (status, expected) in
        [("delivered", true), ("sent: link", true), ("sent: opportunistic", false)]
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test listener");
        let address = listener.local_addr().expect("listener address").to_string();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("trace request");
            let mut request = Vec::new();
            stream.read_to_end(&mut request).expect("request bytes");
            let offset =
                request.windows(4).position(|bytes| bytes == b"\r\n\r\n").expect("HTTP headers")
                    + 4;
            let request: RpcRequest = codec::decode_frame(&request[offset..]).expect("RPC request");
            assert_eq!(request.method, "message_delivery_trace");
            assert_eq!(request.params, Some(serde_json::json!({ "message_id": "trace-message" })));
            let response = codec::encode_frame(&RpcResponse {
                id: request.id,
                result: Some(serde_json::json!({
                    "message_id": "trace-message",
                    "transitions": [{ "status": "queued" }, { "status": status }]
                })),
                error: None,
            })
            .expect("RPC response");
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", response.len())
                .expect("response headers");
            stream.write_all(&response).expect("response body");
        });
        let observed =
            poll_for_delivery_trace_status(&address, "trace-message", "direct", Duration::ZERO, 1)
                .expect("trace observation");
        server.join().expect("trace server");
        assert_eq!(observed, expected, "trace status {status}");
    }
}
