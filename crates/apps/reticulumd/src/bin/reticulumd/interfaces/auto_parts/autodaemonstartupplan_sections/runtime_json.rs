impl AutoDaemonStartupPlan {

    pub(crate) fn runtime_json(&self) -> JsonValue {
        let mut initial_peer_announces = Vec::new();
        let _ = self.send_initial_peer_announces(|datagram| {
            initial_peer_announces.push(peering_datagram_json(datagram));
            Ok(())
        });
        json!({
            "auto_runtime_status": "complete",
            "platform": platform_name(self.platform),
            "group_id": self.config.group_id.clone(),
            "candidate_devices": self.candidates.iter().map(candidate_json).collect::<Vec<_>>(),
            "adopted_devices": self.adopted_devices.iter().map(adopted_json).collect::<Vec<_>>(),
            "startup_plan": startup_plan_json(&self.startup_plan),
            "planned_initial_peer_announce_count": initial_peer_announces.len(),
            "planned_repeat_peer_announce_scheduler_count": usize::from(!self.adopted_devices.is_empty()),
            "planned_peer_job_scheduler_count": usize::from(!self.adopted_devices.is_empty()),
            "initial_peer_announces": initial_peer_announces,
            "native_scope_id_source": "if-addrs interface index",
            "planned_discovery_receive_loop_count": self.discovery_socket_bind_targets().len(),
            "planned_discovery_socket_binds": self.discovery_socket_bind_targets().iter().map(discovery_socket_bind_json).collect::<Vec<_>>(),
            "planned_data_receive_loop_count": self.data_socket_bind_targets().len(),
            "planned_data_socket_binds": self.data_socket_bind_targets().iter().map(data_socket_bind_json).collect::<Vec<_>>(),
        })
    }

    pub(crate) fn initial_peer_announce_datagrams(&self) -> Vec<AutoPeerAnnounceDatagram> {
        self.peering_packets.iter().map(AutoPeerAnnounceDatagram::from).collect()
    }

    #[allow(dead_code)]
    pub(crate) fn due_multicast_peer_announce_datagrams(
        &self,
        state: &mut AutoDiscoveryState,
        now: core::time::Duration,
    ) -> Vec<AutoPeerAnnounceDatagram> {
        let timing = AutoInterfaceTiming::for_platform(self.platform);
        state
            .run_multicast_announce_job(
                &self.config,
                &self.adopted_devices,
                now,
                timing.announce_interval,
            )
            .iter()
            .map(AutoPeerAnnounceDatagram::from)
            .collect()
    }

    pub(crate) fn discovery_socket_bind_targets(&self) -> Vec<AutoDiscoverySocketBindTarget> {
        self.startup_plan
            .discovery_listeners
            .iter()
            .flat_map(|listener| {
                [
                    AutoDiscoverySocketBindTarget::unicast(listener),
                    AutoDiscoverySocketBindTarget::multicast(listener),
                ]
            })
            .collect()
    }

    pub(crate) fn data_socket_bind_targets(&self) -> Vec<AutoDataSocketBindTarget> {
        self.startup_plan
            .data_listeners
            .iter()
            .map(AutoDataSocketBindTarget::from_listener)
            .collect()
    }

    #[allow(dead_code)]
    pub(crate) fn discovery_state(&self) -> AutoDiscoveryState {
        AutoDiscoveryState::from_timing(
            self.adopted_devices.clone(),
            AutoInterfaceTiming::for_platform(self.platform),
        )
    }

    #[allow(dead_code)]
    pub(crate) fn process_discovery_datagram(
        &self,
        state: &mut AutoDiscoveryState,
        datagram: AutoDiscoveryDatagram,
        now: core::time::Duration,
    ) -> Result<AutoProcessedDiscoveryDatagram, AutoDiscoveryRejectReason> {
        let source_address = discovery_source_address(&datagram);
        let event = state.observe_authenticated_discovery_packet(
            &datagram.payload,
            self.config.group_id.as_bytes(),
            &source_address,
            &datagram.ifname,
            now,
        )?;
        Ok(AutoProcessedDiscoveryDatagram { datagram, source_address, event })
    }

    #[allow(dead_code)]
    pub(crate) fn process_peer_data_datagram(
        &self,
        state: &mut AutoDiscoveryState,
        dedupe: &mut AutoInboundPacketDeduplicator,
        datagram: AutoPeerDataDatagram,
        now: core::time::Duration,
    ) -> AutoProcessedPeerDataDatagram {
        let peer_address = peer_data_source_address(&datagram);
        let decision =
            state.handle_spawned_peer_inbound(dedupe, &peer_address, &datagram.payload, now);
        AutoProcessedPeerDataDatagram { datagram, peer_address, decision }
    }

    pub(crate) fn send_initial_peer_announces(
        &self,
        mut send: impl FnMut(&AutoPeerAnnounceDatagram) -> Result<(), String>,
    ) -> Result<usize, String> {
        let datagrams = self.initial_peer_announce_datagrams();
        Self::send_peer_announce_datagrams(&datagrams, "auto peer announce", &mut send)
    }

    #[allow(dead_code)]
    pub(crate) fn run_multicast_peer_announce_job(
        &self,
        state: &mut AutoDiscoveryState,
        now: core::time::Duration,
        mut send: impl FnMut(&AutoPeerAnnounceDatagram) -> Result<(), String>,
    ) -> Result<usize, String> {
        let datagrams = self.due_multicast_peer_announce_datagrams(state, now);
        Self::send_peer_announce_datagrams(&datagrams, "auto multicast peer announce", &mut send)
    }

    #[allow(dead_code)]
    pub(crate) fn run_peer_job(
        &self,
        state: &mut AutoDiscoveryState,
        now: core::time::Duration,
        mut send: impl FnMut(&AutoPeerAnnounceDatagram) -> Result<(), String>,
    ) -> Result<AutoPeerJobRuntimeSummary, String> {
        let (summary, datagrams) = self.run_peer_job_datagrams(state, now);
        Self::send_peer_announce_datagrams(&datagrams, "auto reverse peer announce", &mut send)?;
        Ok(summary)
    }

    fn run_peer_job_datagrams(
        &self,
        state: &mut AutoDiscoveryState,
        now: core::time::Duration,
    ) -> (AutoPeerJobRuntimeSummary, Vec<AutoPeerAnnounceDatagram>) {
        let timing = AutoInterfaceTiming::for_platform(self.platform);
        let run = state.run_peer_job(
            &self.config,
            &self.adopted_devices,
            now,
            timing.multicast_echo_timeout,
        );
        let datagrams = run
            .reverse_peering_packets
            .iter()
            .map(AutoPeerAnnounceDatagram::from)
            .collect::<Vec<_>>();
        (
            AutoPeerJobRuntimeSummary {
                expired_peer_count: run.expired_peers.len(),
                reverse_peer_announce_count: datagrams.len(),
                missing_initial_echo_count: run.missing_initial_echo_interfaces.len(),
                carrier_event_count: run.carrier_events.len(),
            },
            datagrams,
        )
    }

    fn send_peer_announce_datagrams(
        datagrams: &[AutoPeerAnnounceDatagram],
        label: &str,
        mut send: impl FnMut(&AutoPeerAnnounceDatagram) -> Result<(), String>,
    ) -> Result<usize, String> {
        let mut sent = 0;
        for datagram in datagrams {
            send(datagram).map_err(|err| {
                format!(
                    "send {label} {}/{} to {} failed: {err}",
                    sent + 1,
                    datagrams.len(),
                    datagram.destination_socket_target()
                )
            })?;
            sent += 1;
        }
        Ok(sent)
    }

    // Shared by startup and tests to send a fixed set of peer-announce
    // datagrams through a caller-owned UDP socket.
    #[allow(dead_code)]
    pub(crate) async fn send_initial_peer_announces_with_udp_socket(
        &self,
        socket: &tokio::net::UdpSocket,
        mut scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<usize, String> {
        let datagrams = self.initial_peer_announce_datagrams();
        self.send_peer_announce_datagrams_with_udp_socket(
            &datagrams,
            "auto peer announce",
            socket,
            &mut scope_id_for_ifname,
        )
        .await
    }

    #[allow(dead_code)]
    async fn send_peer_announce_datagrams_with_udp_socket(
        &self,
        datagrams: &[AutoPeerAnnounceDatagram],
        label: &str,
        socket: &tokio::net::UdpSocket,
        mut scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<usize, String> {
        let mut sent = 0;
        for datagram in datagrams {
            let target = datagram.socket_target();
            let destination =
                target.resolve_socket_addr(&mut scope_id_for_ifname).map_err(|err| {
                    format!(
                        "resolve {label} {}/{} target {} failed: {err}",
                        sent + 1,
                        datagrams.len(),
                        target.display()
                    )
                })?;
            let sent_bytes =
                socket.send_to(&datagram.payload, destination).await.map_err(|err| {
                    format!(
                        "send {label} {}/{} to {} failed: {err}",
                        sent + 1,
                        datagrams.len(),
                        target.display()
                    )
                })?;
            if sent_bytes != datagram.payload.len() {
                return Err(format!(
                    "send {label} {}/{} to {} sent {sent_bytes}/{} byte(s)",
                    sent + 1,
                    datagrams.len(),
                    target.display(),
                    datagram.payload.len()
                ));
            }
            sent += 1;
        }
        Ok(sent)
    }

    #[allow(dead_code)]
    pub(crate) async fn send_initial_peer_announces_with_native_scope_ids(
        &self,
        socket: &tokio::net::UdpSocket,
    ) -> Result<usize, String> {
        let resolver = AutoInterfaceIndexResolver::from_system()?;
        self.send_initial_peer_announces_with_udp_socket(socket, |ifname| resolver.resolve(ifname))
            .await
    }

    #[allow(dead_code)]
    pub(crate) async fn bind_discovery_sockets_with_native_scope_ids(
        &self,
    ) -> Result<Vec<AutoBoundDiscoverySocket>, String> {
        let resolver = AutoInterfaceIndexResolver::from_system()?;
        let mut sockets =
            self.bind_unicast_discovery_sockets(|ifname| resolver.resolve(ifname)).await?;
        sockets.extend(
            self.bind_multicast_discovery_sockets(|ifname| resolver.resolve(ifname)).await?,
        );
        Ok(sockets)
    }

    #[allow(dead_code)]
    pub(crate) async fn bind_data_sockets_with_native_scope_ids(
        &self,
    ) -> Result<Vec<AutoBoundDataSocket>, String> {
        let resolver = AutoInterfaceIndexResolver::from_system()?;
        self.bind_data_sockets(|ifname| resolver.resolve(ifname)).await
    }

    #[allow(dead_code)]
    pub(crate) async fn spawn_discovery_runtime_with_native_scope_ids(
        &self,
    ) -> Result<AutoDiscoveryRuntimeSummary, String> {
        self.spawn_discovery_runtime_with_native_scope_ids_and_transport(None).await
    }

    #[allow(dead_code)]
    pub(crate) async fn spawn_discovery_runtime_with_native_scope_ids_and_transport(
        &self,
        transport_runtime: Option<AutoInterfaceTransportRuntime>,
    ) -> Result<AutoDiscoveryRuntimeSummary, String> {
        let (transport_bridge, transport_tx_channel) = match transport_runtime {
            Some(runtime) => {
                let (bridge, tx_channel) = runtime.split();
                (Some(bridge), Some(tx_channel))
            }
            None => (None, None),
        };
        let sockets = self.bind_discovery_sockets_with_native_scope_ids().await?;
        let bound_socket_count = sockets.len();
        let data_sockets = self.bind_data_sockets_with_native_scope_ids().await?;
        let data_socket_count = data_sockets.len();
        let state = Arc::new(tokio::sync::Mutex::new(self.discovery_state()));
        let dedupe = Arc::new(tokio::sync::Mutex::new(AutoInboundPacketDeduplicator::from_timing(
            AutoInterfaceTiming::for_platform(self.platform),
        )));
        let announce_socket = if self.adopted_devices.is_empty() {
            None
        } else {
            Some(self.bind_peer_announce_runtime_socket().await?)
        };
        let initial_peer_announce_count = if let Some(socket) = &announce_socket {
            self.send_due_multicast_peer_announces_with_runtime_socket(
                Arc::clone(&state),
                Arc::clone(socket),
                core::time::Duration::ZERO,
            )
            .await?
        } else {
            0
        };
        if sockets.is_empty() {
            return Ok(AutoDiscoveryRuntimeSummary {
                bound_socket_count,
                receive_loop_count: 0,
                initial_peer_announce_count,
                repeat_peer_announce_scheduler_count: 0,
                peer_job_scheduler_count: 0,
                data_socket_count,
                data_receive_loop_count: 0,
            });
        }
        let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(bound_socket_count * 8);
        let data_events_capacity = usize::max(data_socket_count * 8, 1);
        let (data_events_tx, mut data_events_rx) = tokio::sync::mpsc::channel(data_events_capacity);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let handles = self.spawn_discovery_receive_loops(
            sockets,
            Arc::clone(&state),
            events_tx,
            shutdown_rx.clone(),
        );
        let receive_loop_count = handles.len();
        let data_handles = self.spawn_peer_data_receive_loops(
            data_sockets,
            Arc::clone(&state),
            dedupe,
            transport_bridge.clone(),
            data_events_tx,
            shutdown_rx.clone(),
        );
        let data_receive_loop_count = data_handles.len();
        let transport_tx_handle = transport_tx_channel.map(|tx_channel| {
            self.spawn_peer_data_transport_tx_loop(
                transport_bridge.expect("transport bridge exists with tx channel"),
                tx_channel,
                shutdown_rx.clone(),
            )
        });
        let scheduler_handle = announce_socket.as_ref().map(|socket| {
            self.spawn_repeat_peer_announce_scheduler(
                Arc::clone(&state),
                Arc::clone(socket),
                shutdown_rx.clone(),
            )
        });
        let repeat_peer_announce_scheduler_count = usize::from(scheduler_handle.is_some());
        let peer_job_scheduler_handle = announce_socket.as_ref().map(|socket| {
            self.spawn_peer_job_scheduler(
                Arc::clone(&state),
                Arc::clone(socket),
                shutdown_rx.clone(),
            )
        });
        let peer_job_scheduler_count = usize::from(peer_job_scheduler_handle.is_some());
        tokio::spawn(async move {
            let _shutdown_guard = shutdown_tx;
            let mut discovery_events_open = true;
            let mut data_events_open = true;
            while discovery_events_open || data_events_open {
                tokio::select! {
                    event = events_rx.recv(), if discovery_events_open => {
                        match event {
                            Some(event) => log_auto_discovery_loop_event(event),
                            None => discovery_events_open = false,
                        }
                    }
                    event = data_events_rx.recv(), if data_events_open => {
                        match event {
                            Some(event) => log_auto_peer_data_loop_event(event),
                            None => data_events_open = false,
                        }
                    }
                }
            }
            for handle in handles {
                if let Err(err) = handle.await {
                    log::warn!("[daemon-auto] discovery receive loop task stopped: {err}");
                }
            }
            for handle in data_handles {
                if let Err(err) = handle.await {
                    log::warn!("[daemon-auto] peer data receive loop task stopped: {err}");
                }
            }
            if let Some(handle) = scheduler_handle {
                if let Err(err) = handle.await {
                    log::warn!("[daemon-auto] repeat peer-announce scheduler stopped: {err}");
                }
            }
            if let Some(handle) = peer_job_scheduler_handle {
                if let Err(err) = handle.await {
                    log::warn!("[daemon-auto] peer-job scheduler stopped: {err}");
                }
            }
            if let Some(handle) = transport_tx_handle {
                if let Err(err) = handle.await {
                    log::warn!("[daemon-auto] peer data transport tx loop stopped: {err}");
                }
            }
        });
        Ok(AutoDiscoveryRuntimeSummary {
            bound_socket_count,
            receive_loop_count,
            initial_peer_announce_count,
            repeat_peer_announce_scheduler_count,
            peer_job_scheduler_count,
            data_socket_count,
            data_receive_loop_count,
        })
    }

    async fn bind_peer_announce_runtime_socket(
        &self,
    ) -> Result<Arc<tokio::net::UdpSocket>, String> {
        let socket = tokio::net::UdpSocket::bind("[::]:0").await.map_err(|err| {
            format!("bind auto peer-announce scheduler socket [::]:0 failed: {err}")
        })?;
        Ok(Arc::new(socket))
    }
}
