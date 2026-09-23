use super::network::{self, Runtime};
use super::protocol::{self, ResponseStatus};
use rns_transport::destination::link::{unpack_response_envelope, Link, LinkEvent, LinkStatus};
use rns_transport::destination::DestinationDesc;
use rns_transport::hash::AddressHash;
use rns_transport::packet::PacketContext;
use rns_transport::resource::ResourceEventKind;
use rns_transport::transport::{ReceivedData, SendPacketOutcome, Transport};
use std::io::{self, Write};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use tokio::time::{timeout, Duration};

pub(crate) async fn send(runtime: &Runtime, source: &Path, destination: &str) -> io::Result<()> {
    let source_name = source.to_path_buf();
    let data = network::read_file(&source_name)?;
    let metadata = protocol::encode_metadata(&source_name)?;
    let target = protocol::parse_address(destination)?;
    emit_status(runtime.silent, format!("Path to {} requested", target.to_hex_string()))?;
    let description = wait_for_destination(runtime, target).await?;
    emit_status(runtime.silent, format!("Establishing link with {}", target.to_hex_string()))?;
    let (link, link_id) = establish_link(runtime, description).await?;
    let mut events = runtime.transport.resource_events();
    let resource_hash = runtime
        .transport
        .send_resource_with_compression(&link_id, data, Some(metadata), !runtime.no_compress)
        .await
        .map_err(|error| io::Error::other(format!("could not start Resource: {error:?}")))?;
    emit_status(runtime.silent, "Transferring file...")?;
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
    emit_status(runtime.silent, format!("Path to {} requested", target.to_hex_string()))?;
    let description = wait_for_destination(runtime, target).await?;
    emit_status(runtime.silent, format!("Establishing link with {}", target.to_hex_string()))?;
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
    emit_status(runtime.silent, "Requesting file from remote...")?;
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

fn emit_status(silent: bool, message: impl AsRef<str>) -> io::Result<()> {
    if silent {
        return Ok(());
    }
    println!("{}", message.as_ref());
    io::stdout().flush()
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
                if is_matching_fetch_resource(&complete, request_id) {
                    return Ok(complete);
                }
            }
        }
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "file Resource timed out"))?
}

fn is_matching_fetch_resource(
    complete: &rns_transport::resource::ResourceComplete,
    request_id: [u8; 16],
) -> bool {
    let correlated_response =
        complete.is_response && complete.request_id.as_deref() == Some(request_id.as_slice());
    // The pinned Python rncp listener returns True for the Link request and
    // starts a separate ordinary Resource for the file. That Resource has
    // metadata but no request id or response flag, so it cannot use the
    // normal response-resource correlation contract.
    let python_rncp_file = !complete.is_request
        && !complete.is_response
        && complete.request_id.is_none()
        && complete.metadata.is_some();
    correlated_response || python_rncp_file
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
                ResourceEventKind::OutboundRejected => {
                    Err(io::Error::other("Resource transfer rejected"))
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

#[cfg(test)]
mod tests {
    use super::is_matching_fetch_resource;
    use rns_transport::resource::ResourceComplete;

    fn complete(
        request_id: Option<Vec<u8>>,
        is_response: bool,
        has_metadata: bool,
    ) -> ResourceComplete {
        ResourceComplete {
            data: Vec::new(),
            metadata: has_metadata.then(|| vec![0x01]),
            request_id,
            is_request: false,
            is_response,
        }
    }

    #[test]
    fn fetch_accepts_correlated_response_resources() {
        let request_id = [0x11; 16];
        assert!(is_matching_fetch_resource(
            &complete(Some(request_id.to_vec()), true, true),
            request_id,
        ));
    }

    #[test]
    fn fetch_accepts_python_rncp_unassociated_file_resources() {
        let request_id = [0x22; 16];
        assert!(is_matching_fetch_resource(&complete(None, false, true), request_id,));
    }

    #[test]
    fn fetch_rejects_unassociated_resources_without_file_metadata() {
        let request_id = [0x33; 16];
        assert!(!is_matching_fetch_resource(&complete(None, false, false), request_id,));
    }
}
