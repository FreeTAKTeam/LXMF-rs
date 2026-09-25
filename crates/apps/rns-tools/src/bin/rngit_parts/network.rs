use super::{
    decode_page_request, log_page_media_cleanup_failure, page_paths, rngit_paths, Cli,
    GitCommand, PageLinkCleanup, PageResponse, ReticulumGitNode,
};
use super::media_config::media_conversion_enabled;
use rns_transport::destination::link::{LinkEvent, LinkStatus};
use rns_transport::destination::DestinationName;
use rns_transport::error::RnsError;
use rns_transport::hash::AddressHash;
use rns_transport::identity::{Identity, PrivateIdentity};
use rns_transport::iface::tcp_client::TcpClient;
use rns_transport::iface::tcp_server::TcpServer;
use rns_transport::iface::{IfaceRole, InterfaceMode};
use rns_transport::packet::PacketContext;
use rns_transport::resource::ResourceEventKind;
use rns_transport::transport::{Transport, TransportConfig};
use std::fs;
use std::io;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, Mutex};
use tokio::time::interval;

const NULL_IDENTITY: [u8; 16] = [0; 16];

include!("network_fetch.rs");

struct Runtime {
    transport: Arc<Transport>,
    destination: Arc<Mutex<rns_transport::destination::SingleInputDestination>>,
    git_destination: Arc<Mutex<rns_transport::destination::SingleInputDestination>>,
    page_destination_hash: AddressHash,
    git_destination_hash: AddressHash,
    node: Arc<Mutex<ReticulumGitNode>>,
    announce_app_data: Vec<u8>,
    silent: bool,
}

pub(crate) fn run(cli: &Cli) -> io::Result<()> {
    if let Some(GitCommand::Fetch { remote, reference, destination_ref }) = &cli.command {
        return run_git_fetch(cli, remote, reference, destination_ref);
    }
    if let Some(GitCommand::Push { remote, local_ref, remote_ref, force }) = &cli.command {
        return run_git_push(cli, remote, local_ref, remote_ref, *force);
    }
    tokio::runtime::Runtime::new()?.block_on(run_async(cli))
}

async fn run_async(cli: &Cli) -> io::Result<()> {
    if cli.listen.is_empty() && cli.connect.is_empty() && !cli.print_identity {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "network mode needs --listen, --connect, or --print-identity",
        ));
    }
    let root = cli.root.canonicalize()?;
    let identity = load_identity(cli)?;
    let identity_hash = *identity.address_hash();
    let transport = Arc::new(Transport::new(TransportConfig::new("rngit", &identity, true)));
    let destination = transport
        .add_destination(identity.clone(), DestinationName::new("nomadnetwork", "node"))
        .await;
    let git_destination = transport
        .add_destination(identity, DestinationName::new("git", "repositories"))
        .await;
    if cli.print_identity {
        let destination_hash = destination.lock().await.desc.address_hash;
        let git_destination_hash = git_destination.lock().await.desc.address_hash;
        println!("Identity     : {}", hex::encode(identity_hash.as_slice()));
        println!("Listening on : {}", hex::encode(destination_hash.as_slice()));
        println!("Git listening on : {}", hex::encode(git_destination_hash.as_slice()));
        return Ok(());
    }

    spawn_interfaces(&transport, cli).await;
    let mut node = ReticulumGitNode::default();
    node.load_repository_root(&root)?;
    node.load_page_templates(&root.join("templates"))?;
    for value in &cli.blocked_identity_hash {
        let bytes = hex::decode(value).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid blocked identity hash {value:?}: {error}"),
            )
        })?;
        let identity: [u8; 16] = bytes.try_into().map_err(|bytes: Vec<u8>| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("blocked identity hash must be 16 bytes, got {}", bytes.len()),
            )
        })?;
        node.blocked_identities.insert(identity);
    }
    node.media_conversion = !cli.no_media_conversion && media_conversion_enabled(cli.config.as_deref())?;
    node.media_quality = cli.media_quality;
    node.media_max_dimension = cli.media_max_dimension.filter(|value| *value > 0);
    let _registered_page_paths = page_paths();
    if let Ok(name) = fs::read_to_string(root.join(".node_name")) {
        let name = name.trim();
        if !name.is_empty() {
            node.page_node_name = name.to_string();
        }
    }
    let announce_app_data = node.page_node_name.as_bytes().to_vec();
    let page_destination_hash = destination.lock().await.desc.address_hash;
    let git_destination_hash = git_destination.lock().await.desc.address_hash;
    let runtime = Runtime {
        transport,
        destination,
        git_destination,
        page_destination_hash,
        git_destination_hash,
        node: Arc::new(Mutex::new(node)),
        announce_app_data,
        silent: cli.silent,
    };
    serve(runtime).await
}

fn load_identity(cli: &Cli) -> io::Result<PrivateIdentity> {
    let identity = if let Some(path) = cli.identity.as_deref() {
        if path.exists() {
            let bytes = fs::read(path)?;
            PrivateIdentity::from_private_key_bytes(&bytes).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid Reticulum identity")
            })?
        } else {
            create_identity(cli)
        }
    } else {
        create_identity(cli)
    };
    if let Some(path) = cli.identity.as_deref() {
        if !path.exists() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, identity.to_private_key_bytes())?;
        }
    }
    Ok(identity)
}

fn create_identity(cli: &Cli) -> PrivateIdentity {
    cli.identity_seed
        .as_deref()
        .map(PrivateIdentity::new_from_name)
        .unwrap_or_else(|| PrivateIdentity::new_from_rand(rand_core::OsRng))
}

async fn spawn_interfaces(transport: &Arc<Transport>, cli: &Cli) {
    let manager = transport.iface_manager();
    for listen in &cli.listen {
        let mut guard = manager.lock().await;
        guard.spawn_as_with_mode(
            TcpServer::new(listen.clone(), manager.clone()),
            TcpServer::spawn,
            IfaceRole::default(),
            InterfaceMode::default(),
        );
    }
    for connect in &cli.connect {
        let mut guard = manager.lock().await;
        guard.spawn_as_with_mode(
            TcpClient::new(connect.clone()),
            TcpClient::spawn,
            IfaceRole::default(),
            InterfaceMode::default(),
        );
    }
}

async fn serve(runtime: Runtime) -> io::Result<()> {
    serve_with_intervals(runtime, Duration::from_secs(5), Duration::from_secs(60)).await
}

async fn serve_with_intervals(
    runtime: Runtime,
    announce_period: Duration,
    cleanup_period: Duration,
) -> io::Result<()> {
    let mut link_events = runtime.transport.in_link_events();
    let mut data_events = runtime.transport.received_data_events();
    let mut resource_events = runtime.transport.resource_events();
    let mut announce_timer = interval(announce_period);
    let mut cleanup_timer = interval(cleanup_period);
    loop {
        tokio::select! {
            _ = announce_timer.tick() => {
                runtime.transport.send_announce(&runtime.destination, Some(&runtime.announce_app_data)).await;
                runtime.transport.send_announce(&runtime.git_destination, Some(&runtime.announce_app_data)).await;
            }
            _ = cleanup_timer.tick() => {
                let candidates = runtime.node.lock().await.active_page_link_ids();
                let mut statuses = Vec::with_capacity(candidates.len());
                for link_id in candidates {
                    let address = AddressHash::new(link_id);
                    let status = match runtime.transport.find_in_link(&address).await {
                        Some(link) => Some(link.lock().await.status()),
                        None => None,
                    };
                    statuses.push((link_id, status));
                }
                let cleanup = {
                    let mut node = runtime.node.lock().await;
                    clean_stale_page_links(&mut node, statuses)
                };
                log_page_link_cleanup_failures(&cleanup);
                if cleanup.removed_directories > 0 && !runtime.silent {
                    eprintln!("rngit: cleaned {removed} temporary media director{suffix}",
                        removed = cleanup.removed_directories,
                        suffix = if cleanup.removed_directories == 1 { "y" } else { "ies" });
                }
            }
            result = link_events.recv() => match result {
                Ok(event) => {
                    match event.event {
                        LinkEvent::Activated => {
                            if event.address_hash == runtime.page_destination_hash {
                                let link_id = address_array(&event.id);
                                runtime.node.lock().await.page_link_connected(link_id);
                            }
                        }
                        LinkEvent::Closed if event.address_hash == runtime.page_destination_hash => {
                            let cleanup = runtime.node.lock().await.page_link_closed(address_array(&event.id));
                            log_page_link_cleanup_failures(&cleanup);
                            if cleanup.removed_directories > 0 && !runtime.silent {
                                eprintln!("rngit: cleaned {removed} temporary media director{suffix}",
                                    removed = cleanup.removed_directories,
                                    suffix = if cleanup.removed_directories == 1 { "y" } else { "ies" });
                            }
                        }
                        LinkEvent::Closed => {}
                        _ => {}
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(io::Error::other("rngit link event channel closed"));
                }
            },
            result = data_events.recv() => match result {
                Ok(event) if event.context == Some(PacketContext::Request) => {
                    if let Some(request_id) = event.request_id {
                        let runtime = &runtime;
                        process_request(runtime, event.destination, request_id.to_vec(), event.data.as_slice().to_vec()).await;
                    }
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(io::Error::other("rngit received-data channel closed"));
                }
            },
            result = resource_events.recv() => match result {
                Ok(event) => {
                    if let ResourceEventKind::Complete(complete) = event.kind {
                        if complete.is_request {
                            if let Some(request_id) = complete.request_id {
                                process_request(&runtime, event.link_id, request_id, complete.data).await;
                            }
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(io::Error::other("rngit Resource event channel closed"));
                }
            },
        }
    }
}

#[cfg(test)]
mod periodic_cleanup_tests {
    include!("periodic_cleanup_tests.rs");
}

include!("page_link_cleanup.rs");

async fn process_request(
    runtime: &Runtime,
    link_id: AddressHash,
    request_id: Vec<u8>,
    payload: Vec<u8>,
) {
    let Some(service) = service_for_link(runtime, &link_id).await else { return };
    let peer_identity = remote_peer_identity(runtime, &link_id).await;
    let remote = peer_identity
        .as_ref()
        .map(|identity| address_array(&identity.address_hash))
        .unwrap_or(NULL_IDENTITY);
    let response = match service {
        RequestService::Pages => {
            let Some(request) = decode_page_request(&payload).ok().flatten() else { return };
            let _ = request.requested_at;
            let Some(link) = runtime.transport.find_in_link(&link_id).await else { return };
            let mut cancelled = || {
                link.try_lock()
                    .is_ok_and(|guard| !matches!(guard.status(), LinkStatus::Active))
            };
            let page_response = {
                let mut node = runtime.node.lock().await;
                node.handle_page_request_with_cancel(
                    request.path,
                    &request.data,
                    remote,
                    address_array(&link_id),
                    &mut cancelled,
                )
            };
            page_response
        }
        RequestService::Git => {
            let Some(request) = decode_rngit_request(&payload).ok().flatten() else { return };
            let _ = request.requested_at;
            let mut encoded = Vec::new();
            if rmpv::encode::write_value(&mut encoded, &request.data).is_err() {
                return;
            }
            let mut node = runtime.node.lock().await;
            let data = node.handle_request_with_peer_identity(
                request.path,
                &encoded,
                remote,
                peer_identity,
            );
            Some(PageResponse { data, metadata: None, response_is_false: false })
        }
    };
    if let Some(response) = response {
        if let Err(error) = send_response(runtime, link_id, request_id, response).await {
            if !runtime.silent {
                eprintln!("rngit: could not send NomadNet response: {error}");
            }
            if matches!(service, RequestService::Pages)
                && error.kind() == io::ErrorKind::NotConnected
            {
                if let Some(link) = runtime.transport.find_in_link(&link_id).await {
                    link.lock().await.close();
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum RequestService {
    Pages,
    Git,
}

async fn service_for_link(runtime: &Runtime, link_id: &AddressHash) -> Option<RequestService> {
    let link = runtime.transport.find_in_link(link_id).await?;
    let destination = link.lock().await.destination().address_hash;
    if destination == runtime.page_destination_hash {
        Some(RequestService::Pages)
    } else if destination == runtime.git_destination_hash {
        Some(RequestService::Git)
    } else {
        None
    }
}

fn decode_rngit_request(data: &[u8]) -> Result<Option<DecodedRngitRequest>, String> {
    let mut cursor = std::io::Cursor::new(data);
    let value = rmpv::decode::read_value(&mut cursor)
        .map_err(|error| format!("invalid rngit request: {error}"))?;
    if cursor.position() != data.len() as u64 {
        return Err("rngit request has trailing bytes".to_string());
    }
    let Some(values) = value.as_array() else { return Ok(None) };
    if values.len() != 3 {
        return Ok(None);
    }
    let requested_at = values[0]
        .as_f64()
        .or_else(|| values[0].as_u64().map(|value| value as f64))
        .ok_or_else(|| "rngit request timestamp is not numeric".to_string())?;
    let Some(path_hash) = values[1].as_slice() else { return Ok(None) };
    let Some(path) = rngit_paths()
        .iter()
        .copied()
        .find(|path| rns_transport::hash::address_hash(path.as_bytes()) == path_hash)
    else {
        return Ok(None);
    };
    Ok(Some(DecodedRngitRequest { path, requested_at, data: values[2].clone() }))
}

#[derive(Debug, Clone)]
struct DecodedRngitRequest {
    path: &'static str,
    requested_at: f64,
    data: rmpv::Value,
}

async fn remote_peer_identity(runtime: &Runtime, link_id: &AddressHash) -> Option<Identity> {
    let link = runtime.transport.find_in_link(link_id).await?;
    let guard = link.lock().await;
    let identity = guard.identified_peer_identity()?;
    let address_hash = identity.address_hash.as_slice();
    if address_hash.len() != 16 {
        if !runtime.silent {
            eprintln!(
                "rngit: identified peer identity for link {} has an invalid address hash length: {}",
                hex::encode(link_id.as_slice()),
                address_hash.len()
            );
        }
        return None;
    }
    Some(*identity)
}

async fn send_response(
    runtime: &Runtime,
    link_id: AddressHash,
    request_id: Vec<u8>,
    response: PageResponse,
) -> io::Result<()> {
    if let Some(metadata) = response.metadata {
        runtime
            .transport
            .send_response_resource_with_compression(
                &link_id,
                request_id,
                response.data,
                Some(metadata),
                false,
            )
            .await
            .map_err(|error| match error {
                connection @ RnsError::ConnectionError => io::Error::new(
                    io::ErrorKind::NotConnected,
                    format!("media Resource response failed: {connection:?}"),
                ),
                other => io::Error::other(format!("media Resource response failed: {other:?}")),
            })?;
        return Ok(());
    }

    let response_value = if response.response_is_false {
        rmpv::Value::Boolean(false)
    } else {
        rmpv::Value::Binary(response.data)
    };
    let envelope = rmpv::Value::Array(vec![rmpv::Value::Binary(request_id.clone()), response_value]);
    let mut encoded = Vec::new();
    rmpv::encode::write_value(&mut encoded, &envelope).map_err(io::Error::other)?;
    let link = runtime.transport.find_in_link(&link_id).await.ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotConnected, "incoming link disappeared")
    })?;
    let packet = {
        let guard = link.lock().await;
        if encoded.len() <= guard.link_mdu() {
            Some(guard.response_packet(&encoded).map_err(|error| {
                io::Error::other(format!("could not build page response packet: {error:?}"))
            })?)
        } else {
            None
        }
    };
    if let Some(packet) = packet {
        let outcome = runtime.transport.send_link_packet_on_bound_iface(&link, packet).await;
        if matches!(
            outcome,
            rns_transport::transport::SendPacketOutcome::SentDirect
                | rns_transport::transport::SendPacketOutcome::SentBroadcast
        ) {
            Ok(())
        } else {
            Err(io::Error::new(io::ErrorKind::NotConnected, "could not send page response packet"))
        }
    } else {
        runtime
            .transport
            .send_response_resource(&link_id, request_id, encoded, None)
            .await
            .map_err(|error| match error {
                connection @ RnsError::ConnectionError => io::Error::new(
                    io::ErrorKind::NotConnected,
                    format!("page Resource response failed: {connection:?}"),
                ),
                other => io::Error::other(format!("page Resource response failed: {other:?}")),
            })?;
        Ok(())
    }
}

fn address_array(value: &AddressHash) -> [u8; 16] {
    value.as_slice().try_into().unwrap_or(NULL_IDENTITY)
}
