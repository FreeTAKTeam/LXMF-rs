#[derive(Debug, Clone)]
pub struct MeshtasticInterfaceHandle {
    inbound_tx: mpsc::Sender<MeshtasticReceivedFrame>,
    outbound_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<MeshtasticTransmitFrame>>>,
}

impl MeshtasticInterfaceHandle {
    pub async fn inject_received(
        &self,
        frame: MeshtasticReceivedFrame,
    ) -> Result<(), mpsc::error::SendError<MeshtasticReceivedFrame>> {
        self.inbound_tx.send(frame).await
    }

    pub async fn recv_transmit(&self) -> Option<MeshtasticTransmitFrame> {
        self.outbound_rx.lock().await.recv().await
    }
}

#[derive(Debug)]
pub struct MeshtasticInterface {
    name: String,
    config: MeshtasticInterfaceConfig,
    inbound_tx: mpsc::Sender<MeshtasticReceivedFrame>,
    inbound_rx: Arc<Mutex<Option<mpsc::Receiver<MeshtasticReceivedFrame>>>>,
    outbound_tx: mpsc::Sender<MeshtasticTransmitFrame>,
    outbound_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<MeshtasticTransmitFrame>>>,
    runtime_status: Arc<Mutex<MeshtasticTunnelStatus>>,
}

impl MeshtasticInterface {
    #[must_use]
    pub fn new(name: impl Into<String>, config: MeshtasticInterfaceConfig) -> Self {
        let (inbound_tx, inbound_rx) = mpsc::channel(MESHTASTIC_CHANNEL_CAPACITY);
        let (outbound_tx, outbound_rx) = mpsc::channel(MESHTASTIC_CHANNEL_CAPACITY);
        Self {
            name: name.into(),
            config,
            inbound_tx,
            inbound_rx: Arc::new(Mutex::new(Some(inbound_rx))),
            outbound_tx,
            outbound_rx: Arc::new(tokio::sync::Mutex::new(outbound_rx)),
            runtime_status: Arc::new(Mutex::new(MeshtasticTunnelStatus::default())),
        }
    }

    #[must_use]
    pub fn handle(&self) -> MeshtasticInterfaceHandle {
        MeshtasticInterfaceHandle {
            inbound_tx: self.inbound_tx.clone(),
            outbound_rx: self.outbound_rx.clone(),
        }
    }

    #[must_use]
    pub fn runtime_status_json(&self) -> serde_json::Value {
        self.runtime_status
            .lock()
            .expect("meshtastic status mutex poisoned")
            .to_json()
    }

    #[must_use]
    pub fn runtime_status_handle(&self) -> MeshtasticRuntimeStatusHandle {
        MeshtasticRuntimeStatusHandle { inner: self.runtime_status.clone() }
    }

    pub async fn spawn(context: InterfaceContext<Self>) {
        let iface_stop = context.channel.stop.clone();
        let iface_address = context.channel.address;
        let (rx_channel, mut tx_channel) = context.channel.split();
        let (name, config, mut inbound_rx, outbound_tx, runtime_status) = {
            let guard = context.inner.lock().expect("meshtastic interface mutex poisoned");
            let inbound_rx = guard
                .inbound_rx
                .lock()
                .expect("meshtastic inbound mutex poisoned")
                .take();
            (
                guard.name.clone(),
                guard.config.clone(),
                inbound_rx,
                guard.outbound_tx.clone(),
                guard.runtime_status.clone(),
            )
        };
        let Some(ref mut inbound_rx) = inbound_rx else {
            iface_stop.cancel();
            return;
        };
        let mut tunnel = MeshtasticTunnel::new(config.clone());
        let mut send_tick = tokio::time::interval(config.send_delay);
        send_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = context.cancel.cancelled() => break,
                _ = iface_stop.cancelled() => break,
                received = inbound_rx.recv() => {
                    let Some(received) = received else { break };
                    if !process_received_for_interface(
                        &mut tunnel,
                        received,
                        iface_address,
                        &rx_channel,
                    ).await {
                        break;
                    }
                    record_meshtastic_status(&runtime_status, tunnel.status());
                }
                Some(message) = tx_channel.recv() => {
                    if let Err(err) = queue_packet_for_meshtastic(&mut tunnel, message) {
                        tunnel.status.last_error = Some(err);
                    }
                    record_meshtastic_status(&runtime_status, tunnel.status());
                }
                _ = send_tick.tick() => {
                    if let Some(frame) = tunnel.next_transmit() {
                        if let Err(err) = outbound_tx.send(frame).await {
                            tunnel.status.last_error = Some(err.to_string());
                            log::warn!(
                                "meshtastic_interface outbound queue closed name={} iface={}",
                                name,
                                iface_address
                            );
                            break;
                        }
                    }
                    record_meshtastic_status(&runtime_status, tunnel.status());
                }
            }
        }

        iface_stop.cancel();
    }
}

impl Interface for MeshtasticInterface {
    fn mtu() -> usize {
        DEFAULT_MESHTASTIC_HW_MTU
    }
}

async fn process_received_for_interface(
    tunnel: &mut MeshtasticTunnel,
    received: MeshtasticReceivedFrame,
    iface_address: AddressHash,
    rx_channel: &mpsc::Sender<RxMessage>,
) -> bool {
    match tunnel.process_received(received) {
        Ok(Some(data)) => match Packet::from_bytes(&data) {
            Ok(packet) => {
                if rx_channel
                    .send(RxMessage {
                        address: iface_address,
                        packet,
                        source: IfaceSource::None,
                    })
                    .await
                    .is_err()
                {
                    tunnel.status.last_error = Some("transport receive queue closed".to_string());
                    log::warn!(
                        "meshtastic_interface receive queue closed iface={iface_address}"
                    );
                    return false;
                }
            }
            Err(err) => {
                tunnel.status.decode_errors = tunnel.status.decode_errors.saturating_add(1);
                tunnel.status.last_error = Some(format!("{err:?}"));
            }
        },
        Ok(None) => {}
        Err(err) => {
            tunnel.status.decode_errors = tunnel.status.decode_errors.saturating_add(1);
            tunnel.status.last_error = Some(err);
        }
    }
    true
}

fn queue_packet_for_meshtastic(
    tunnel: &mut MeshtasticTunnel,
    message: TxMessage,
) -> Result<(), String> {
    let data = message.packet.to_bytes().map_err(|err| format!("{err:?}"))?;
    tunnel.queue_outgoing_packet(&data)
}

fn record_meshtastic_status(
    runtime_status: &Arc<Mutex<MeshtasticTunnelStatus>>,
    status: MeshtasticTunnelStatus,
) {
    *runtime_status.lock().expect("meshtastic status mutex poisoned") = status;
}

pub fn spawn_meshtastic(
    mgr: &mut InterfaceManager,
    name: impl Into<String>,
    config: MeshtasticInterfaceConfig,
) -> (AddressHash, MeshtasticInterfaceHandle) {
    let iface = MeshtasticInterface::new(name, config);
    let handle = iface.handle();
    let address = mgr.spawn_as(iface, MeshtasticInterface::spawn, IfaceRole::Unicast);
    (address, handle)
}
