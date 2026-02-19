use reticulum::rpc::{http, RpcDaemon};
use std::net::SocketAddr;
use std::rc::Rc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub(super) async fn run_rpc_loop(addr: SocketAddr, daemon: Rc<RpcDaemon>) {
    let listener = TcpListener::bind(addr).await.unwrap();
    println!("reticulumd listening on http://{}", addr);

    loop {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buffer = Vec::new();
        loop {
            let mut chunk = [0u8; 4096];
            let read = stream.read(&mut chunk).await.unwrap();
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(header_end) = http::find_header_end(&buffer) {
                let headers = &buffer[..header_end];
                if let Some(length) = http::parse_content_length(headers) {
                    let body_start = header_end + 4;
                    if buffer.len() >= body_start + length {
                        break;
                    }
                } else {
                    break;
                }
            }
        }

        if buffer.is_empty() {
            continue;
        }

        let response = http::handle_http_request(&daemon, &buffer)
            .unwrap_or_else(|err| http::build_error_response(&format!("rpc error: {}", err)));
        let _ = stream.write_all(&response).await;
        let _ = stream.shutdown().await;
    }
}
