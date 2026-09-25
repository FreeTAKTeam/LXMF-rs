use super::network::{self, Runtime};
use super::protocol;
use rns_transport::destination::link::LinkEvent;
use rns_transport::hash::AddressHash;
use rns_transport::packet::PacketContext;
use rns_transport::resource::{ResourceEvent, ResourceEventKind};
use rns_transport::transport::{ReceivedData, SendPacketOutcome};
use std::collections::HashMap;
use std::io;
use tokio::sync::broadcast;
use tokio::time::{interval, Duration};

const REQUEST_RESPONSE_GRACE: Duration = Duration::from_millis(50);
const MAX_PENDING_REQUESTS_PER_LINK: usize = 8;

pub(crate) async fn serve(runtime: Runtime) -> io::Result<()> {
    let mut link_events = runtime.transport.in_link_events();
    let mut data_events = runtime.transport.received_data_events();
    let mut resource_events = runtime.transport.resource_events();
    let mut announce_timer = interval(Duration::from_secs(5));
    let mut authorized = HashMap::<AddressHash, bool>::new();
    let mut pending_requests = HashMap::<AddressHash, Vec<ReceivedData>>::new();

    loop {
        tokio::select! {
            _ = announce_timer.tick() => {
                network::announce(&runtime).await;
            }
            result = link_events.recv() => match result {
                Ok(event) => {
                    if let LinkEvent::PeerIdentified(identity) = &event.event {
                        let accepted = runtime.no_auth || runtime.allowed.contains(&identity.address_hash);
                        authorized.insert(event.id, accepted);
                        if !accepted {
                            pending_requests.remove(&event.id);
                            if !runtime.silent {
                                eprintln!("rncp: rejected unauthorised sender {}", identity.address_hash.to_hex_string());
                            }
                            close_link(&runtime, event.id).await;
                        } else if let Some(requests) = pending_requests.remove(&event.id) {
                            for request in requests {
                                handle_fetch_request(&runtime, request).await?;
                            }
                        }
                    }
                    if matches!(event.event, LinkEvent::Closed) {
                        authorized.remove(&event.id);
                        pending_requests.remove(&event.id);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(io::Error::other("link event channel closed"));
                }
            },
            result = data_events.recv() => match result {
                Ok(event) if event.context == Some(PacketContext::Request) => {
                    match authorized.get(&event.destination).copied() {
                        Some(true) => handle_fetch_request(&runtime, event).await?,
                        Some(false) => {}
                        None if runtime.no_auth => handle_fetch_request(&runtime, event).await?,
                        None => {
                            let link_id = event.destination;
                            let queue_full = {
                                let requests = pending_requests.entry(link_id).or_default();
                                if requests.len() >= MAX_PENDING_REQUESTS_PER_LINK {
                                    true
                                } else {
                                    requests.push(event);
                                    false
                                }
                            };
                            if queue_full {
                                pending_requests.remove(&link_id);
                                if !runtime.silent {
                                    eprintln!(
                                        "rncp: too many requests before peer identification {}",
                                        link_id
                                    );
                                }
                                close_link(&runtime, link_id).await;
                            }
                        }
                    }
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(io::Error::other("received-data channel closed"));
                }
            },
            result = resource_events.recv() => match result {
                Ok(event) => handle_resource_event(&runtime, &authorized, event).await,
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(io::Error::other("Resource event channel closed"));
                }
            },
        }
    }
}

async fn handle_fetch_request(runtime: &Runtime, event: ReceivedData) -> io::Result<()> {
    let Some(request_id) = event.request_id else { return Ok(()) };
    let Some(requested) = protocol::request_path_and_file(event.data.as_slice())? else {
        return Ok(());
    };

    if !runtime.allow_fetch {
        send_response(runtime, event.destination, request_id, rmpv::Value::Integer(0xF0.into()))
            .await?;
        return Ok(());
    }

    let path = match protocol::resolve_fetch_path(&requested, runtime.jail.as_deref()) {
        Ok(path) => path,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            send_response(runtime, event.destination, request_id, rmpv::Value::Boolean(false))
                .await?;
            return Ok(());
        }
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            send_response(
                runtime,
                event.destination,
                request_id,
                rmpv::Value::Integer(0xF0.into()),
            )
            .await?;
            return Ok(());
        }
        Err(error) => {
            send_response(runtime, event.destination, request_id, rmpv::Value::Nil).await?;
            if !runtime.silent {
                eprintln!("rncp: could not resolve fetch path: {error}");
            }
            return Ok(());
        }
    };
    let data = match network::read_file(&path) {
        Ok(data) => data,
        Err(error) => {
            send_response(runtime, event.destination, request_id, rmpv::Value::Nil).await?;
            if !runtime.silent {
                eprintln!("rncp: could not read fetch path: {error}");
            }
            return Ok(());
        }
    };
    send_response(runtime, event.destination, request_id, rmpv::Value::Boolean(true)).await?;
    // Give the Python fetch loop time to switch the link to ACCEPT_ALL after
    // its response callback resolves the request status. Otherwise the
    // Resource advertisement can arrive before that callback returns.
    tokio::time::sleep(REQUEST_RESPONSE_GRACE).await;
    let metadata = protocol::encode_metadata(&path)?;
    // Python rncp returns True for the request and starts an ordinary,
    // metadata-bearing Resource; it is not a response-flagged Resource.
    runtime
        .transport
        .send_resource_with_compression(
            &event.destination,
            data,
            Some(metadata),
            !runtime.no_compress,
        )
        .await
        .map_err(|error| io::Error::other(format!("could not send fetch Resource: {error:?}")))?;
    Ok(())
}

async fn send_response(
    runtime: &Runtime,
    link_id: AddressHash,
    request_id: [u8; 16],
    response: rmpv::Value,
) -> io::Result<()> {
    // Python's Link.request sends the packet before it appends its
    // RequestReceipt to pending_requests. A Rust response can otherwise win
    // that small race and be discarded before rncp's fetch callback exists.
    tokio::time::sleep(REQUEST_RESPONSE_GRACE).await;
    let link =
        runtime.transport.find_in_link(&link_id).await.ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotConnected, "incoming link disappeared")
        })?;
    let frame = protocol::response_frame(request_id, response)?;
    let packet = {
        let guard = link.lock().await;
        guard.response_packet(&frame).map_err(|error| {
            io::Error::other(format!("could not build response packet: {error:?}"))
        })?
    };
    let outcome = runtime.transport.send_link_packet_on_bound_iface(&link, packet).await;
    if matches!(outcome, SendPacketOutcome::SentDirect | SendPacketOutcome::SentBroadcast) {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::NotConnected, "could not send response packet"))
    }
}

async fn handle_resource_event(
    runtime: &Runtime,
    authorized: &HashMap<AddressHash, bool>,
    event: ResourceEvent,
) {
    match event.kind {
        ResourceEventKind::Complete(complete)
            if !complete.is_response
                && authorized.get(&event.link_id).copied().unwrap_or(runtime.no_auth) =>
        {
            match protocol::save_received(&complete, runtime.save.as_deref(), runtime.overwrite) {
                Ok(path) => {
                    if !runtime.silent {
                        println!("saved received file to {}", path.display());
                    }
                }
                Err(error) if !runtime.silent => {
                    eprintln!("rncp: could not save received file: {error}")
                }
                Err(_) => {}
            }
        }
        ResourceEventKind::InboundFailed(failure) if !runtime.silent => {
            eprintln!("rncp: incoming Resource failed: {}", failure.reason);
        }
        ResourceEventKind::OutboundFailed if !runtime.silent => {
            eprintln!("rncp: outgoing Resource failed");
        }
        ResourceEventKind::OutboundComplete if !runtime.silent => {
            eprintln!("rncp: outgoing Resource complete ({})", hex::encode(event.hash.as_slice()));
        }
        ResourceEventKind::OutboundRejected if !runtime.silent => {
            eprintln!("rncp: outgoing Resource rejected");
        }
        ResourceEventKind::OutboundCancelled if !runtime.silent => {
            eprintln!("rncp: outgoing Resource cancelled");
        }
        _ => {}
    }
}

async fn close_link(runtime: &Runtime, link_id: AddressHash) {
    let Some(link) = runtime.transport.find_in_link(&link_id).await else { return };
    let packet = link.lock().await.teardown();
    if let Some(packet) = packet {
        let _ = runtime.transport.send_link_packet_on_bound_iface(&link, packet).await;
    }
}
