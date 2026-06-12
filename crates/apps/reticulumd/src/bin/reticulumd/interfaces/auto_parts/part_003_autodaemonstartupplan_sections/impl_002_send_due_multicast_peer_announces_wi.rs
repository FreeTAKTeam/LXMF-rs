impl AutoDaemonStartupPlan {

    async fn send_due_multicast_peer_announces_with_runtime_socket(
        &self,
        state: Arc<tokio::sync::Mutex<AutoDiscoveryState>>,
        socket: Arc<tokio::net::UdpSocket>,
        now: core::time::Duration,
    ) -> Result<usize, String> {
        let datagrams = {
            let mut state = state.lock().await;
            self.due_multicast_peer_announce_datagrams(&mut state, now)
        };
        if datagrams.is_empty() {
            return Ok(0);
        }
        let resolver = AutoInterfaceIndexResolver::from_system()?;
        self.send_peer_announce_datagrams_with_udp_socket(
            &datagrams,
            "auto multicast peer announce",
            &socket,
            |ifname| resolver.resolve(ifname),
        )
        .await
    }

    fn spawn_repeat_peer_announce_scheduler(
        &self,
        state: Arc<tokio::sync::Mutex<AutoDiscoveryState>>,
        socket: Arc<tokio::net::UdpSocket>,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> tokio::task::JoinHandle<()> {
        let plan = self.clone();
        let timing = AutoInterfaceTiming::for_platform(self.platform);
        tokio::spawn(async move {
            if *shutdown.borrow() {
                return;
            }
            let started_at = Instant::now();
            let mut interval = tokio::time::interval(timing.announce_interval);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            break;
                        }
                    }
                    _ = interval.tick() => {
                        match plan
                            .send_due_multicast_peer_announces_with_runtime_socket(
                                Arc::clone(&state),
                                Arc::clone(&socket),
                                started_at.elapsed(),
                            )
                            .await
                        {
                            Ok(sent) if sent > 0 => {
                                log::debug!("[daemon-auto] repeat peer-announce scheduler sent {sent} packet(s)");
                            }
                            Ok(_) => {}
                            Err(err) => {
                                log::warn!("[daemon-auto] repeat peer-announce scheduler failed: {err}");
                            }
                        }
                    }
                }
            }
        })
    }

    async fn send_due_peer_job_with_runtime_socket(
        &self,
        state: Arc<tokio::sync::Mutex<AutoDiscoveryState>>,
        socket: Arc<tokio::net::UdpSocket>,
        now: core::time::Duration,
    ) -> Result<AutoPeerJobRuntimeSummary, String> {
        let (summary, datagrams) = {
            let mut state = state.lock().await;
            self.run_peer_job_datagrams(&mut state, now)
        };
        if datagrams.is_empty() {
            return Ok(summary);
        }
        let resolver = AutoInterfaceIndexResolver::from_system()?;
        self.send_peer_announce_datagrams_with_udp_socket(
            &datagrams,
            "auto reverse peer announce",
            &socket,
            |ifname| resolver.resolve(ifname),
        )
        .await?;
        Ok(summary)
    }

    fn spawn_peer_job_scheduler(
        &self,
        state: Arc<tokio::sync::Mutex<AutoDiscoveryState>>,
        socket: Arc<tokio::net::UdpSocket>,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> tokio::task::JoinHandle<()> {
        let plan = self.clone();
        let timing = AutoInterfaceTiming::for_platform(self.platform);
        tokio::spawn(async move {
            if *shutdown.borrow() {
                return;
            }
            let started_at = Instant::now();
            let mut interval = tokio::time::interval(timing.peer_job_interval);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            break;
                        }
                    }
                    _ = interval.tick() => {
                        match plan
                            .send_due_peer_job_with_runtime_socket(
                                Arc::clone(&state),
                                Arc::clone(&socket),
                                started_at.elapsed(),
                            )
                            .await
                        {
                            Ok(summary)
                                if summary.expired_peer_count > 0
                                    || summary.reverse_peer_announce_count > 0
                                    || summary.carrier_event_count > 0 =>
                            {
                                log::debug!(
                                    "[daemon-auto] peer-job scheduler expired={} reverse_announces={} missing_initial_echoes={} carrier_events={}",
                                    summary.expired_peer_count,
                                    summary.reverse_peer_announce_count,
                                    summary.missing_initial_echo_count,
                                    summary.carrier_event_count
                                );
                            }
                            Ok(_) => {}
                            Err(err) => {
                                log::warn!("[daemon-auto] peer-job scheduler failed: {err}");
                            }
                        }
                    }
                }
            }
        })
    }

    // Binds only the unicast side of discovery; startup combines these sockets
    // with multicast sockets before spawning receive loops.
    #[allow(dead_code)]
    pub(crate) async fn bind_unicast_discovery_sockets(
        &self,
        mut scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<Vec<AutoBoundDiscoverySocket>, String> {
        let mut sockets = Vec::new();
        for target in self
            .discovery_socket_bind_targets()
            .into_iter()
            .filter(|target| target.kind == AutoDiscoverySocketKind::Unicast)
        {
            let bind_addr = target.resolve_bind_addr(&mut scope_id_for_ifname).map_err(|err| {
                format!(
                    "resolve auto discovery unicast bind {} failed: {err}",
                    target.display_bind_addr()
                )
            })?;
            let socket = tokio::net::UdpSocket::bind(bind_addr).await.map_err(|err| {
                format!(
                    "bind auto discovery unicast socket {} failed: {err}",
                    target.display_bind_addr()
                )
            })?;
            sockets.push(AutoBoundDiscoverySocket {
                kind: target.kind,
                ifname: target.ifname,
                bind_addr: socket.local_addr().unwrap_or(bind_addr),
                multicast_group_addr: None,
                socket,
            });
        }
        Ok(sockets)
    }

    #[allow(dead_code)]
    pub(crate) async fn bind_data_sockets(
        &self,
        mut scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<Vec<AutoBoundDataSocket>, String> {
        let mut sockets = Vec::new();
        for target in self.data_socket_bind_targets() {
            let bind_addr = target.resolve_bind_addr(&mut scope_id_for_ifname).map_err(|err| {
                format!("resolve auto peer data bind {} failed: {err}", target.display_bind_addr())
            })?;
            let socket = tokio::net::UdpSocket::bind(bind_addr).await.map_err(|err| {
                format!("bind auto peer data socket {} failed: {err}", target.display_bind_addr())
            })?;
            sockets.push(AutoBoundDataSocket {
                ifname: target.ifname,
                bind_addr: socket.local_addr().unwrap_or(bind_addr),
                socket: Arc::new(socket),
            });
        }
        Ok(sockets)
    }

    // Binds and joins only the multicast side of discovery; startup combines
    // these sockets with unicast sockets before spawning receive loops.
    #[allow(dead_code)]
    pub(crate) async fn bind_multicast_discovery_sockets(
        &self,
        mut scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<Vec<AutoBoundDiscoverySocket>, String> {
        let mut sockets = Vec::new();
        for target in self
            .discovery_socket_bind_targets()
            .into_iter()
            .filter(|target| target.kind == AutoDiscoverySocketKind::Multicast)
        {
            let resolved =
                target.resolve_multicast_bind(&mut scope_id_for_ifname).map_err(|err| {
                    format!(
                        "resolve auto discovery multicast bind {} failed: {err}",
                        target.display_bind_addr()
                    )
                })?;
            let std_socket = std::net::UdpSocket::bind(resolved.bind_addr).map_err(|err| {
                format!(
                    "bind auto discovery multicast socket {} failed: {err}",
                    target.display_bind_addr()
                )
            })?;
            match resolved.multicast_group_addr.ip() {
                IpAddr::V6(group) => std_socket
                    .join_multicast_v6(&group, resolved.multicast_scope_id)
                    .map_err(|err| {
                        format!(
                            "join auto discovery multicast group {} on ifindex {} failed: {err}",
                            resolved.multicast_group_addr, resolved.multicast_scope_id
                        )
                    })?,
                IpAddr::V4(group) => std_socket
                    .join_multicast_v4(&group, &std::net::Ipv4Addr::UNSPECIFIED)
                    .map_err(|err| {
                        format!(
                            "join auto discovery multicast group {} failed: {err}",
                            resolved.multicast_group_addr
                        )
                    })?,
            }
            std_socket.set_nonblocking(true).map_err(|err| {
                format!("set auto discovery multicast socket nonblocking failed: {err}")
            })?;
            let socket = tokio::net::UdpSocket::from_std(std_socket).map_err(|err| {
                format!("convert auto discovery multicast socket to tokio failed: {err}")
            })?;
            sockets.push(AutoBoundDiscoverySocket {
                kind: target.kind,
                ifname: target.ifname,
                bind_addr: socket.local_addr().unwrap_or(resolved.bind_addr),
                multicast_group_addr: Some(resolved.multicast_group_addr),
                socket,
            });
        }
        Ok(sockets)
    }

    #[allow(dead_code)]
    pub(crate) fn spawn_discovery_receive_loops(
        &self,
        sockets: Vec<AutoBoundDiscoverySocket>,
        state: Arc<tokio::sync::Mutex<AutoDiscoveryState>>,
        events: tokio::sync::mpsc::Sender<AutoDiscoveryLoopEvent>,
        shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> Vec<tokio::task::JoinHandle<()>> {
        sockets
            .into_iter()
            .map(|socket| {
                self.spawn_discovery_receive_loop(
                    socket,
                    Arc::clone(&state),
                    events.clone(),
                    shutdown.clone(),
                )
            })
            .collect()
    }

    #[allow(dead_code)]
    fn spawn_discovery_receive_loop(
        &self,
        socket: AutoBoundDiscoverySocket,
        state: Arc<tokio::sync::Mutex<AutoDiscoveryState>>,
        events: tokio::sync::mpsc::Sender<AutoDiscoveryLoopEvent>,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> tokio::task::JoinHandle<()> {
        let group_id = self.config.group_id.clone();
        tokio::spawn(async move {
            if *shutdown.borrow() {
                return;
            }
            let started_at = Instant::now();
            loop {
                tokio::select! {
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            break;
                        }
                    }
                    received = socket.recv_discovery_datagram() => {
                        let datagram = match received {
                            Ok(datagram) => datagram,
                            Err(error) => {
                                let _ = events
                                    .send(AutoDiscoveryLoopEvent::ReceiveFailed {
                                        ifname: socket.ifname.clone(),
                                        kind: socket.kind,
                                        bind_addr: socket.bind_addr,
                                        error,
                                    })
                                    .await;
                                break;
                            }
                        };
                        let source_address = discovery_source_address(&datagram);
                        let event = {
                            let mut state = state.lock().await;
                            state.observe_authenticated_discovery_packet(
                                &datagram.payload,
                                group_id.as_bytes(),
                                &source_address,
                                &datagram.ifname,
                                started_at.elapsed(),
                            )
                        };
                        let loop_event = match event {
                            Ok(event) => AutoDiscoveryLoopEvent::Processed(
                                AutoProcessedDiscoveryDatagram {
                                    datagram,
                                    source_address,
                                    event,
                                },
                            ),
                            Err(reason) => AutoDiscoveryLoopEvent::Rejected {
                                datagram,
                                source_address,
                                reason,
                            },
                        };
                        if events.send(loop_event).await.is_err() {
                            break;
                        }
                    }
                }
            }
        })
    }

    #[allow(dead_code)]
    pub(crate) fn spawn_peer_data_receive_loops(
        &self,
        sockets: Vec<AutoBoundDataSocket>,
        state: Arc<tokio::sync::Mutex<AutoDiscoveryState>>,
        dedupe: Arc<tokio::sync::Mutex<AutoInboundPacketDeduplicator>>,
        transport: Option<AutoInterfaceTransportBridge>,
        events: tokio::sync::mpsc::Sender<AutoPeerDataLoopEvent>,
        shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> Vec<tokio::task::JoinHandle<()>> {
        sockets
            .into_iter()
            .map(|socket| {
                self.spawn_peer_data_receive_loop(
                    socket,
                    Arc::clone(&state),
                    Arc::clone(&dedupe),
                    transport.clone(),
                    events.clone(),
                    shutdown.clone(),
                )
            })
            .collect()
    }
}
