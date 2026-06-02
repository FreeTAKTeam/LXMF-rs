use lxmf_sdk::app::{Client, Config, EventKind, SendRequest, SubscriptionStart};
use serde_json::json;
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> Result<(), lxmf_sdk::app::Error> {
    let endpoint =
        std::env::var("LXMF_RPC").unwrap_or_else(|_| "unix:/tmp/lxmf-rpc.sock".to_owned());
    let source = std::env::var("LXMF_SOURCE").unwrap_or_else(|_| "example.desktop".to_owned());
    let destination =
        std::env::var("LXMF_DESTINATION").unwrap_or_else(|_| "example.peer".to_owned());

    let client = Client::rpc(endpoint);
    let handle = client.runtime().start_async(Config::desktop_default()).await?;
    println!("started runtime_id={}", handle.runtime_id);

    let send_request = SendRequest::new(
        source,
        destination,
        json!({
            "title": "SDK Example",
            "content": "hello from lxmf-sdk example"
        }),
    )
    .with_ttl_ms(30_000)
    .with_correlation_id("example-rpc-desktop-send")
    .with_delivery_method("direct")
    .with_stamp_cost(8)
    .with_include_ticket(true)
    .with_try_propagation_on_fail(true);
    let receipt = client.messages().send_async(send_request).await?;
    println!("queued message_id={}", receipt.message_id);

    let mut events = client.events().subscribe(SubscriptionStart::Tail)?;
    while let Some(event) = events.next().await.transpose()? {
        match event.kind {
            EventKind::MessageDelivered
            | EventKind::MessageFailed
            | EventKind::MessageCancelled
                if event.metadata.message_id.as_deref() == Some(receipt.message_id.as_str()) =>
            {
                println!("terminal delivery event={:?}", event.kind);
                break;
            }
            EventKind::StreamGapDetected(gap) => {
                eprintln!("event stream gap detected: {:?}", gap);
                break;
            }
            _ => {}
        }
    }

    Ok(())
}
