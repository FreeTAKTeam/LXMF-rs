use crate::rnsh_parts::protocol::ErrorMessage;
use crate::rnsh_parts::{identity, session};
use crate::Cli;
use rns_transport::destination::{DestinationDesc, DestinationName, SingleInputDestination};
use rns_transport::hash::AddressHash;
use rns_transport::identity::{Identity, PrivateIdentity};
use rns_transport::iface::tcp_client::{TcpClient, TcpRuntimeStatusHandle};
use rns_transport::iface::tcp_server::TcpServer;
use rns_transport::iface::{IfaceRole, InterfaceMode};
use rns_transport::transport::{Transport, TransportConfig};
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, watch, Mutex};
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};

#[derive(Clone)]
pub(crate) struct Runtime {
    pub(crate) transport: Arc<Transport>,
    pub(crate) identity: PrivateIdentity,
    pub(crate) destination: Arc<Mutex<SingleInputDestination>>,
    pub(crate) timeout: Duration,
    pub(crate) no_auth: bool,
    pub(crate) no_id: bool,
    pub(crate) allowed: HashSet<AddressHash>,
    pub(crate) root: PathBuf,
    pub(crate) default_command: Vec<String>,
    pub(crate) no_remote_command: bool,
    pub(crate) remote_command_as_args: bool,
}

struct SessionTask {
    cancel: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl Runtime {
    pub(crate) async fn peer_identity(&self, link_id: AddressHash) -> Option<Identity> {
        self.transport.find_in_link(&link_id).await.and_then(|link| {
            link.try_lock().ok().and_then(|guard| guard.identified_peer_identity().copied())
        })
    }
}

pub(crate) async fn run(cli: &Cli) -> io::Result<i64> {
    validate(cli)?;
    let identity = identity::load(cli.identity.as_deref(), cli.identity_seed.as_deref())?;
    let mut config = TransportConfig::new("rnsh", &identity, true);
    let timeout_secs = cli.timeout.ceil() as u64;
    config.set_path_request_timeout_secs(timeout_secs);
    config.set_link_proof_timeout_secs(timeout_secs.max(30));
    config.set_resource_retry_interval_secs(1);
    let transport = Arc::new(Transport::new(config));
    let destination =
        transport.add_destination(identity.clone(), DestinationName::new_app("rnsh")).await;

    if cli.print_identity {
        println!("Identity     : {}", identity.address_hash().to_hex_string());
        println!("Listening on : {}", destination.lock().await.desc.address_hash.to_hex_string());
        return Ok(0);
    }

    let tcp_clients = spawn_interfaces(&transport, cli).await;
    wait_for_tcp_clients(&tcp_clients).await;
    let root = cli.root.as_deref().unwrap_or_else(|| std::path::Path::new(".")).canonicalize()?;
    let runtime = Runtime {
        transport,
        identity,
        destination,
        timeout: cli.timeout_duration()?,
        no_auth: cli.no_auth,
        no_id: cli.no_id,
        allowed: cli
            .allowed
            .iter()
            .map(|value| AddressHash::new_from_hex_string(value).map_err(|_| invalid_hash(value)))
            .collect::<io::Result<HashSet<_>>>()?,
        root,
        default_command: cli.default_command(),
        no_remote_command: cli.no_remote_command,
        remote_command_as_args: cli.remote_command_as_args,
    };

    if let Some(period) = cli.announce {
        announce(&runtime).await;
        if period > 0 && !cli.listen.is_empty() {
            let periodic_runtime = runtime.clone();
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(Duration::from_secs(period as u64));
                ticker.tick().await;
                loop {
                    ticker.tick().await;
                    announce(&periodic_runtime).await;
                }
            });
        }
    }

    if cli.listen.is_empty() {
        let destination = cli.destination.as_deref().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "rnsh initiator needs a destination")
        })?;
        let destination =
            AddressHash::new_from_hex_string(destination).map_err(|_| invalid_hash(destination))?;
        session::initiate(&runtime, destination, cli.command.clone()).await
    } else {
        serve(runtime).await.map(|()| 0)
    }
}

pub(crate) async fn wait_for_destination(
    runtime: &Runtime,
    target: AddressHash,
) -> io::Result<DestinationDesc> {
    let mut announces = runtime.transport.recv_announces().await;
    runtime.transport.request_path(&target, None, None).await;
    timeout(runtime.timeout, async {
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
                    return Err(io::Error::other("rnsh announce channel closed"));
                }
            }
        }
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "rnsh path discovery timed out"))?
}

pub(crate) async fn establish_link(
    runtime: &Runtime,
    description: DestinationDesc,
) -> io::Result<(Arc<Mutex<rns_transport::destination::link::Link>>, AddressHash)> {
    let mut events = runtime.transport.out_link_events();
    let link = runtime.transport.link(description).await;
    let link_id = *link.lock().await.id();
    timeout(runtime.timeout, async {
        loop {
            match events.recv().await {
                Ok(event)
                    if event.id == link_id
                        && matches!(
                            event.event,
                            rns_transport::destination::link::LinkEvent::Activated
                        ) =>
                {
                    return Ok(());
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(io::Error::other("rnsh link event channel closed"));
                }
            }
        }
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "rnsh link establishment timed out"))??;
    Ok((link, link_id))
}

pub(crate) async fn close_link(
    transport: &Transport,
    link: &Arc<Mutex<rns_transport::destination::link::Link>>,
) {
    let packet = {
        let mut guard = link.lock().await;
        (guard.status() != rns_transport::destination::link::LinkStatus::Closed)
            .then(|| guard.teardown())
            .flatten()
    };
    if let Some(packet) = packet {
        let _ = transport.send_link_packet_on_bound_iface(link, packet).await;
    }
}

async fn serve(runtime: Runtime) -> io::Result<()> {
    let mut events = runtime.transport.in_link_events();
    let mut sessions: HashMap<AddressHash, SessionTask> = HashMap::new();
    loop {
        tokio::select! {
            signal = tokio::signal::ctrl_c() => {
                signal.map_err(|error| io::Error::other(format!("could not install interrupt handler: {error}")))?;
                stop_sessions(&mut sessions).await;
                return Ok(());
            }
            event = events.recv() => match event {
                Ok(event) => {
                    match &event.event {
                        rns_transport::destination::link::LinkEvent::Activated if runtime.no_auth => {
                            spawn_session(&runtime, event.id, &mut sessions);
                        }
                        rns_transport::destination::link::LinkEvent::PeerIdentified(identity) => {
                            let accepted = runtime.no_auth || runtime.allowed.contains(&identity.address_hash);
                            if accepted {
                                spawn_session(&runtime, event.id, &mut sessions);
                            } else {
                                if !runtime.no_auth {
                                    log::warn!(
                                        "rnsh rejected unauthorised identity {} on link {}",
                                        identity.address_hash.to_hex_string(),
                                        event.id.to_hex_string()
                                    );
                                }
                                let channel = runtime.transport.channel(event.id);
                                if let Err(error) = channel
                                    .send_typed(&ErrorMessage::fatal("Identity not allowed"))
                                    .await
                                {
                                    log::warn!(
                                        "rnsh could not report rejected identity {} on link {}: {:?}",
                                        identity.address_hash.to_hex_string(),
                                        event.id.to_hex_string(),
                                        error
                                    );
                                }
                                close_inbound_link(&runtime.transport, event.id).await;
                            }
                        }
                        rns_transport::destination::link::LinkEvent::Closed => {
                            if let Some(session) = sessions.remove(&event.id) {
                                stop_session(session).await;
                            }
                        }
                        _ => {}
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => {
                    stop_sessions(&mut sessions).await;
                    return Err(io::Error::other("rnsh link event channel closed"));
                }
            }
        }
    }
}

fn spawn_session(
    runtime: &Runtime,
    link_id: AddressHash,
    sessions: &mut HashMap<AddressHash, SessionTask>,
) {
    if sessions.contains_key(&link_id) {
        return;
    }
    let (cancel, link_closed) = watch::channel(false);
    let runtime = runtime.clone();
    let task = tokio::spawn(async move {
        if let Err(error) = session::serve_link(runtime, link_id, link_closed).await {
            log::debug!("rnsh session {} ended: {}", link_id.to_hex_string(), error);
        }
    });
    sessions.insert(link_id, SessionTask { cancel, task });
}

async fn stop_session(session: SessionTask) {
    let _ = session.cancel.send(true);
    if let Err(error) = session.task.await {
        log::debug!("rnsh session task ended before join: {}", error);
    }
}

async fn stop_sessions(sessions: &mut HashMap<AddressHash, SessionTask>) {
    for (_, session) in sessions.drain() {
        stop_session(session).await;
    }
}

async fn close_inbound_link(transport: &Transport, link_id: AddressHash) {
    let Some(link) = transport.find_in_link(&link_id).await else { return };
    close_link(transport, &link).await;
}

async fn announce(runtime: &Runtime) {
    runtime.transport.send_announce(&runtime.destination, None).await;
}

async fn spawn_interfaces(transport: &Arc<Transport>, cli: &Cli) -> Vec<TcpRuntimeStatusHandle> {
    let manager = transport.iface_manager();
    let mut clients = Vec::with_capacity(cli.connect.len());
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
        let client = {
            let mut guard = manager.lock().await;
            let (_, client) = guard.spawn_as_with_mode_and_handle(
                TcpClient::new(connect.clone()),
                TcpClient::spawn,
                IfaceRole::default(),
                InterfaceMode::default(),
            );
            client
        };
        clients.push(client.lock().expect("TCP client mutex poisoned").runtime_status_handle());
    }
    clients
}

async fn wait_for_tcp_clients(clients: &[TcpRuntimeStatusHandle]) {
    if clients.is_empty() {
        return;
    }
    let _ = timeout(TcpClient::DEFAULT_CONNECT_TIMEOUT, async {
        loop {
            if clients
                .iter()
                .all(|client| client.to_json()["stream_state"].as_str() == Some("connected"))
            {
                return;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
}

fn validate(cli: &Cli) -> io::Result<()> {
    if cli.print_identity {
        return Ok(());
    }
    if cli.listen.is_empty() && cli.connect.is_empty() && !cli.print_identity {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "network rnsh needs --listen, --connect, or --print-identity",
        ));
    }
    if cli.destination.is_some() && cli.connect.is_empty() && cli.listen.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "rnsh initiator needs at least one --connect interface",
        ));
    }
    if cli.destination.is_none() && cli.connect.is_empty() && cli.listen.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "rnsh listener needs --listen"));
    }
    if let Some(root) = cli.root.as_deref() {
        if !root.is_dir() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "rnsh root is not a directory"));
        }
    }
    Ok(())
}

fn invalid_hash(value: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("invalid destination or identity hash: {value}"),
    )
}
