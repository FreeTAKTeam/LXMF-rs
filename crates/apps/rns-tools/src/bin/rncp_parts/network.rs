use super::{client, identity, protocol, server};
use crate::Cli;
use rns_transport::destination::{DestinationName, SingleInputDestination};
use rns_transport::hash::AddressHash;
use rns_transport::identity::PrivateIdentity;
use rns_transport::iface::tcp_client::{TcpClient, TcpRuntimeStatusHandle};
use rns_transport::iface::tcp_server::TcpServer;
use rns_transport::iface::{IfaceRole, InterfaceMode};
use rns_transport::transport::{Transport, TransportConfig};
use std::collections::HashSet;
use std::fs;
use std::future::Future;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

pub(crate) struct Runtime {
    pub(crate) transport: Arc<Transport>,
    pub(crate) identity: PrivateIdentity,
    pub(crate) destination: Arc<Mutex<SingleInputDestination>>,
    pub(crate) timeout: Duration,
    pub(crate) no_auth: bool,
    pub(crate) no_compress: bool,
    pub(crate) allowed: HashSet<AddressHash>,
    pub(crate) allow_fetch: bool,
    pub(crate) jail: Option<PathBuf>,
    pub(crate) save: Option<PathBuf>,
    pub(crate) overwrite: bool,
    pub(crate) silent: bool,
}

pub(crate) async fn run(cli: &Cli) -> io::Result<()> {
    validate(cli)?;
    let identity = identity::load(cli)?;
    let destination_name = DestinationName::new("rncp", "receive");
    let mut config = TransportConfig::new("rncp", &identity, true);
    config.set_path_request_timeout_secs(cli.timeout);
    config.set_link_proof_timeout_secs(cli.timeout.max(30));
    config.set_resource_retry_interval_secs(1);
    let transport = Arc::new(Transport::new(config));
    let destination = transport.add_destination(identity.clone(), destination_name).await;

    if cli.print_identity {
        let destination_hash = destination.lock().await.desc.address_hash;
        println!("Identity     : {}", identity.address_hash().to_hex_string());
        println!("Listening on : {}", destination_hash.to_hex_string());
        return Ok(());
    }

    let tcp_clients = spawn_interfaces(&transport, cli).await;
    run_with_cancellation(wait_for_tcp_clients(&tcp_clients)).await?;

    let runtime = Runtime {
        transport,
        identity,
        destination,
        timeout: Duration::from_secs(cli.timeout),
        no_auth: cli.no_auth,
        no_compress: cli.no_compress,
        allowed: cli
            .allowed_identity
            .iter()
            .map(|value| protocol::parse_address(value))
            .collect::<io::Result<HashSet<_>>>()?,
        allow_fetch: cli.allow_fetch,
        jail: cli.jail.clone(),
        save: cli.save.clone(),
        overwrite: cli.overwrite,
        silent: cli.silent,
    };

    if cli.listen.is_empty() && cli.source.is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "network mode needs --listen, a source file, or --print-identity",
        ));
    }
    if cli.fetch {
        run_with_cancellation(client::fetch(&runtime, cli.source()?, cli.destination()?)).await
    } else if cli.source.is_some() || cli.destination.is_some() {
        run_with_cancellation(client::send(&runtime, cli.source()?, cli.destination()?)).await
    } else {
        server::serve(runtime).await
    }
}

async fn run_with_cancellation<F>(operation: F) -> io::Result<()>
where
    F: Future<Output = io::Result<()>>,
{
    tokio::select! {
        result = operation => result,
        signal = tokio::signal::ctrl_c() => {
            signal.map_err(|error| io::Error::other(format!("could not install interrupt handler: {error}")))?;
            Err(io::Error::new(io::ErrorKind::Interrupted, "operation cancelled by user"))
        }
    }
}

fn validate(cli: &Cli) -> io::Result<()> {
    if cli.connect.is_empty() && cli.listen.is_empty() && !cli.print_identity {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "at least one --listen or --connect interface is required",
        ));
    }
    if cli.fetch && cli.source.is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "--fetch needs a remote file path",
        ));
    }
    if cli.allow_fetch && cli.listen.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "--allow-fetch needs --listen"));
    }
    if cli.jail.is_some() && !cli.allow_fetch {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "--jail needs --allow-fetch"));
    }
    if let Some(save) = cli.save.as_deref() {
        if !save.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "save directory is not a directory",
            ));
        }
    }
    Ok(())
}

async fn spawn_interfaces(transport: &Arc<Transport>, cli: &Cli) -> Vec<TcpRuntimeStatusHandle> {
    let manager = transport.iface_manager();
    let mut tcp_clients = Vec::with_capacity(cli.connect.len());
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
        tcp_clients.push(client.lock().expect("TCP client mutex poisoned").runtime_status_handle());
    }
    tcp_clients
}

async fn wait_for_tcp_clients(clients: &[TcpRuntimeStatusHandle]) -> io::Result<()> {
    if clients.is_empty() {
        return Ok(());
    }
    let _ = tokio::time::timeout(TcpClient::DEFAULT_CONNECT_TIMEOUT, async {
        loop {
            if clients
                .iter()
                .all(|client| client.to_json()["stream_state"].as_str() == Some("connected"))
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    Ok(())
}

pub(crate) async fn operation_timeout(runtime: &Runtime) -> Duration {
    runtime.timeout.max(runtime.transport.medium_path_timeout().await)
}

pub(crate) async fn announce(runtime: &Runtime) {
    runtime.transport.send_announce(&runtime.destination, None).await;
}

pub(crate) fn read_file(path: &std::path::Path) -> io::Result<Vec<u8>> {
    if !path.is_file() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "source file was not found"));
    }
    fs::read(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn local_and_network_cli_shapes_remain_distinct() {
        let local = Cli::try_parse_from(["rncp", "source", "destination"]).expect("local cli");
        assert!(!local.network_mode());
        let network =
            Cli::try_parse_from(["rncp", "--listen", "127.0.0.1:4243"]).expect("network cli");
        assert!(network.network_mode());
    }
}
