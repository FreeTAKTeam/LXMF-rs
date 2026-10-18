/// Native Rust request client for the pinned Python `RNS.Utilities.rngit`
/// service. The compatibility client above remains transport-neutral for
/// local semantic tests; this type owns the real Reticulum path, Link,
/// identity, request-id, and Resource handling needed by mixed-language use.
use tokio::sync::broadcast;
use tokio::time::timeout;

pub struct NativeRngitClient {
    transport: Arc<rns_transport::transport::Transport>,
    identity: rns_transport::identity::PrivateIdentity,
    timeout: Duration,
}

impl NativeRngitClient {
    pub fn new(identity: rns_transport::identity::PrivateIdentity, timeout: Duration) -> Self {
        let mut config =
            rns_transport::transport::TransportConfig::new("rngit-client", &identity, true);
        config.set_path_request_timeout_secs(timeout.as_secs().max(1));
        config.set_link_proof_timeout_secs(timeout.as_secs().max(1));
        config.set_resource_retry_interval_secs(1);
        Self {
            transport: Arc::new(rns_transport::transport::Transport::new(config)),
            identity,
            timeout,
        }
    }

    /// Attach the native TCP interface used by the command-line utility and
    /// by the mixed Python/Rust interoperability trace.
    pub async fn add_tcp_client(&self, endpoint: String) {
        let manager = self.transport.iface_manager();
        let mut guard = manager.lock().await;
        guard.spawn_as_with_mode(
            rns_transport::iface::tcp_client::TcpClient::new(endpoint),
            rns_transport::iface::tcp_client::TcpClient::spawn,
            rns_transport::iface::IfaceRole::default(),
            rns_transport::iface::InterfaceMode::default(),
        );
    }

    pub fn transport(&self) -> Arc<rns_transport::transport::Transport> {
        self.transport.clone()
    }

    /// Execute one Python-compatible `RNS.Link.request` against the Git
    /// destination. Small requests use a Request packet; larger requests use
    /// a request Resource with `truncated_hash(packed_request)`, exactly as
    /// the Python reference does.
    pub async fn request(
        &self,
        destination: [u8; 16],
        path: &str,
        data: rmpv::Value,
    ) -> io::Result<Vec<u8>> {
        let target = rns_transport::hash::AddressHash::new(destination);
        let description = self.wait_for_destination(target).await?;
        let (link, _) = self.establish_link(description).await?;
        let result = self.request_on_link(&link, path, data).await;
        self.close_link(&link).await;
        result
    }

    async fn wait_for_destination(
        &self,
        target: rns_transport::hash::AddressHash,
    ) -> io::Result<rns_transport::destination::DestinationDesc> {
        let mut announces = self.transport.recv_announces().await;
        self.transport.request_path(&target, None, None).await;
        timeout(self.timeout, async {
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
                        return Err(io::Error::other("rngit announce channel closed"));
                    }
                }
            }
        })
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "rngit path discovery timed out"))?
    }

    async fn establish_link(
        &self,
        description: rns_transport::destination::DestinationDesc,
    ) -> io::Result<(
        Arc<tokio::sync::Mutex<rns_transport::destination::link::Link>>,
        rns_transport::hash::AddressHash,
    )> {
        let mut events = self.transport.out_link_events();
        let link = self.transport.link(description).await;
        let link_id = *link.lock().await.id();
        timeout(self.timeout, async {
            loop {
                match events.recv().await {
                    Ok(event)
                        if event.id == link_id
                            && matches!(
                                event.event,
                                rns_transport::destination::link::LinkEvent::Activated
                            ) =>
                    {
                        break;
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => {
                        return Err(io::Error::other("rngit link event channel closed"));
                    }
                }
            }
            Ok(())
        })
        .await
        .map_err(|_| {
            io::Error::new(io::ErrorKind::TimedOut, "rngit link establishment timed out")
        })??;

        let identify = {
            let guard = link.lock().await;
            let payload = guard.identify_payload(&self.identity);
            guard.identify_packet(&payload).map_err(|error| {
                io::Error::other(format!("could not build rngit identity packet: {error:?}"))
            })?
        };
        self.send_on_link(&link, identify).await?;
        Ok((link, link_id))
    }

    async fn request_on_link(
        &self,
        link: &Arc<tokio::sync::Mutex<rns_transport::destination::link::Link>>,
        path: &str,
        data: rmpv::Value,
    ) -> io::Result<Vec<u8>> {
        let request = rns_transport::destination::link::Link::request_payload(path, data).map_err(
            |error| io::Error::other(format!("could not build rngit request: {error:?}")),
        )?;
        let mut receipt =
            self.transport.send_request_on_link(link, request, self.timeout, None).await.map_err(
                |error| io::Error::other(format!("could not send rngit request: {error:?}")),
            )?;
        let response = match timeout(self.timeout, receipt.wait()).await {
            Ok(result) => result.map_err(|error| {
                if error == rns_transport::destination::link::RequestFailure::Timeout {
                    io::Error::new(io::ErrorKind::TimedOut, "rngit request timed out")
                } else {
                    io::Error::other(format!("rngit request failed: {error}"))
                }
            })?,
            Err(_) => {
                let (committed, commit) = tokio::sync::oneshot::channel();
                tokio::select! {
                    _ = self.transport.cancel_request_on_link_observed(link, &receipt, |_| {
                        // Local latch only; optional I/O may be abandoned after cleanup.
                        if committed.send(()).is_err() {
                        log::trace!("rngit native cleanup observer was dropped");
                    }
                    }) => {},
                    _ = commit => {},
                }
                return Err(io::Error::new(io::ErrorKind::TimedOut, "rngit request timed out"));
            }
        };
        match rns_transport::destination::link::unpack_response_envelope(&response.data) {
            Ok((id, value)) if id.as_slice() == receipt.request_id() => {
                decode_response_value(value)
            }
            _ if response.is_resource
                && response.metadata.is_some()
                && matches!(path, "/git/fetch" | "/mgmt/release") =>
            {
                Ok(python_resource_response(response.data.clone()))
            }
            _ => Err(io::Error::new(io::ErrorKind::InvalidData, "invalid rngit response envelope")),
        }
    }

    async fn send_on_link(
        &self,
        link: &Arc<tokio::sync::Mutex<rns_transport::destination::link::Link>>,
        packet: rns_transport::packet::Packet,
    ) -> io::Result<()> {
        let outcome = self.transport.send_link_packet_on_bound_iface(link, packet).await;
        if matches!(
            outcome,
            rns_transport::transport::SendPacketOutcome::SentDirect
                | rns_transport::transport::SendPacketOutcome::SentBroadcast
        ) {
            Ok(())
        } else {
            Err(io::Error::new(io::ErrorKind::NotConnected, "rngit link has no usable interface"))
        }
    }

    async fn close_link(
        &self,
        link: &Arc<tokio::sync::Mutex<rns_transport::destination::link::Link>>,
    ) {
        let (committed, commit) = tokio::sync::oneshot::channel();
        tokio::select! {
            result = self.transport.teardown_link_observed(link, || {
                // Local latch only; optional I/O may be abandoned after cleanup.
                if committed.send(()).is_err() {
                        log::trace!("rngit native cleanup observer was dropped");
                    }
            }) => {
                if let Err(error) = result {
                    log::debug!("rngit native Link close frame handoff failed: {error}");
                }
            },
            _ = commit => {},
        }
    }
}

fn decode_response_value(value: rmpv::Value) -> io::Result<Vec<u8>> {
    value
        .as_slice()
        .map(ToOwned::to_owned)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "rngit response was not binary"))
}

fn python_resource_response(mut data: Vec<u8>) -> Vec<u8> {
    // Python returns successful Git bundles and release artifacts as raw
    // Resource bodies and carries their success metadata separately. The Rust
    // client API exposes the common rngit response shape, so restore the zero
    // status byte at this compatibility boundary.
    data.insert(0, 0);
    data
}
