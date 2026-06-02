use lxmf_sdk::app::{Client, Config, EventKind, SendRequest, SubscriptionStart};
use serde_json::json;
use tokio_stream::StreamExt;

// Contract anchors:
// - lifecycle.start_stop_restart: start once, stop explicitly, restart with Config when needed.
// - events.delivery_ordering: subscribe before send so queued, sent, and delivered events are ordered.
// - delivery.queue_pressure: use send_async errors or send_with_options for queue-pressure policy.
#[tokio::main]
async fn main() -> Result<(), lxmf_sdk::app::Error> {
    let endpoint =
        std::env::var("LXMF_RPC_ENDPOINT").unwrap_or_else(|_| "unix:/tmp/lxmf-rpc.sock".to_owned());
    let source = std::env::var("LXMF_SOURCE").unwrap_or_else(|_| "example.app".to_owned());
    let destination =
        std::env::var("LXMF_DESTINATION").unwrap_or_else(|_| "example.peer".to_owned());

    let client = Client::rpc(endpoint);
    let handle = client.runtime().start_async(Config::desktop_default()).await?;
    println!("runtime_id={}", handle.runtime_id);

    let mut events = client.events().subscribe(SubscriptionStart::Tail)?;
    let receipt = client
        .messages()
        .send_async(
            SendRequest::new(
                source,
                destination,
                json!({
                    "title": "hello",
                    "content": "sent from lxmf-sdk easy mode"
                }),
            )
            .with_correlation_id("easy-rust-managed-send")
            .with_ttl_ms(30_000),
        )
        .await?;
    println!("queued message_id={}", receipt.message_id);

    while let Some(event) = events.next().await.transpose()? {
        match event.kind {
            EventKind::MessageDelivered
                if event.metadata.message_id.as_deref() == Some(receipt.message_id.as_str()) =>
            {
                println!("delivered message_id={}", receipt.message_id);
                break;
            }
            EventKind::MessageFailed
                if event.metadata.message_id.as_deref() == Some(receipt.message_id.as_str()) =>
            {
                eprintln!("failed message_id={}", receipt.message_id);
                break;
            }
            EventKind::StreamGapDetected(gap) => {
                eprintln!("stream gap detected; recovery_required={}", gap.recovery_required);
                break;
            }
            _ => {}
        }
    }

    client.runtime().stop_async(lxmf_sdk::ShutdownMode::Graceful).await?;
    Ok(())
}
