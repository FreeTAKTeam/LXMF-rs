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
    pub fn new(
        identity: rns_transport::identity::PrivateIdentity,
        timeout: Duration,
    ) -> Self {
        let mut config = rns_transport::transport::TransportConfig::new("rngit-client", &identity, true);
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
        let (link, link_id) = self.establish_link(description).await?;
        let result = self.request_on_link(&link, link_id, path, data).await;
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
    ) -> io::Result<(Arc<tokio::sync::Mutex<rns_transport::destination::link::Link>>, rns_transport::hash::AddressHash)> {
        let mut events = self.transport.out_link_events();
        let link = self.transport.link(description).await;
        let link_id = *link.lock().await.id();
        timeout(self.timeout, async {
            loop {
                match events.recv().await {
                    Ok(event)
                        if event.id == link_id
                            && matches!(event.event, rns_transport::destination::link::LinkEvent::Activated) =>
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
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "rngit link establishment timed out"))??;

        let identify = {
            let guard = link.lock().await;
            let payload = guard.identify_payload(&self.identity);
            guard
                .identify_packet(&payload)
                .map_err(|error| io::Error::other(format!("could not build rngit identity packet: {error:?}")))?
        };
        self.send_on_link(&link, identify).await?;
        Ok((link, link_id))
    }

    async fn request_on_link(
        &self,
        link: &Arc<tokio::sync::Mutex<rns_transport::destination::link::Link>>,
        link_id: rns_transport::hash::AddressHash,
        path: &str,
        data: rmpv::Value,
    ) -> io::Result<Vec<u8>> {
        let request = rns_transport::destination::link::Link::request_payload(path, data)
            .map_err(|error| io::Error::other(format!("could not build rngit request: {error:?}")))?;
        let mut data_events = self.transport.received_data_events();
        let mut resource_events = self.transport.resource_events();

        let (request_id, packet) = {
            let guard = link.lock().await;
            if request.packed.len() <= guard.link_mdu() {
                let packet = guard
                    .request_packet(&request.packed)
                    .map_err(|error| io::Error::other(format!("could not build rngit request packet: {error:?}")))?;
                let hash = packet.hash().to_bytes();
                (hash[..rns_transport::hash::ADDRESS_HASH_SIZE].to_vec(), Some(packet))
            } else {
                (request.resource_request_id.to_vec(), None)
            }
        };

        if let Some(packet) = packet {
            self.send_on_link(link, packet).await?;
        } else {
            self.transport
                .send_request_resource(&link_id, request_id.clone(), request.packed, None)
                .await
                .map_err(|error| io::Error::other(format!("could not send rngit request Resource: {error:?}")))?;
        }

        timeout(self.timeout, async {
            loop {
                tokio::select! {
                    result = data_events.recv() => match result {
                        Ok(event) if event.destination == link_id && event.context == Some(rns_transport::packet::PacketContext::Response) => {
                            if let Ok((received_id, response)) = rns_transport::destination::link::unpack_response_envelope(event.data.as_slice()) {
                                if received_id.as_slice() == request_id.as_slice() {
                                    return decode_response_value(response);
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(broadcast::error::RecvError::Lagged(_)) => {}
                        Err(broadcast::error::RecvError::Closed) => return Err(io::Error::other("rngit response channel closed")),
                    },
                    result = resource_events.recv() => match result {
                        Ok(event) if event.link_id == link_id => {
                            if let rns_transport::resource::ResourceEventKind::Complete(complete) = event.kind {
                                if complete.is_response && complete.request_id.as_deref() == Some(request_id.as_slice()) {
                                    match rns_transport::destination::link::unpack_response_envelope(&complete.data) {
                                        Ok((received_id, response)) if received_id.as_slice() == request_id.as_slice() => {
                                            return decode_response_value(response);
                                        }
                                        Ok(_) if matches!(path, "/git/fetch" | "/mgmt/release") => {
                                            return Ok(python_resource_response(complete.data));
                                        }
                                        Err(_) if matches!(path, "/git/fetch" | "/mgmt/release") => {
                                            return Ok(python_resource_response(complete.data));
                                        }
                                        Ok(_) => {}
                                        Err(error) => {
                                            return Err(io::Error::other(format!(
                                                "invalid rngit Resource response: {error:?}"
                                            )))
                                        }
                                    }
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(broadcast::error::RecvError::Lagged(_)) => {}
                        Err(broadcast::error::RecvError::Closed) => return Err(io::Error::other("rngit Resource channel closed")),
                    },
                }
            }
        })
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "rngit request timed out"))?
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
        let packet = {
            let mut guard = link.lock().await;
            (guard.status() != rns_transport::destination::link::LinkStatus::Closed)
                .then(|| guard.teardown())
                .flatten()
        };
        if let Some(packet) = packet {
            let _ = self.send_on_link(link, packet).await;
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
