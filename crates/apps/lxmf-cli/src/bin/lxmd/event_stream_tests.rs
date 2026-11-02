use super::*;
use crate::rpc_client::encode_rpc_frame;
use serde_json::json;
use std::net::TcpListener;

/// Serves one `/events/stream` response and returns the request line it saw.
fn serve_once(response: Vec<u8>) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake daemon");
    let addr = listener.local_addr().expect("fake daemon address").to_string();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept watcher");
        let mut reader = BufReader::new(stream);
        let mut request_line = String::new();
        reader.read_line(&mut request_line).expect("request line");
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).expect("request header");
            if line == "\r\n" {
                break;
            }
        }
        reader.get_mut().write_all(&response).expect("write response");
        request_line
    });
    (addr, server)
}

fn event(seq_no: u64, event_type: &str) -> serde_json::Value {
    json!({
        "runtime_id": "rt",
        "stream_id": "sdk-events",
        "seq_no": seq_no,
        "event_type": event_type,
        "payload": {},
    })
}

#[test]
fn stream_hands_over_each_event_and_resumes_after_the_last() {
    let mut response = b"HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\n\r\n".to_vec();
    for value in [event(6, "announce_received"), event(7, "inbound")] {
        response.extend(encode_rpc_frame(value).expect("event frame"));
    }
    let (addr, server) = serve_once(response);
    let mut seen = Vec::new();

    stream_events(&addr, Some("v2:rt:sdk-events:5"), |event| seen.push(event.clone()))
        .expect("stream ends cleanly when the daemon closes it");

    assert_eq!(
        server.join().expect("server"),
        "GET /events/stream?cursor=v2:rt:sdk-events:5 HTTP/1.1\r\n"
    );
    assert_eq!(seen, vec![event(6, "announce_received"), event(7, "inbound")]);
    assert_eq!(cursor_after(&seen[1]).as_deref(), Some("v2:rt:sdk-events:7"));
}

#[test]
fn expired_cursor_rejection_resets_instead_of_stopping() {
    let mut response = b"HTTP/1.1 400 Bad Request\r\n\r\n".to_vec();
    response.extend(
        encode_rpc_frame(json!([0, null, ["SDK_RUNTIME_CURSOR_EXPIRED", "outside window"]]))
            .expect("error frame"),
    );
    let (addr, server) = serve_once(response);

    let result = stream_events(&addr, Some("v2:rt:sdk-events:1"), |_| panic!("no events"));

    server.join().expect("server");
    assert!(
        matches!(result, Err(StreamError::Cursor(ref text)) if text.contains("CURSOR_EXPIRED"))
    );
}

#[test]
fn error_frame_mid_stream_is_reported() {
    let mut response = b"HTTP/1.1 200 OK\r\n\r\n".to_vec();
    response.extend(encode_rpc_frame(event(3, "inbound")).expect("event frame"));
    response.extend(
        encode_rpc_frame(json!([0, null, ["SDK_RUNTIME_INTERNAL", "catch-up failed"]]))
            .expect("error frame"),
    );
    let (addr, server) = serve_once(response);
    let mut seen = 0;

    let result = stream_events(&addr, None, |_| seen += 1);

    assert_eq!(server.join().expect("server"), "GET /events/stream HTTP/1.1\r\n");
    assert_eq!(seen, 1);
    assert!(
        matches!(result, Err(StreamError::Fatal(ref text)) if text.contains("catch-up failed"))
    );
}
