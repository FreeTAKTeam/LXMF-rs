use reticulum::delivery::LinkSendResult;
use reticulum::destination::DestinationDesc;
use reticulum::transport::Transport;
use std::io;
use std::time::Duration;

pub async fn send_via_link(
    transport: &Transport,
    destination: DestinationDesc,
    payload: &[u8],
    wait_timeout: Duration,
) -> io::Result<LinkSendResult> {
    reticulum::delivery::send_via_link(transport, destination, payload, wait_timeout).await
}
