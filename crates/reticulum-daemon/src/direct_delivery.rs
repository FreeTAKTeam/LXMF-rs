use std::io;

use reticulum::destination::link::{LinkEvent, LinkStatus};
use reticulum::destination::DestinationDesc;
use reticulum::packet::Packet;
use reticulum::transport::{SendPacketOutcome, Transport};
use tokio::time::{timeout, Duration, Instant};

pub async fn send_via_link(
    transport: &Transport,
    destination: DestinationDesc,
    payload: &[u8],
    wait_timeout: Duration,
) -> io::Result<Packet> {
    let link = transport.link(destination).await;
    let link_id = *link.lock().await.id();

    if link.lock().await.status() != LinkStatus::Active {
        let mut events = transport.out_link_events();
        let deadline = Instant::now() + wait_timeout;

        loop {
            if link.lock().await.status() == LinkStatus::Active {
                break;
            }

            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "link activation timed out"));
            }

            // Poll in short slices so activation can be detected even if the
            // activation event was emitted before subscribing.
            let wait_slice = remaining.min(Duration::from_millis(250));
            match timeout(wait_slice, events.recv()).await {
                Ok(Ok(event)) => {
                    if event.id == link_id {
                        if let LinkEvent::Activated = event.event {
                            break;
                        }
                    }
                }
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "link event channel closed",
                    ));
                }
                Err(_) => continue,
            }
        }
    }

    let packet = link
        .lock()
        .await
        .data_packet(payload)
        .map_err(|err| io::Error::other(format!("{:?}", err)))?;

    let outcome = transport.send_packet_with_outcome(packet).await;
    if !matches!(outcome, SendPacketOutcome::SentDirect | SendPacketOutcome::SentBroadcast) {
        return Err(io::Error::other(format!(
            "link packet not sent: {}",
            send_outcome_label(outcome)
        )));
    }

    Ok(packet)
}

fn send_outcome_label(outcome: SendPacketOutcome) -> &'static str {
    match outcome {
        SendPacketOutcome::SentDirect => "sent direct",
        SendPacketOutcome::SentBroadcast => "sent broadcast",
        SendPacketOutcome::DroppedMissingDestinationIdentity => "missing destination identity",
        SendPacketOutcome::DroppedCiphertextTooLarge => "ciphertext too large",
        SendPacketOutcome::DroppedEncryptFailed => "encrypt failed",
        SendPacketOutcome::DroppedNoRoute => "no route",
    }
}
