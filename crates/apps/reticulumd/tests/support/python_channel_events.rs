use std::io::Read;
use std::process::Child;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use rns_transport::channel_buffer::RawChannelReader;
use rns_transport::destination::link::{Link, LinkEvent, LinkEventData, LinkStatus};
use rns_transport::destination::{DestinationDesc, SingleInputDestination};
use rns_transport::hash::{AddressHash, Hash};
use rns_transport::resource::{ResourceEvent, ResourceEventKind};
use rns_transport::transport::{ReceivedData, Transport};
use tokio::time::{sleep, timeout, Instant};

pub(super) async fn wait_for_announce(
    transport: &Transport,
    target_hash: AddressHash,
    duration: Duration,
) -> DestinationDesc {
    let mut announces = transport.recv_announces().await;
    timeout(duration, async {
        loop {
            let event = announces.recv().await.expect("announce event");
            let destination = event.destination.lock().await.desc;
            if destination.address_hash == target_hash {
                return destination;
            }
        }
    })
    .await
    .expect("timed out waiting for Python announce")
}

pub(super) async fn wait_for_out_link_active(
    events: &mut tokio::sync::broadcast::Receiver<LinkEventData>,
    link: &Arc<tokio::sync::Mutex<Link>>,
    duration: Duration,
) -> AddressHash {
    let expected = *link.lock().await.id();
    timeout(duration, async {
        loop {
            let event = events.recv().await.expect("link event");
            if event.id == expected && matches!(event.event, LinkEvent::Activated) {
                assert_eq!(link.lock().await.status(), LinkStatus::Active);
                return expected;
            }
        }
    })
    .await
    .expect("timed out waiting for Rust link activation")
}

pub(super) async fn wait_for_in_link_active_with_announces(
    transport: &Transport,
    destination: &Arc<tokio::sync::Mutex<SingleInputDestination>>,
    events: &mut tokio::sync::broadcast::Receiver<LinkEventData>,
    duration: Duration,
) -> AddressHash {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        transport.send_announce(destination, None).await;
        let remaining = deadline.saturating_duration_since(Instant::now());
        let slice = remaining.min(Duration::from_millis(250));
        match timeout(slice, events.recv()).await {
            Ok(Ok(event)) if matches!(event.event, LinkEvent::Activated) => return event.id,
            Ok(Ok(_)) => {}
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {}
            Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => {
                panic!("inbound link event channel closed")
            }
            Err(_) => {}
        }
    }
    panic!("timed out waiting for Python-initiated Rust link activation")
}

pub(super) async fn wait_for_reply(
    seen: &Arc<StdMutex<Vec<(String, String)>>>,
    duration: Duration,
) {
    wait_for_reply_tuple(seen, duration, "rust-1", "reply:hello-python").await;
}

pub(super) async fn wait_for_reply_tuple(
    seen: &Arc<StdMutex<Vec<(String, String)>>>,
    duration: Duration,
    expected_id: &str,
    expected_data: &str,
) {
    wait_for_seen_tuple(seen, duration, "Python channel reply", |id, data| {
        id == expected_id && data == expected_data
    })
    .await;
}

pub(super) async fn wait_for_python_message(
    seen: &Arc<StdMutex<Vec<(String, String)>>>,
    duration: Duration,
) {
    wait_for_seen_tuple(seen, duration, "Python channel message", |id, data| {
        id == "python-1" && data == "hello-rust"
    })
    .await;
}

pub(super) async fn wait_for_resource_ack(
    seen: &Arc<StdMutex<Vec<(String, String)>>>,
    duration: Duration,
) {
    wait_for_seen_tuple(seen, duration, "Python resource acknowledgement", |id, data| {
        id == "rust-resource" && data == "resource:rust-resource-data:rust-meta"
    })
    .await;
}

pub(super) async fn wait_for_identify_ack(
    seen: &Arc<StdMutex<Vec<(String, String)>>>,
    duration: Duration,
) {
    wait_for_seen_tuple(seen, duration, "Python identify acknowledgement", |id, data| {
        id == "rust-identify" && data.starts_with("identified:")
    })
    .await;
}

async fn wait_for_seen_tuple(
    seen: &Arc<StdMutex<Vec<(String, String)>>>,
    duration: Duration,
    label: &str,
    matches_seen: impl Fn(&str, &str) -> bool,
) {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        {
            let seen = seen.lock().expect("seen lock");
            if seen.iter().any(|(id, data)| matches_seen(id, data)) {
                return;
            }
        }
        sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for {label}");
}

pub(super) async fn wait_for_buffer_data(
    reader: &RawChannelReader,
    expected: &[u8],
    duration: Duration,
) {
    let deadline = Instant::now() + duration;
    let mut received = Vec::new();
    while Instant::now() < deadline {
        if let Some(chunk) = reader.read(usize::MAX) {
            received.extend_from_slice(&chunk);
            if received.windows(expected.len()).any(|window| window == expected) {
                return;
            }
        }
        sleep(Duration::from_millis(50)).await;
    }
    panic!(
        "timed out waiting for buffer data {:?}; received {:?}",
        String::from_utf8_lossy(expected),
        String::from_utf8_lossy(&received)
    );
}

pub(super) async fn wait_for_outbound_resource_complete(
    events: &mut tokio::sync::broadcast::Receiver<ResourceEvent>,
    expected_hash: Hash,
    duration: Duration,
) {
    timeout(duration, async {
        loop {
            let event = events.recv().await.expect("resource event");
            if event.hash == expected_hash
                && matches!(event.kind, ResourceEventKind::OutboundComplete)
            {
                return;
            }
        }
    })
    .await
    .expect("timed out waiting for outbound resource completion");
}

pub(super) async fn wait_for_inbound_resource_complete(
    events: &mut tokio::sync::broadcast::Receiver<ResourceEvent>,
    expected_data: &[u8],
    expected_metadata: &str,
    duration: Duration,
) {
    timeout(duration, async {
        loop {
            let event = events.recv().await.expect("resource event");
            if let ResourceEventKind::Complete(complete) = event.kind {
                if complete.data == expected_data {
                    let metadata = complete.metadata.expect("resource metadata");
                    let decoded: String =
                        rmp_serde::from_slice(&metadata).expect("decode resource metadata");
                    assert_eq!(decoded, expected_metadata);
                    return;
                }
            }
        }
    })
    .await
    .expect("timed out waiting for inbound resource completion");
}

pub(super) async fn wait_for_inbound_resource_data_or_child_exit(
    events: &mut tokio::sync::broadcast::Receiver<ResourceEvent>,
    link_id: AddressHash,
    child: &mut Child,
    duration: Duration,
) -> Vec<u8> {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        match timeout(Duration::from_millis(100), events.recv()).await {
            Ok(Ok(event)) if event.link_id == link_id => {
                if let ResourceEventKind::Complete(complete) = event.kind {
                    return complete.data;
                }
            }
            Ok(Ok(_)) => {}
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {}
            Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => {
                panic!("resource event channel closed")
            }
            Err(_) => {}
        }
        if let Some(status) = child.try_wait().expect("poll python child") {
            let mut stdout = String::new();
            if let Some(mut pipe) = child.stdout.take() {
                let _ = pipe.read_to_string(&mut stdout);
            }
            let mut stderr = String::new();
            if let Some(mut pipe) = child.stderr.take() {
                let _ = pipe.read_to_string(&mut stderr);
            }
            panic!(
                "python child exited before resource completion: {status}\nstdout:\n{stdout}\nstderr:\n{stderr}"
            );
        }
    }
    let _ = child.kill();
    let status = child.wait().expect("wait for timed-out python child");
    let mut stdout = String::new();
    if let Some(mut pipe) = child.stdout.take() {
        let _ = pipe.read_to_string(&mut stdout);
    }
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    panic!(
        "timed out waiting for inbound resource data; python child status after kill: {status}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

pub(super) async fn wait_for_link_data(
    events: &mut tokio::sync::broadcast::Receiver<ReceivedData>,
    link_id: AddressHash,
    expected: &[u8],
    duration: Duration,
) {
    timeout(duration, async {
        loop {
            let event = events.recv().await.expect("received data event");
            if event.destination == link_id && event.data.as_slice() == expected {
                return;
            }
        }
    })
    .await
    .expect("timed out waiting for link data");
}
