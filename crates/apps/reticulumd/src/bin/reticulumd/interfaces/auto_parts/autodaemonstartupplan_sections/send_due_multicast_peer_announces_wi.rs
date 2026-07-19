#[derive(Clone)]
struct AutoInterfaceRuntimeLoopHandles {
    discovery_supervisor: Arc<tokio::sync::Mutex<AutoDiscoveryListenerSupervisor>>,
    data_supervisor: Arc<tokio::sync::Mutex<AutoPeerDataListenerSupervisor>>,
    discovery_events: tokio::sync::mpsc::Sender<AutoDiscoveryLoopEvent>,
    data_events: tokio::sync::mpsc::Sender<AutoPeerDataLoopEvent>,
}

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
        runtime_status: Option<&AutoRuntimeStatusHandle>,
        now: core::time::Duration,
    ) -> Result<AutoPeerJobRuntimeSummary, String> {
        let (summary, datagrams) = {
            let mut state = state.lock().await;
            self.run_peer_job_datagrams(&mut state, now)
        };
        if datagrams.is_empty() {
            if let Some(runtime_status) = runtime_status {
                runtime_status.record_peer_job_summary(&summary);
            }
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
        if let Some(runtime_status) = runtime_status {
            runtime_status.record_peer_job_summary(&summary);
        }
        Ok(summary)
    }

    fn spawn_peer_job_scheduler(
        &self,
        state: Arc<tokio::sync::Mutex<AutoDiscoveryState>>,
        socket: Arc<tokio::net::UdpSocket>,
        runtime_status: Option<AutoRuntimeStatusHandle>,
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
                                runtime_status.as_ref(),
                                started_at.elapsed(),
                            )
                            .await
                        {
                            Ok(summary)
                                if summary.expired_peer_count > 0
                                    || summary.reverse_peer_announce_count > 0
                                    || summary.carrier_changed =>
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

    async fn detect_link_local_updates_from_candidates(
        &self,
        state: Arc<tokio::sync::Mutex<AutoDiscoveryState>>,
        candidates: Vec<AutoInterfaceDeviceCandidate>,
    ) -> Vec<AutoLinkLocalAddressUpdate> {
        let allowed = state
            .lock()
            .await
            .adopted_devices()
            .into_iter()
            .map(|device| device.ifname)
            .collect::<Vec<_>>();
        let filter = AutoInterfaceDeviceFilter { allowed, ignored: Vec::new() };
        let adopted = filter.adopt_devices(&candidates, self.platform);
        let mut updates = Vec::new();
        let state = state.lock().await;
        for device in adopted {
            if let Some(update) = state.plan_adopted_link_local_address_update(
                &self.config,
                &device.ifname,
                &device.link_local_address,
            ) {
                updates.push(update);
            }
        }
        updates
    }

    async fn reconcile_link_local_addresses(
        &self,
        state: Arc<tokio::sync::Mutex<AutoDiscoveryState>>,
        supervisor: Arc<tokio::sync::Mutex<AutoPeerDataListenerSupervisor>>,
        runtime_status: Option<&AutoRuntimeStatusHandle>,
        events: &tokio::sync::mpsc::Sender<AutoPeerDataLoopEvent>,
        candidates: Vec<AutoInterfaceDeviceCandidate>,
        mut scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<usize, String> {
        let updates =
            self.detect_link_local_updates_from_candidates(Arc::clone(&state), candidates).await;
        let mut restarted = 0;
        for update in updates {
            supervisor
                .lock()
                .await
                .restart_link_local_listener(&update, None, events, &mut scope_id_for_ifname)
                .await?;
            state.lock().await.apply_adopted_link_local_address_update(&update);
            if let Some(runtime_status) = runtime_status {
                runtime_status.record_link_local_update(Some(&update));
            }
            restarted += 1;
        }
        Ok(restarted)
    }

    async fn reconcile_adopted_interface_add_remove(
        &self,
        state: Arc<tokio::sync::Mutex<AutoDiscoveryState>>,
        runtime: &AutoInterfaceRuntimeLoopHandles,
        candidates: Vec<AutoInterfaceDeviceCandidate>,
        runtime_status: Option<&AutoRuntimeStatusHandle>,
        now: core::time::Duration,
        mut scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<usize, String> {
        let desired = self.device_filter.adopt_devices(&candidates, self.platform);
        let changes = state.lock().await.plan_adopted_interface_changes(
            &self.config,
            self.platform,
            &desired,
        );
        let mut applied = 0;
        for change in changes {
            match &change {
                AutoAdoptedInterfaceChange::Added {
                    adopted,
                    discovery_listener,
                    data_listener,
                    ..
                } => {
                    let discovery_sockets = self
                        .bind_discovery_sockets_for_listener(
                            discovery_listener,
                            &mut scope_id_for_ifname,
                        )
                        .await?;
                    let data_socket = self
                        .bind_data_socket_for_listener(data_listener, &mut scope_id_for_ifname)
                        .await?;
                    runtime.discovery_supervisor.lock().await.spawn_bound_listener(
                        adopted.ifname.clone(),
                        discovery_sockets,
                        &runtime.discovery_events,
                    );
                    runtime
                        .data_supervisor
                        .lock()
                        .await
                        .spawn_bound_socket(data_socket, &runtime.data_events);
                    state.lock().await.apply_adopted_interface_change_at(&change, now);
                    if let Some(runtime_status) = runtime_status {
                        runtime_status.record_adopted_interface_change(&change);
                    }
                    applied += 1;
                }
                AutoAdoptedInterfaceChange::Removed { adopted, .. } => {
                    runtime
                        .discovery_supervisor
                        .lock()
                        .await
                        .remove_listener(&adopted.ifname)
                        .await;
                    runtime
                        .data_supervisor
                        .lock()
                        .await
                        .remove_listener(&adopted.ifname)
                        .await;
                    state.lock().await.apply_adopted_interface_change_at(&change, now);
                    if let Some(runtime_status) = runtime_status {
                        runtime_status.record_adopted_interface_change(&change);
                    }
                    applied += 1;
                }
                AutoAdoptedInterfaceChange::LinkLocalChanged(_) => {}
            }
        }
        Ok(applied)
    }

    fn spawn_link_local_address_reconciler(
        &self,
        state: Arc<tokio::sync::Mutex<AutoDiscoveryState>>,
        runtime: AutoInterfaceRuntimeLoopHandles,
        runtime_status: Option<AutoRuntimeStatusHandle>,
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
                        let candidates = match enumerate_link_local_candidates() {
                            Ok(candidates) => candidates,
                            Err(err) => {
                                log::warn!("[daemon-auto] link-local reconciler failed to enumerate interfaces: {err}");
                                continue;
                            }
                        };
                        let resolver = match AutoInterfaceIndexResolver::from_system() {
                            Ok(resolver) => resolver,
                            Err(err) => {
                                log::warn!("[daemon-auto] link-local reconciler failed to resolve interface indexes: {err}");
                                continue;
                            }
                        };
                        match plan
                            .reconcile_adopted_interface_add_remove(
                                Arc::clone(&state),
                                &runtime,
                                candidates.clone(),
                                runtime_status.as_ref(),
                                started_at.elapsed(),
                                |ifname| resolver.resolve(ifname),
                            )
                            .await
                        {
                            Ok(applied) if applied > 0 => {
                                log::debug!("[daemon-auto] adopted-interface reconciler applied {applied} add/remove change(s)");
                            }
                            Ok(_) => {}
                            Err(err) => {
                                log::warn!("[daemon-auto] adopted-interface reconciler failed: {err}");
                                continue;
                            }
                        }
                        match plan
                            .reconcile_link_local_addresses(
                                Arc::clone(&state),
                                Arc::clone(&runtime.data_supervisor),
                                runtime_status.as_ref(),
                                &runtime.data_events,
                                candidates,
                                |ifname| resolver.resolve(ifname),
                            )
                            .await
                        {
                            Ok(restarted) if restarted > 0 => {
                                log::debug!("[daemon-auto] link-local reconciler restarted {restarted} peer data listener(s)");
                            }
                            Ok(_) => {}
                            Err(err) => {
                                log::warn!("[daemon-auto] link-local reconciler failed: {err}");
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
            sockets.push(self.bind_discovery_socket_target(target, &mut scope_id_for_ifname).await?);
        }
        Ok(sockets)
    }

    #[allow(dead_code)]
    pub(crate) async fn bind_discovery_sockets_for_listener(
        &self,
        listener: &AutoDiscoveryListenerBinding,
        mut scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<Vec<AutoBoundDiscoverySocket>, String> {
        let mut sockets = Vec::new();
        for target in [
            AutoDiscoverySocketBindTarget::unicast(listener),
            AutoDiscoverySocketBindTarget::multicast(listener),
        ] {
            sockets.push(self.bind_discovery_socket_target(target, &mut scope_id_for_ifname).await?);
        }
        Ok(sockets)
    }

    async fn bind_discovery_socket_target(
        &self,
        target: AutoDiscoverySocketBindTarget,
        mut scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<AutoBoundDiscoverySocket, String> {
        match target.kind {
            AutoDiscoverySocketKind::Unicast => {
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
                Ok(AutoBoundDiscoverySocket {
                    kind: target.kind,
                    ifname: target.ifname,
                    bind_addr: socket.local_addr().unwrap_or(bind_addr),
                    multicast_group_addr: None,
                    socket,
                })
            }
            AutoDiscoverySocketKind::Multicast => {
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
                Ok(AutoBoundDiscoverySocket {
                    kind: target.kind,
                    ifname: target.ifname,
                    bind_addr: socket.local_addr().unwrap_or(resolved.bind_addr),
                    multicast_group_addr: Some(resolved.multicast_group_addr),
                    socket,
                })
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) async fn bind_data_sockets(
        &self,
        mut scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<Vec<AutoBoundDataSocket>, String> {
        let mut sockets = Vec::new();
        for target in self.data_socket_bind_targets() {
            sockets.push(self.bind_data_socket_target(target, &mut scope_id_for_ifname).await?);
        }
        Ok(sockets)
    }

    #[allow(dead_code)]
    pub(crate) async fn bind_data_socket_for_listener(
        &self,
        listener: &AutoDataListenerBinding,
        mut scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<AutoBoundDataSocket, String> {
        self.bind_data_socket_target(
            AutoDataSocketBindTarget::from_listener(listener),
            &mut scope_id_for_ifname,
        )
        .await
    }

    async fn bind_data_socket_target(
        &self,
        target: AutoDataSocketBindTarget,
        mut scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<AutoBoundDataSocket, String> {
        let bind_addr = target.resolve_bind_addr(&mut scope_id_for_ifname).map_err(|err| {
            format!("resolve auto peer data bind {} failed: {err}", target.display_bind_addr())
        })?;
        let socket = tokio::net::UdpSocket::bind(bind_addr).await.map_err(|err| {
            format!("bind auto peer data socket {} failed: {err}", target.display_bind_addr())
        })?;
        Ok(AutoBoundDataSocket {
            ifname: target.ifname,
            bind_addr: socket.local_addr().unwrap_or(bind_addr),
            socket: Arc::new(socket),
        })
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
            sockets.push(self.bind_discovery_socket_target(target, &mut scope_id_for_ifname).await?);
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
        let started_at = Instant::now();
        sockets
            .into_iter()
            .map(|socket| {
                self.spawn_discovery_receive_loop(
                    socket,
                    Arc::clone(&state),
                    events.clone(),
                    shutdown.clone(),
                    started_at,
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
        started_at: Instant,
    ) -> tokio::task::JoinHandle<()> {
        let group_id = self.config.group_id.clone();
        let initial_peering_wait = self.startup_plan.initial_peering_wait;
        tokio::spawn(async move {
            if *shutdown.borrow() {
                return;
            }
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
                                if events
                                    .send(AutoDiscoveryLoopEvent::ReceiveFailed {
                                        ifname: socket.ifname.clone(),
                                        kind: socket.kind,
                                        bind_addr: socket.bind_addr,
                                        error,
                                    })
                                    .await
                                    .is_err()
                                {
                                    log::debug!(
                                        "[daemon-auto] discovery failure receiver closed ifname={} bind_addr={}",
                                        socket.ifname,
                                        socket.bind_addr
                                    );
                                }
                                break;
                            }
                        };
                        let elapsed = started_at.elapsed();
                        if elapsed < initial_peering_wait {
                            continue;
                        }
                        let source_address = discovery_source_address(&datagram);
                        let event = {
                            let mut state = state.lock().await;
                            state.observe_authenticated_discovery_packet(
                                &datagram.payload,
                                group_id.as_bytes(),
                                &source_address,
                                &datagram.ifname,
                                elapsed,
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
        let started_at = Instant::now();
        let runtime = AutoPeerDataReceiveLoopRuntime {
            state,
            dedupe,
            transport,
            runtime_status: None,
            events,
            shutdown,
            started_at,
        };
        sockets
            .into_iter()
            .map(|socket| self.spawn_peer_data_receive_loop(socket, runtime.clone()))
            .collect()
    }
}

impl AutoDiscoveryListenerSupervisor {
    #[allow(dead_code)]
    pub(crate) fn new(
        plan: AutoDaemonStartupPlan,
        state: Arc<tokio::sync::Mutex<AutoDiscoveryState>>,
        shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> Self {
        Self {
            plan,
            state,
            shutdown,
            started_at: Instant::now(),
            listeners: BTreeMap::new(),
            pending_stops: Vec::new(),
        }
    }

    pub(crate) fn spawn_sockets(
        &mut self,
        sockets: Vec<AutoBoundDiscoverySocket>,
        events: &tokio::sync::mpsc::Sender<AutoDiscoveryLoopEvent>,
    ) {
        let mut by_ifname = BTreeMap::<String, Vec<AutoBoundDiscoverySocket>>::new();
        for socket in sockets {
            by_ifname.entry(socket.ifname.clone()).or_default().push(socket);
        }
        for (ifname, sockets) in by_ifname {
            self.spawn_listener(ifname, sockets, events);
        }
    }

    fn spawn_listener(
        &mut self,
        ifname: String,
        sockets: Vec<AutoBoundDiscoverySocket>,
        events: &tokio::sync::mpsc::Sender<AutoDiscoveryLoopEvent>,
    ) {
        let joins = sockets
            .into_iter()
            .map(|socket| {
                self.plan.spawn_discovery_receive_loop(
                    socket,
                    Arc::clone(&self.state),
                    events.clone(),
                    self.shutdown.clone(),
                    self.started_at,
                )
            })
            .collect();
        if let Some(old) = self.listeners.insert(ifname, AutoDiscoveryListenerHandle { joins }) {
            self.pending_stops.push(tokio::spawn(async move {
                old.stop().await;
            }));
        }
    }

    #[allow(dead_code)]
    pub(crate) fn spawn_bound_listener(
        &mut self,
        ifname: String,
        sockets: Vec<AutoBoundDiscoverySocket>,
        events: &tokio::sync::mpsc::Sender<AutoDiscoveryLoopEvent>,
    ) {
        self.spawn_listener(ifname, sockets, events);
    }

    #[allow(dead_code)]
    pub(crate) async fn add_listener(
        &mut self,
        listener: &AutoDiscoveryListenerBinding,
        events: &tokio::sync::mpsc::Sender<AutoDiscoveryLoopEvent>,
        mut scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<(), String> {
        let sockets = self
            .plan
            .bind_discovery_sockets_for_listener(listener, &mut scope_id_for_ifname)
            .await?;
        self.spawn_listener(listener.ifname.clone(), sockets, events);
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) async fn remove_listener(&mut self, ifname: &str) -> bool {
        let Some(old) = self.listeners.remove(ifname) else {
            return false;
        };
        old.stop().await;
        self.await_pending_stops().await;
        true
    }

    #[allow(dead_code)]
    pub(crate) fn receive_loop_count(&self) -> usize {
        self.listeners.values().map(|listener| listener.joins.len()).sum()
    }

    #[allow(dead_code)]
    pub(crate) fn pending_stop_count(&self) -> usize {
        self.pending_stops.len()
    }

    #[allow(dead_code)]
    pub(crate) async fn shutdown_all(&mut self) {
        let listeners = std::mem::take(&mut self.listeners);
        for handle in listeners.into_values() {
            handle.stop().await;
        }
        self.await_pending_stops().await;
    }

    async fn await_pending_stops(&mut self) {
        let pending_stops = std::mem::take(&mut self.pending_stops);
        for stop in pending_stops {
            if let Err(err) = stop.await {
                if !err.is_cancelled() {
                    log::warn!("[daemon-auto] discovery replacement-stop task failed: {err}");
                }
            }
        }
    }
}

impl AutoDiscoveryListenerHandle {
    async fn stop(self) {
        for join in self.joins {
            join.abort();
            if let Err(err) = join.await {
                if !err.is_cancelled() {
                    log::warn!("[daemon-auto] discovery receive loop task stopped: {err}");
                }
            }
        }
    }
}

impl AutoPeerDataListenerSupervisor {
    #[allow(dead_code)]
    pub(crate) fn new(
        plan: AutoDaemonStartupPlan,
        state: Arc<tokio::sync::Mutex<AutoDiscoveryState>>,
        dedupe: Arc<tokio::sync::Mutex<AutoInboundPacketDeduplicator>>,
        transport: Option<AutoInterfaceTransportBridge>,
        shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> Self {
        Self {
            plan,
            state,
            dedupe,
            transport,
            runtime_status: None,
            shutdown,
            started_at: Instant::now(),
            listeners: BTreeMap::new(),
            pending_stops: Vec::new(),
        }
    }

    pub(crate) fn with_runtime_status(
        mut self,
        runtime_status: Option<AutoRuntimeStatusHandle>,
    ) -> Self {
        self.runtime_status = runtime_status;
        self
    }

    pub(crate) fn spawn_sockets(
        &mut self,
        sockets: Vec<AutoBoundDataSocket>,
        events: &tokio::sync::mpsc::Sender<AutoPeerDataLoopEvent>,
    ) {
        for socket in sockets {
            self.spawn_socket(socket, events);
        }
    }

    fn spawn_socket(
        &mut self,
        socket: AutoBoundDataSocket,
        events: &tokio::sync::mpsc::Sender<AutoPeerDataLoopEvent>,
    ) {
        let ifname = socket.ifname.clone();
        let socket_handle = Arc::clone(&socket.socket);
        let runtime = AutoPeerDataReceiveLoopRuntime {
            state: Arc::clone(&self.state),
            dedupe: Arc::clone(&self.dedupe),
            transport: self.transport.clone(),
            runtime_status: self.runtime_status.clone(),
            events: events.clone(),
            shutdown: self.shutdown.clone(),
            started_at: self.started_at,
        };
        let join = self.plan.spawn_peer_data_receive_loop(socket, runtime);
        if let Some(old) =
            self.listeners.insert(ifname, AutoPeerDataListenerHandle { socket: socket_handle, join })
        {
            self.pending_stops.push(tokio::spawn(async move {
                old.stop().await;
            }));
        }
    }

    #[allow(dead_code)]
    pub(crate) fn spawn_bound_socket(
        &mut self,
        socket: AutoBoundDataSocket,
        events: &tokio::sync::mpsc::Sender<AutoPeerDataLoopEvent>,
    ) -> SocketAddr {
        let bind_addr = socket.bind_addr;
        self.spawn_socket(socket, events);
        bind_addr
    }

    #[allow(dead_code)]
    pub(crate) async fn add_listener(
        &mut self,
        listener: &AutoDataListenerBinding,
        events: &tokio::sync::mpsc::Sender<AutoPeerDataLoopEvent>,
        mut scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<SocketAddr, String> {
        let socket = self.plan.bind_data_socket_for_listener(listener, &mut scope_id_for_ifname).await?;
        let bind_addr = socket.bind_addr;
        self.spawn_socket(socket, events);
        Ok(bind_addr)
    }

    #[allow(dead_code)]
    pub(crate) async fn remove_listener(&mut self, ifname: &str) -> bool {
        let Some(old) = self.listeners.remove(ifname) else {
            return false;
        };
        let old_socket = Arc::clone(&old.socket);
        old.stop().await;
        if let Some(transport) = &self.transport {
            transport.remove_outbound_routes_for_socket(&old_socket).await;
        }
        self.await_pending_stops().await;
        true
    }

    #[allow(dead_code)]
    pub(crate) fn len(&self) -> usize {
        self.listeners.len()
    }

    #[allow(dead_code)]
    pub(crate) fn pending_stop_count(&self) -> usize {
        self.pending_stops.len()
    }

    #[allow(dead_code)]
    pub(crate) async fn restart_link_local_listener(
        &mut self,
        update: &AutoLinkLocalAddressUpdate,
        runtime_status: Option<&AutoRuntimeStatusHandle>,
        events: &tokio::sync::mpsc::Sender<AutoPeerDataLoopEvent>,
        mut scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<SocketAddr, String> {
        let socket = self
            .plan
            .bind_data_socket_for_listener(&update.listener_binding, &mut scope_id_for_ifname)
            .await?;
        let bind_addr = socket.bind_addr;
        let old = self.listeners.remove(&update.ifname);
        self.spawn_socket(socket, events);
        if let Some(old) = old {
            let old_socket = Arc::clone(&old.socket);
            old.stop().await;
            if let Some(transport) = &self.transport {
                transport.remove_outbound_routes_for_socket(&old_socket).await;
            }
        }
        self.await_pending_stops().await;
        if let Some(runtime_status) = runtime_status {
            runtime_status.record_link_local_update(Some(update));
        }
        Ok(bind_addr)
    }

    #[allow(dead_code)]
    pub(crate) async fn shutdown_all(&mut self) {
        let listeners = std::mem::take(&mut self.listeners);
        for handle in listeners.into_values() {
            handle.stop().await;
        }
        self.await_pending_stops().await;
    }

    async fn await_pending_stops(&mut self) {
        let pending_stops = std::mem::take(&mut self.pending_stops);
        for stop in pending_stops {
            if let Err(err) = stop.await {
                if !err.is_cancelled() {
                    log::warn!("[daemon-auto] peer-data replacement-stop task failed: {err}");
                }
            }
        }
    }
}

impl AutoPeerDataListenerHandle {
    async fn stop(self) {
        self.join.abort();
        if let Err(err) = self.join.await {
            if !err.is_cancelled() {
                log::warn!("[daemon-auto] peer data receive loop task stopped: {err}");
            }
        }
    }
}
