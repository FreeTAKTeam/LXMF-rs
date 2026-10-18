impl RNodeMultiInterface {
    #[must_use]
    pub fn new<T: Into<String>>(
        device: T,
        iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    ) -> Self {
        let (management_frame_tx, management_frame_rx) = rnode_multi_management_channel();
        Self {
            endpoint: RNodeMultiEndpoint::Serial {
                device: device.into(),
                baud_rate: DEFAULT_BAUD_RATE,
            },
            subinterfaces: Vec::new(),
            id_beacon: None,
            mtu: DEFAULT_MTU,
            runtime_status: Arc::new(Mutex::new(RNodeMultiRuntimeStatus::from_subinterfaces(&[]))),
            iface_manager,
            management_frame_tx,
            management_frame_rx,
        }
    }

    #[must_use]
    pub fn new_tcp<T: Into<String>>(
        addr: T,
        iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    ) -> Self {
        let (management_frame_tx, management_frame_rx) = rnode_multi_management_channel();
        Self {
            endpoint: RNodeMultiEndpoint::Tcp { addr: addr.into() },
            subinterfaces: Vec::new(),
            id_beacon: None,
            mtu: DEFAULT_MTU,
            runtime_status: Arc::new(Mutex::new(RNodeMultiRuntimeStatus::from_subinterfaces(&[]))),
            iface_manager,
            management_frame_tx,
            management_frame_rx,
        }
    }

    #[must_use]
    pub fn with_baud_rate(mut self, baud_rate: u32) -> Self {
        if let RNodeMultiEndpoint::Serial { baud_rate: current, .. } = &mut self.endpoint {
            *current = baud_rate.max(1);
        }
        self
    }

    #[must_use]
    pub fn with_subinterfaces(mut self, subinterfaces: Vec<RNodeMultiSubInterfaceConfig>) -> Self {
        self.runtime_status =
            Arc::new(Mutex::new(RNodeMultiRuntimeStatus::from_subinterfaces(&subinterfaces)));
        self.subinterfaces = subinterfaces;
        self
    }

    #[must_use]
    pub fn with_id_beacon(mut self, id_beacon: Option<KissIdBeaconConfig>) -> Self {
        self.id_beacon = id_beacon;
        self
    }

    #[must_use]
    pub fn with_mtu(mut self, mtu: usize) -> Self {
        self.mtu = mtu.max(256);
        self
    }

    #[must_use]
    pub fn subinterfaces(&self) -> &[RNodeMultiSubInterfaceConfig] {
        &self.subinterfaces
    }

    #[must_use]
    pub fn id_beacon(&self) -> Option<&KissIdBeaconConfig> {
        self.id_beacon.as_ref()
    }

    #[must_use]
    pub fn mtu_value(&self) -> usize {
        self.mtu
    }

    #[must_use]
    pub fn endpoint(&self) -> &str {
        self.endpoint.label()
    }

    #[must_use]
    pub fn baud_rate(&self) -> Option<u32> {
        self.endpoint.baud_rate()
    }

    #[must_use]
    pub fn runtime_status_handle(&self) -> RNodeMultiRuntimeStatusHandle {
        RNodeMultiRuntimeStatusHandle { inner: self.runtime_status.clone() }
    }

    #[must_use]
    pub fn rnode_management_handle(&self) -> RNodeMultiManagementHandle {
        RNodeMultiManagementHandle { tx: self.management_frame_tx.clone() }
    }

    pub fn preflight_open(&self) -> Result<(), String> {
        match &self.endpoint {
            RNodeMultiEndpoint::Serial { device, baud_rate } => {
                tokio_serial::new(device.clone(), *baud_rate)
                    .data_bits(DataBits::Eight)
                    .parity(Parity::None)
                    .stop_bits(StopBits::One)
                    .flow_control(FlowControl::None)
                    .open_native_async()
                    .map(|_| ())
                    .map_err(|err| {
                        format!(
                            "rnode_multi preflight open failed device={} baud_rate={} err={}",
                            device, baud_rate, err
                        )
                    })
            }
            RNodeMultiEndpoint::Tcp { addr } => preflight_tcp_connect(addr),
        }
    }

    pub async fn spawn(context: InterfaceContext<Self>) {
        let iface_stop = context.channel.stop.clone();
        let parent_iface = context.channel.address;
        let ifac_state = context.channel.ifac_state.clone();
        let ifac_violations = context.channel.ifac_violations.clone();
        let (
            endpoint,
            subinterfaces,
            id_beacon,
            mtu,
            runtime_status,
            iface_manager,
            management_frame_rx,
        ) = {
            let guard = context.inner.lock().expect("rnode multi interface mutex poisoned");
            (
                guard.endpoint.clone(),
                guard.subinterfaces.clone(),
                guard.id_beacon.clone(),
                guard.mtu,
                guard.runtime_status.clone(),
                guard.iface_manager.clone(),
                guard.management_frame_rx.clone(),
            )
        };
        let mut vport_map = BTreeMap::new();
        {
            let mut manager = iface_manager.lock().await;
            for subinterface in &subinterfaces {
                if let Some(address) =
                    manager.register_virtual_iface(parent_iface, IfaceRole::VirtualUnicast)
                {
                    manager.set_outgoing(address, subinterface.outgoing);
                    vport_map.insert(address, subinterface.vport);
                }
            }
        }

        let (rx_channel, tx_channel) = context.channel.split();
        let tx_channel = Arc::new(tokio::sync::Mutex::new(tx_channel));

        loop {
            if context.cancel.is_cancelled() || iface_stop.is_cancelled() {
                break;
            }

            update_rnode_multi_runtime_state(&runtime_status, "opening", None);
            match &endpoint {
                RNodeMultiEndpoint::Serial { device, baud_rate } => {
                    let port = match tokio_serial::new(device.clone(), *baud_rate)
                        .data_bits(DataBits::Eight)
                        .parity(Parity::None)
                        .stop_bits(StopBits::One)
                        .flow_control(FlowControl::None)
                        .open_native_async()
                    {
                        Ok(port) => port,
                        Err(err) => {
                            log::warn!(
                                "failed to open RNodeMulti serial device={} baud_rate={} err={}",
                                device,
                                baud_rate,
                                err
                            );
                            update_rnode_multi_runtime_state(
                                &runtime_status,
                                "open_failed",
                                Some(err.to_string()),
                            );
                            tokio::select! {
                                _ = context.cancel.cancelled() => break,
                                _ = iface_stop.cancelled() => break,
                                _ = tokio::time::sleep(RECONNECT_WAIT) => {}
                            }
                            continue;
                        }
                    };

                    log::info!(
                        "opened RNodeMulti serial device={} baud_rate={} iface={} subinterfaces={}",
                        device,
                        baud_rate,
                        parent_iface,
                        subinterfaces.len()
                    );

                    run_rnode_multi_stream_with_ifac(
                        port,
                        rnode_multi_stream_options(
                            parent_iface,
                            device.clone(),
                            &subinterfaces,
                            mtu,
                            RNodeMultiStreamRuntime {
                                status: runtime_status.clone(),
                                management_frame_rx: management_frame_rx.clone(),
                            },
                            vport_map.clone(),
                            id_beacon.clone(),
                        ),
                        context.cancel.clone(),
                        iface_stop.clone(),
                        rx_channel.clone(),
                        tx_channel.clone(),
                        IfacRuntime::from_parts(ifac_state.clone(), ifac_violations.clone()),
                    )
                    .await;
                }
                RNodeMultiEndpoint::Tcp { addr } => {
                    let stream = match TcpStream::connect(addr.clone()).await {
                        Ok(stream) => stream,
                        Err(err) => {
                            log::warn!(
                                "failed to connect RNodeMulti tcp addr={} err={}",
                                addr,
                                err
                            );
                            update_rnode_multi_runtime_state(
                                &runtime_status,
                                "open_failed",
                                Some(err.to_string()),
                            );
                            tokio::select! {
                                _ = context.cancel.cancelled() => break,
                                _ = iface_stop.cancelled() => break,
                                _ = tokio::time::sleep(RECONNECT_WAIT) => {}
                            }
                            continue;
                        }
                    };

                    log::info!(
                        "opened RNodeMulti tcp addr={} iface={} subinterfaces={}",
                        addr,
                        parent_iface,
                        subinterfaces.len()
                    );

                    run_rnode_multi_stream_with_ifac(
                        stream,
                        rnode_multi_stream_options(
                            parent_iface,
                            addr.clone(),
                            &subinterfaces,
                            mtu,
                            RNodeMultiStreamRuntime {
                                status: runtime_status.clone(),
                                management_frame_rx: management_frame_rx.clone(),
                            },
                            vport_map.clone(),
                            id_beacon.clone(),
                        ),
                        context.cancel.clone(),
                        iface_stop.clone(),
                        rx_channel.clone(),
                        tx_channel.clone(),
                        IfacRuntime::from_parts(ifac_state.clone(), ifac_violations.clone()),
                    )
                    .await;
                }
            }
        }

        update_rnode_multi_runtime_state(&runtime_status, "stopped", None);
        iface_stop.cancel();
        cleanup_rnode_multi_virtual_ifaces(&iface_manager, &vport_map).await;
    }
}

fn preflight_tcp_connect(addr: &str) -> Result<(), String> {
    let socket_addr = addr
        .to_socket_addrs()
        .map_err(|err| format!("rnode_multi tcp preflight resolve failed addr={addr} err={err}"))?
        .next()
        .ok_or_else(|| format!("rnode_multi tcp preflight resolve failed addr={addr}"))?;
    StdTcpStream::connect_timeout(&socket_addr, Duration::from_secs(3))
        .map(|_| ())
        .map_err(|err| format!("rnode_multi tcp preflight connect failed addr={addr} err={err}"))
}

async fn cleanup_rnode_multi_virtual_ifaces(
    iface_manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
    vport_map: &BTreeMap<AddressHash, u8>,
) {
    if vport_map.is_empty() {
        return;
    }

    let mut manager = iface_manager.lock().await;
    for iface in vport_map.keys() {
        if !manager.stop_interface(*iface) {
            log::debug!("RNodeMulti virtual interface already absent iface={iface}");
        }
    }
}

fn rnode_multi_stream_options(
    parent_iface: AddressHash,
    device: String,
    subinterfaces: &[RNodeMultiSubInterfaceConfig],
    mtu: usize,
    runtime: RNodeMultiStreamRuntime,
    vport_map: BTreeMap<AddressHash, u8>,
    id_beacon: Option<KissIdBeaconConfig>,
) -> RNodeMultiStreamOptions {
    RNodeMultiStreamOptions {
        parent_iface,
        device,
        subinterfaces: subinterfaces.to_vec(),
        runtime_status: runtime.status,
        vport_map,
        mtu,
        startup_probe: Some(RNodeMultiStartupProbe::from_subinterfaces(subinterfaces)),
        initial_frames: rnode_multi_initial_frames(subinterfaces),
        shutdown_frames: rnode_multi_shutdown_frames(subinterfaces),
        id_beacon,
        management_frame_rx: runtime.management_frame_rx,
    }
}

struct RNodeMultiStreamRuntime {
    status: Arc<Mutex<RNodeMultiRuntimeStatus>>,
    management_frame_rx: RNodeMultiManagementFrameReceiver,
}

impl Interface for RNodeMultiInterface {
    fn ifac_default_size_bytes() -> usize {
        8
    }

    fn mtu() -> usize {
        DEFAULT_MTU
    }

    fn configured_mtu(&self) -> usize {
        self.mtu
    }
}
