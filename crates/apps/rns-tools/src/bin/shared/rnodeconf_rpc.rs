// The RPC/HTTP leg, split out so `rnodeconf.rs` stays inside the module-size
// policy. Nothing here knows about RNode commands.

fn rpc_call(
    rpc: &str,
    id: u64,
    method: &str,
    params: Option<serde_json::Value>,
) -> io::Result<rns_rpc::RpcResponse> {
    let frame = build_rpc_frame(id, method, params)?;
    let request = build_http_post("/rpc", rpc, &frame);
    let mut stream = TcpStream::connect(rpc)?;
    stream.write_all(&request)?;
    stream.shutdown(Shutdown::Write)?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    let body = parse_http_response_body(&response)?;
    rns_rpc::rpc::codec::decode_frame(&body)
}

fn ensure_rpc_ok(
    response: rns_rpc::RpcResponse,
    context: &str,
) -> io::Result<Option<serde_json::Value>> {
    if let Some(error) = response.error {
        return Err(io::Error::other(format!(
            "{} failed: {} ({})",
            context, error.message, error.code
        )));
    }
    Ok(response.result)
}
