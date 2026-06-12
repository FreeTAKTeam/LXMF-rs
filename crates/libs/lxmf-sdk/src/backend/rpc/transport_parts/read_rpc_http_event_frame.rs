#[cfg(feature = "sdk-async")]
async fn read_rpc_http_event_frame<S>(stream: &mut S) -> Result<SdkEvent, SdkError>
where
    S: AsyncRead + Unpin + ?Sized,
{
    let mut frame_len = [0_u8; 4];
    stream
        .read_exact(&mut frame_len)
        .await
        .map_err(|err| SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string()))?;
    let len = u32::from_be_bytes(frame_len) as usize;
    if len > RPC_EVENT_STREAM_MAX_FRAME_BYTES {
        return Err(SdkError::new(
            code::INTERNAL,
            ErrorCategory::Transport,
            format!("event stream frame exceeded {} bytes", RPC_EVENT_STREAM_MAX_FRAME_BYTES),
        ));
    }
    let mut frame = Vec::with_capacity(4 + len);
    frame.extend_from_slice(&frame_len);
    frame.resize(4 + len, 0);
    stream
        .read_exact(&mut frame[4..])
        .await
        .map_err(|err| SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string()))?;
    match codec::decode_frame::<SdkEvent>(&frame) {
        Ok(event) => Ok(event),
        Err(event_err) => match codec::decode_frame::<RpcResponse>(&frame) {
            Ok(response) if response.error.is_some() => {
                Err(RpcBackendClient::map_rpc_error(response.error.expect("checked error")))
            }
            _ => {
                Err(SdkError::new(code::INTERNAL, ErrorCategory::Transport, event_err.to_string()))
            }
        },
    }
}
