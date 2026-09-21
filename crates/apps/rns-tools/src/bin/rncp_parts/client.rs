use super::network::{self, Runtime};
use super::protocol::{self, ResponseStatus};
use rns_transport::destination::link::{unpack_response_envelope, Link, LinkEvent, LinkStatus};
use rns_transport::destination::DestinationDesc;
use rns_transport::hash::AddressHash;
use rns_transport::packet::PacketContext;
use rns_transport::resource::ResourceEventKind;
use rns_transport::transport::{ReceivedData, SendPacketOutcome, Transport};
use std::io;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use tokio::time::{timeout, Duration};

pub(crate) async fn send(runtime: &Runtime, source: &Path, destination: &str) -> io::Result<()> {
    let source_name = source.to_path_buf();
    let data = network::read_file(&source_name)?;
    let metadata = protocol::encode_metadata(&source_name)?;
    let target = protocol::parse_address(destination)?;
    let description = wait_for_destination(runtime, target).await?;
    let (link, link_id) = establish_link(runtime, description).await?;
    let mut events = runtime.transport.resource_events();
    let resource_hash = runtime
        .transport
        .send_resource_with_compression(&link_id, data, Some(metadata), !runtime.no_compress)
        .await
        .map_err(|error| io::Error::other(format!("could not start Resource: {error:?}")))?;
    wait_for_outbound(&mut events, resource_hash, network::operation_timeout(runtime).await)
        .await?;
    if !runtime.silent {
        println!("{} copied to {}", source_name.display(), target.to_hex_string());
    }
    close_link(&runtime.transport, &link).await;
    Ok(())
}

pub(crate) async fn fetch(
    runtime: &Runtime,
    remote_file: &Path,
    destination: &str,
) -> io::Result<()> {
    let target = protocol::parse_address(destination)?;
    let description = wait_for_destination(runtime, target).await?;
    let (link, link_id) = establish_link(runtime, description).await?;
    let request = Link::request_payload(
        "fetch_file",
        rmpv::Value::String(remote_file.to_string_lossy().into_owned().into()),
    )
    .map_err(|error| io::Error::other(format!("could not build fetch request: {error:?}")))?;
    let packet = {
        let guard = link.lock().await;
        guard
            .request_packet(&request.packed)
            .map_err(|error| io::Error::other(format!("could not build fetch packet: {error:?}")))?
    };
    let request_id = request_id_from_packet(&packet);
    let mut data_events = runtime.transport.received_data_events();
    let mut resource_events = runtime.transport.resource_events();
    send_on_link(&runtime.transport, &link, packet).await?;
    let status = wait_for_response(
        &mut data_events,
        link_id,
        request_id,
        network::operation_timeout(runtime).await,
    )
    .await?;
    match protocol::response_status(&status)? {
        ResponseStatus::Found => {}
        ResponseStatus::NotFound => {
            return Err(io::Error::new(io::ErrorKind::NotFound, "remote file was not found"))
        }
        ResponseStatus::NotAllowed => {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "remote fetch was not allowed",
            ))
        }
        ResponseStatus::RemoteError => return Err(io::Error::other("remote fetch failed")),
    }
    let complete = wait_for_response_resource(
        &mut resource_events,
        link_id,
        request_id,
        network::operation_timeout(runtime).await,
    )
    .await?;
    let saved = protocol::save_received(&complete, runtime.save.as_deref(), runtime.overwrite)?;
    if !runtime.silent {
        println!("{} fetched from {}", saved.display(), target.to_hex_string());
    }
    close_link(&runtime.transport, &link).await;
    Ok(())
}

async fn wait_for_destination(
    runtime: &Runtime,
    target: AddressHash,
) -> io::Result<DestinationDesc> {
    let mut announces = runtime.transport.recv_announces().await;
    runtime.transport.request_path(&target, None, None).await;
    let duration = network::operation_timeout(runtime).await;
    timeout(duration, async {
        loop {
            match announces.recv().await {
                Ok(event) => {
                    let description = event.destination.lock().await.desc;
                    if description.address_hash == target {
                        return Ok(description);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(io::Error::other("announce channel closed"));
                }
            }
        }
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "path discovery timed out"))?
}

async fn establish_link(
    runtime: &Runtime,
    description: DestinationDesc,
) -> io::Result<(Arc<Mutex<Link>>, AddressHash)> {
    let mut events = runtime.transport.out_link_events();
    let link = runtime.transport.link(description).await;
    let link_id = *link.lock().await.id();
    timeout(network::operation_timeout(runtime).await, async {
        loop {
            match events.recv().await {
                Ok(event) if event.id == link_id && matches!(event.event, LinkEvent::Activated) => {
                    return Ok(());
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(io::Error::other("link event channel closed"));
                }
            }
        }
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "link establishment timed out"))??;

    let identify = {
        let guard = link.lock().await;
        let payload = guard.identify_payload(&runtime.identity);
        guard.identify_packet(&payload).map_err(|error| {
            io::Error::other(format!("could not build identity packet: {error:?}"))
        })?
    };
    send_on_link(&runtime.transport, &link, identify).await?;
    Ok((link, link_id))
}

async fn send_on_link(
    transport: &Transport,
    link: &Arc<Mutex<Link>>,
    packet: rns_transport::packet::Packet,
) -> io::Result<()> {
    let outcome = transport.send_link_packet_on_bound_iface(link, packet).await;
    if matches!(outcome, SendPacketOutcome::SentDirect | SendPacketOutcome::SentBroadcast) {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::NotConnected, "link has no usable interface"))
    }
}

fn request_id_from_packet(packet: &rns_transport::packet::Packet) -> [u8; 16] {
    let hash = packet.hash().to_bytes();
    let mut request_id = [0u8; 16];
    request_id.copy_from_slice(&hash[..16]);
    request_id
}

async fn wait_for_response(
    events: &mut broadcast::Receiver<ReceivedData>,
    link_id: AddressHash,
    request_id: [u8; 16],
    duration: Duration,
) -> io::Result<rmpv::Value> {
    timeout(duration, async {
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(io::Error::other("received-data channel closed"));
                }
            };
            if event.destination != link_id || event.context != Some(PacketContext::Response) {
                continue;
            }
            let Ok((received_id, response)) = unpack_response_envelope(event.data.as_slice())
            else {
                continue;
            };
            if received_id == request_id {
                return Ok(response);
            }
        }
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "remote fetch response timed out"))?
}

async fn wait_for_response_resource(
    events: &mut broadcast::Receiver<rns_transport::resource::ResourceEvent>,
    link_id: AddressHash,
    request_id: [u8; 16],
    duration: Duration,
) -> io::Result<rns_transport::resource::ResourceComplete> {
    timeout(duration, async {
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(io::Error::other("Resource event channel closed"));
                }
            };
            if event.link_id != link_id {
                continue;
            }
            if let ResourceEventKind::Complete(complete) = event.kind {
                if complete.is_response
                    && complete.request_id.as_deref() == Some(request_id.as_slice())
                {
                    return Ok(complete);
                }
            }
        }
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "file Resource timed out"))?
}

async fn wait_for_outbound(
    events: &mut broadcast::Receiver<rns_transport::resource::ResourceEvent>,
    resource_hash: rns_transport::hash::Hash,
    duration: Duration,
) -> io::Result<()> {
    timeout(duration, async {
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(io::Error::other("Resource event channel closed"));
                }
            };
            if event.hash != resource_hash {
                continue;
            }
            return match event.kind {
                ResourceEventKind::OutboundComplete => Ok(()),
                ResourceEventKind::OutboundFailed => {
                    Err(io::Error::other("Resource transfer failed"))
                }
                ResourceEventKind::OutboundCancelled => {
                    Err(io::Error::other("Resource transfer cancelled"))
                }
                _ => continue,
            };
        }
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Resource transfer timed out"))?
}

async fn close_link(transport: &Transport, link: &Arc<Mutex<Link>>) {
    let packet = {
        let mut guard = link.lock().await;
        (guard.status() != LinkStatus::Closed).then(|| guard.teardown()).flatten()
    };
    if let Some(packet) = packet {
        let _ = transport.send_link_packet_on_bound_iface(link, packet).await;
    }
}
